-- autorun_enemy_item_ally_target.lua
--
-- Aim a single-target enemy item (Point Card 0xFE or Lippian Flute 0x98)
-- at a party seat through the real pad path and log what the action does
-- to its target. Backs docs/reference/glitches/items-and-equipment.md
-- ("Enemy items aimed at an ally").
--
-- The pad walk is the retail one: Begin -> command ring -> Item window ->
-- enemy picker (flow 0x5B) -> LEFT (steps onto the ring head) -> confirm
-- -> begin the round. What is POKED, once, at capture frame 5:
--   * bag slot 0 = LEGAIA_ITEM x3, Point Card bank = LEGAIA_POINTS
--   * the user's stored target actor[+0x1DD] = LEGAIA_TGT (the stale ally
--     a healing item would have left there)
--   * every party seat other than LEGAIA_USER gets HP 0 and its knockdown
--     clip staged, so it is lying dead by the time the walk starts and the
--     user is the only member owing a command
--   * ctx[+0x13] = LEGAIA_USER (the member the ring opens on)
-- With LEGAIA_KILL=0/1 the Lippian kill flag 0x801F7C4C is forced for the
-- life of the cast (the roll itself is 1 in 128).
--
-- Env:
--   LEGAIA_ITEM    item id, default 0xFE (254); 152 = Lippian Flute
--   LEGAIA_USER    acting party seat, default 0
--   LEGAIA_TGT     party seat to aim at, default = LEGAIA_USER
--   LEGAIA_POINTS  Point Card bank, default 50
--   LEGAIA_KILL    -1 leave the roll alone (default), 0 / 1 force it
--
-- Scenario: a battle state parked on Begin|Run (flow 0x1E) with three
-- seated members, e.g.
--   timeout 900 bash scripts/pcsx-redux/run_probe.sh --fast \
--     --lua scripts/pcsx-redux/autorun_enemy_item_ally_target.lua \
--     --scenario cort_evolved_battle_first_menu --frames 3000
-- Whether the dead-target redirect FUN_801DB124 runs depends on turn
-- order: it is skipped for the round's first actor (ctx[+0x274] at the
-- menu). In that scenario the first actor is seat 1.
--
-- Poll-only (no breakpoints) - safe to run --fast. Output: the emulator
-- log ([sm] / [seatN] / [pick] lines, printed on change) and a
-- screenshot every 50 frames in the output directory.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")
local pad   = require("probe.pad")

local TBL, CTXP = 0x801C9370, 0x8007BD24
local INV, BANK = 0x80085958, 0x800845B4
local LOADER_B, KILL_FLAG = 0x8007BC4C, 0x801F7C4C

local ITEM = probe.getenv_num("LEGAIA_ITEM", 0xFE)
local USER = probe.getenv_num("LEGAIA_USER", 0)
local TGT  = probe.getenv_num("LEGAIA_TGT", USER)
local PTS  = probe.getenv_num("LEGAIA_POINTS", 50)
local KF   = probe.getenv_num("LEGAIA_KILL", -1)
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 3000)
local START = 200   -- frames given to the poked seats to finish falling

local function actor(i) return mem.read_u32(TBL + i * 4) or 0 end

local function shot(stem)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not ok or ss == nil then return end
    local bpp = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
    local h = io.open(probe.out_path(stem .. ".raw"), "wb")
    if h then h:write(tostring(ss.data)); h:close() end
    local m = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if m then
        m:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            tonumber(ss.width), tonumber(ss.height), bpp))
        m:close()
    end
end

local last = {}
local function log_change(key, s, el)
    if last[key] ~= s then
        PCSX.log(string.format("[%s] f=%d %s", key, el, s))
        last[key] = s
    end
end

