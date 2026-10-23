-- autorun_approach_hazard.lua
--
-- How often does the 0x19 approach park's precondition arise in ordinary
-- play? The park needs a monster to start an out-of-reach attack while
-- its anim event byte +0x1DC still carries bit 2 (the exit-to-idle flag
-- a hit reaction raises; docs/subsystems/battle-action-exit-gates.md).
-- This probe drives a fight by itself and records, for every actor, when
-- bit 2 goes up and when it comes down, against the action state the
-- battle was in - so the bit's lifetime can be read off the capture
-- instead of argued from the code.
--
-- Poll-only (no breakpoints): runs under --fast and --timing.
--
-- Pad driver: a command-flow-aware masher keyed on ctx[+0x06]
-- (docs/subsystems/battle-command-flow.md): at the command ring (0x28)
-- the choice LEGAIA_PLAN names for the member in ctx[+0x13] (A = Left/Attack,
-- M = Right/magic window, first entry), Circle at the run confirm (0x32),
-- Cross at every other prompt (Begin, Auto, target cursor, spell list,
-- commit confirm). A spell window it cannot leave backs out to Attack.
--
-- Env:
--   LEGAIA_SSTATE   state to load (a battle at or before a round prompt)
--   LEGAIA_FRAMES   vsyncs to run (default 20000)
--   LEGAIA_PLAN     per-member ring choice, e.g. "AAA" or "AMA" (default AAA);
--                   S = Down/Spirit (a short action that touches no monster)
--   LEGAIA_POKES    one-shot u16 pokes "V:SLOT:OFF:VAL[,...]" (hex OFF/VAL),
--                   e.g. "5:3:14C:FA0" sets seat 3's HP to 4000 at vsync 5.
--                   A poked run is a poked run: say so wherever it is cited.
--   LEGAIA_QUIT_ON_PARK  vsyncs to keep running after a park (default 600)
--   LEGAIA_PLAN_ALT a second plan used on odd rounds (default = LEGAIA_PLAN)
--
-- Output (probe.out_path):
--   anim.csv    vsync,ctx6,ctx7,acting,seat,d9,da,dc,hp  - change-triggered
--               per seat (every seat 0..7 whose anim bytes moved)
--   events.csv  vsync,kind,ctx7,acting,seat,dc,d9,da,target,x,z,tx,tz
--               kind = bit2_up / bit2_down / approach (first vsync of a
--               state-0x19 approach: the hazard is ARMED when dc & 4)
--               / park (0x19 with d9 == da == 0 held 120 vsyncs; the probe
--               saves park.sstate there and quits LEGAIA_QUIT_ON_PARK later)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 20000)
local PLAN = string.upper(probe.getenv("LEGAIA_PLAN", "AAA"))
local PLAN_ALT = string.upper(probe.getenv("LEGAIA_PLAN_ALT", PLAN))
local QUIT_AFTER_PARK = probe.getenv_num("LEGAIA_QUIT_ON_PARK", 600)
local pokes = {}
for item in string.gmatch(probe.getenv("LEGAIA_POKES", ""), "[^,]+") do
    local v, slot, off, val = string.match(item, "^%s*(%d+):(%d+):(%x+):(%x+)%s*$")
    if v ~= nil then
        pokes[#pokes + 1] = { at = tonumber(v), slot = tonumber(slot),
            off = tonumber(off, 16), val = tonumber(val, 16) }
    end
end
local parked_at = nil

local MODE_VA = 0x8007B83C
local CTX_PTR = 0x8007BD24
local ACTORS = 0x801C9370

local function u8(a) return probe.read_u8(a) or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function i16(a) local v = u16(a); return v >= 0x8000 and v - 0x10000 or v end
local function in_ram(a) return a >= 0x80000000 and a < 0x80200000 end

local anim = probe.csv_open(probe.out_path("anim.csv"),
    "vsync,ctx6,ctx7,acting,seat,d9,da,dc,hp")
local events = probe.csv_open(probe.out_path("events.csv"),
    "vsync,kind,ctx7,acting,seat,dc,d9,da,target,x,z,tx,tz")

-- Summon band (ctx[+0x07] 0x32..0x38): per-vsync anim-tick inputs of
-- every monster seat, for "why does a carried reaction clip never end?".
-- node = actor[+0x22C]; FUN_80047430 skips a monster whose
-- node[+0x74] & 0xFFFFFF is 0 (0x800474D4) and advances node[+0x68].
local carry = probe.csv_open(probe.out_path("carry.csv"),
    "vsync,ctx7,seat,d9,da,dc,actor4,a21c,a21d,node74,node68,node10,x,z")
local last = {}
local prev_ctx7 = -1
local park_run = 0
local parked_logged = false

-- pad driver state
local held = nil
local hold_left = 0
local gap_left = 0
local round = 0
local prev_ctx6 = -1
local stall, stall_ctx6, fallback = 0, -1, false

local press_log = 0
local function press(btn)
    if held ~= nil then probe.pad_release(held) end
    if press_log < 400 then
        press_log = press_log + 1
        PCSX.log(string.format("[hazard] press %d at ctx6=0x%02X", btn, prev_ctx6))
    end
    probe.pad_force(btn)
    held = btn
    hold_left = 3
    gap_left = 10
end

local function drive(ctx6, seat)
    if held ~= nil then
        hold_left = hold_left - 1
        if hold_left <= 0 then probe.pad_release(held); held = nil end
        return
    end
    if gap_left > 0 then gap_left = gap_left - 1; return end
    if ctx6 ~= prev_ctx6 then
        if ctx6 == 0x1E then round = round + 1; fallback = false end
        prev_ctx6 = ctx6
    end
    if ctx6 == 0x28 then
        local plan = (round % 2 == 1) and PLAN or PLAN_ALT
        -- ctx[+0x13] is the member being commanded (battle-command-flow.md)
        local c = string.sub(plan, seat + 1, seat + 1)
        if c == "M" and not fallback then
            press(probe.BTN.RIGHT)
        elseif c == "S" then
            press(probe.BTN.DOWN)
        else
            press(probe.BTN.LEFT)
        end
    elseif ctx6 == 0x32 then
        press(probe.BTN.CIRCLE) -- never run
    elseif ctx6 ~= 0xFF and ctx6 ~= 0x00 then
        -- every other prompt (Begin, Auto, target, spell list and its
        -- sub-states, commit confirm) takes Cross
        press(probe.BTN.CROSS)
    end
end

local function actor(seat)
    local a = u32(ACTORS + seat * 4)
    if in_ram(a) then return a end
    return nil
end