local stage, press_at, held, wait_until = 0, nil, nil, 0
local function press(b, el) pad.force(b); held = b; press_at = el end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = FRAMES,
    on_arm = function() return {} end,
    on_capture = function(ctx, el)
        local c = mem.read_u32(CTXP) or 0
        if c < 0x80000000 or c >= 0x80200000 then
            log_change("ctx", "battle context gone", el)
            return
        end
        local flow, st = mem.read_u8(c + 6) or 0, mem.read_u8(c + 7) or 0
        local act = mem.read_u8(c + 0x13) or 0

        if el == 5 then
            mem.write_u8(INV, ITEM); mem.write_u8(INV + 1, 3)
            mem.write_u32(BANK, PTS)
            for i = 0, 2 do
                local p = actor(i)
                if i ~= USER and p ~= 0 then
                    mem.write_u16(p + 0x14c, 0); mem.write_u16(p + 0x172, 0)
                    mem.write_u8(p + 0x1da, 4); mem.write_u8(p + 0x1dc, 1)
                end
            end
            mem.write_u8(actor(USER) + 0x1dd, TGT)
            mem.write_u8(c + 0x13, USER)
        end
        if held and el >= press_at + 4 then pad.release(held); held = nil end

        -- Force the Lippian kill roll once the cast module has rolled it.
        if KF >= 0 and (mem.read_u32(LOADER_B) or 0) == 0x1D then
            local ph = mem.read_u8(c + 0x279) or 0
            if ph >= 1 and ph < 11 then mem.write_u32(KILL_FLAG, KF) end
        end

        log_change("sm", string.format(
            "flow=%02x st=%02x acting=%d next=%d bank=%d bag0=%02x x%d cast=%x/%d",
            flow, st, act, mem.read_u8(c + 0x274) or 0,
            mem.read_u32(BANK) or 0, mem.read_u8(INV) or 0,
            mem.read_u8(INV + 1) or 0, mem.read_u32(LOADER_B) or 0,
            mem.read_u8(c + 0x279) or 0), el)
        for i = 0, 2 do
            local p = actor(i)
            if p ~= 0 then
                log_change("seat" .. i, string.format(
                    "hp=%d shown=%d acc=%d clip=%d staged=%d ev=%02x "
                    .. "tint=%d cap=%d tgt=%d cat=%d id=%02x sel=%02x",
                    mem.read_u16(p + 0x14c), mem.read_u16(p + 0x172),
                    mem.read_u32(p + 0x10), mem.read_u8(p + 0x1d9),
                    mem.read_u8(p + 0x1da), mem.read_u8(p + 0x1dc),
                    mem.read_u8(p + 0x21c), mem.read_u8(p + 0x225),
                    mem.read_u8(p + 0x1dd), mem.read_u8(p + 0x1de),
                    mem.read_u8(p + 0x1df), mem.read_u8(p + 0x1e8)), el)
            end
        end
        if el % 50 == 0 then shot(string.format("frame_%05d", el)) end
        if el >= FRAMES - 2 then ctx.request_quit = true end
        if el < START or held or el < wait_until then return end

        local B = pad.BTN
        if stage == 0 then
            if flow == 0x1E then press(B.LEFT, el); wait_until = el + 40
            elseif flow == 0x28 then press(B.UP, el); wait_until = el + 50
            elseif flow == 0x3C then press(B.CROSS, el); wait_until = el + 50
            elseif flow == 0x5B then stage = 1; wait_until = el + 40 end
        elseif stage == 1 then
            PCSX.log(string.format(
                "[pick] f=%d before LEFT tgt=%d ring count=%d seats=%d,%d,%d,%d",
                el, mem.read_u8(actor(act) + 0x1dd), mem.read_u8(c + 0x244),
                mem.read_u8(c + 0x245), mem.read_u8(c + 0x246),
                mem.read_u8(c + 0x247), mem.read_u8(c + 0x248)))
            press(B.LEFT, el); stage = 2; wait_until = el + 40
        elseif stage == 2 then
            PCSX.log(string.format("[pick] f=%d after LEFT tgt=%d",
                el, mem.read_u8(actor(act) + 0x1dd)))
            shot("picker_on_ally")
            press(B.CROSS, el); stage = 3; wait_until = el + 50
        elseif stage == 3 then
            if flow == 0x6E then press(B.CROSS, el); wait_until = el + 50
            elseif flow == 0xFE or flow == 0xFF then stage = 4 end
        end
    end,
})