local function event(v, kind, ctx7, acting, seat, a)
    local t = u8(a + 0x1DD)
    local ta = (t < 8) and actor(t) or nil
    events:row("%d,%s,0x%02X,%d,%d,0x%02X,%d,%d,%d,%d,%d,%d,%d", v, kind, ctx7,
        acting, seat, u8(a + 0x1DC), u8(a + 0x1D9), u8(a + 0x1DA), t,
        i16(a + 0x34), i16(a + 0x38),
        ta and i16(ta + 0x3C) or 0, ta and i16(ta + 0x40) or 0)
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_approach_hazard.lua",
            { sstate = SSTATE, frames = FRAMES, plan = PLAN, plan_alt = PLAN_ALT,
              pokes = probe.getenv("LEGAIA_POKES", "") })
        return {}
    end,
    on_capture = function(ctx, v)
        for _, p in ipairs(pokes) do
            if v == p.at then
                local a = u32(ACTORS + p.slot * 4)
                if in_ram(a) then
                    probe.write_u16(a + p.off, p.val)
                    PCSX.log(string.format("[hazard] poke seat %d +0x%X = 0x%X", p.slot, p.off, p.val))
                    events:row("%d,poke,0x00,0,%d,0x00,0,0,0,%d,%d,0,0", v, p.slot, p.off, p.val)
                end
            end
        end
        if parked_at ~= nil and v - parked_at >= QUIT_AFTER_PARK then
            ctx.request_quit = true
            return
        end
        local c = u32(CTX_PTR)
        if v % 600 == 0 then
            PCSX.log(string.format("[hazard] hb v=%d mode=0x%02X ctx6=0x%02X ctx7=0x%02X",
                v, u8(MODE_VA), in_ram(c) and u8(c + 6) or 0, in_ram(c) and u8(c + 7) or 0))
        end
        if u8(MODE_VA) ~= 0x15 then return end
        if not in_ram(c) then return end
        local ctx6, ctx7, acting = u8(c + 6), u8(c + 7), u8(c + 0x13)
        -- A sub-screen the masher cannot leave (an unaffordable spell, an
        -- empty list) backs out with Circle and falls back to Attack.
        if ctx6 == stall_ctx6 then
            stall = stall + 1
        else
            stall_ctx6 = ctx6; stall = 0
        end
        if stall > 300 and (ctx6 == 0x46 or ctx6 == 0x3C) then
            press(probe.BTN.CIRCLE)
            fallback = true
            stall = 0
        end
        drive(ctx6, acting)
        for seat = 0, 7 do
            local a = actor(seat)
            if a ~= nil then
                local d9, da, dc = u8(a + 0x1D9), u8(a + 0x1DA), u8(a + 0x1DC)
                local hp = u16(a + 0x14C)
                local sig = string.format("%d,%d,%d,%d", d9, da, dc, hp)
                local was = last[seat]
                if sig ~= (was and was.sig or "") then
                    anim:row("%d,0x%02X,0x%02X,%d,%d,%d,%d,0x%02X,%d", v, ctx6,
                        ctx7, acting, seat, d9, da, dc, hp)
                    local wbit = was and bit.band(was.dc, 4) or 0
                    if bit.band(dc, 4) ~= 0 and wbit == 0 then
                        event(v, "bit2_up", ctx7, acting, seat, a)
                    elseif bit.band(dc, 4) == 0 and wbit ~= 0 then
                        event(v, "bit2_down", ctx7, acting, seat, a)
                    end
                    last[seat] = { sig = sig, dc = dc }
                end
            end
        end
        if ctx7 >= 0x32 and ctx7 <= 0x38 then
            for seat = 3, 7 do
                local m = actor(seat)
                if m ~= nil and u16(m + 0x14C) ~= 0 then
                    local n = u32(m + 0x22C)
                    local n74, n68, n10 = 0, 0, 0
                    if in_ram(n) then
                        n74, n68, n10 = u32(n + 0x74), u16(n + 0x68), u32(n + 0x10)
                    end
                    carry:row("%d,0x%02X,%d,%d,%d,0x%02X,0x%08X,%d,%d,0x%08X,%d,0x%08X,%d,%d",
                        v, ctx7, seat, u8(m + 0x1D9), u8(m + 0x1DA), u8(m + 0x1DC),
                        u32(m + 4), u8(m + 0x21C), u8(m + 0x21D), n74, n68, n10,
                        i16(m + 0x34), i16(m + 0x38))
                end
            end
        end
        local a = actor(acting)
        if ctx7 == 0x19 and prev_ctx7 ~= 0x19 and a ~= nil then
            event(v, "approach", ctx7, acting, acting, a)
            park_run = 0
            parked_logged = false
        end
        if ctx7 == 0x19 and a ~= nil and u8(a + 0x1D9) == 0 and u8(a + 0x1DA) == 0 then
            park_run = park_run + 1
            if park_run == 120 and not parked_logged then
                event(v, "park", ctx7, acting, acting, a)
                parked_logged = true
                parked_at = v
                PCSX.log(string.format("[hazard] PARK at v=%d seat %d", v, acting))
                probe.sstate.save(probe.out_path("park.sstate"))
            end
        else
            park_run = 0
        end
        prev_ctx7 = ctx7
    end,
})
