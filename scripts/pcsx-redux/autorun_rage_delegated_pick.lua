-- autorun_rage_delegated_pick.lua
--
-- Who writes a Rage (Evil Medallion) member's action, and what does its turn
-- play? Run against a battle state parked at the round's Begin / Run prompt
-- (the `battle_gaza2_prompt` scenario is the PCSX-Redux one in the catalogue).
--
-- The probe POKES the Rage passive into the character records before the
-- round starts: record `+0xF8 |= 0x2000` (accessory passive 0x2D) for every
-- party seat listed in LEGAIA_RAGE_SEATS, the record found through the seat's
-- roster id `DAT_8007BD10[seat]` (`0x80084708 + (id - 1) * 0x414`). That is
-- the bit FUN_80047430's Rage arm (0x80047F10..0x80047F98) promotes into the
-- actor's `+0x16E |= 0x380` and the bit FUN_801F0450's delegated arm gates on
-- (0x801F04D4). It is an intervention on the save, not a played equip: the
-- capture grade is "poked".
--
-- Then it drives the prompt with LEGAIA_PAD_SCRIPT and records:
--   writes.csv     every write to a party actor's +0x1DD / +0x1DE /
--                  +0x1DF..+0x1E6 with pc / ra (the writer attribution)
--   calls.csv      entries of FUN_801F0450 (round state 0x00), FUN_801EED1C
--                  (per-action builder, a0 = seat) and the 0x0C seed's
--                  `jal 0x801EED1C` site
--   timeline.csv   change-triggered: ctx+6 (flow), ctx+7 (action SM),
--                  ctx+0x274 (acting seat), each party seat's +0x16E,
--                  +0x1DE, +0x1DD and stream, plus the latched clip +0x1DB
--
-- Knobs (env):
--   LEGAIA_RAGE_SEATS   comma list of party seats to poke (default "0"; "" = none)
--   LEGAIA_STATUS_SEATS comma list of seats whose actor +0x16E takes 0x380
--   LEGAIA_PAD_SCRIPT   ";"-separated "vsync:BUTTON[:hold]" (probe.pad names)
--   LEGAIA_FRAMES       capture length in vsyncs
--
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_rage_delegated_pick.lua \
--     --scenario battle_gaza2_prompt --frames 2400

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local watch = require("probe.watch")
local pad   = require("probe.pad")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 2400)
local SCRIPT = probe.getenv("LEGAIA_PAD_SCRIPT", "")
local SEATS  = probe.getenv("LEGAIA_RAGE_SEATS", "0")
-- LEGAIA_STATUS_SEATS: party seats whose ACTOR +0x16E takes `|= 0x380` once,
-- with no record bit - the status-confused shape (the charm roll at
-- 0x8004D118 and the slot-B images' `ori 0x380` write the actor word only).
local STATUS_SEATS = probe.getenv("LEGAIA_STATUS_SEATS", "")

local CTX_PTR   = 0x8007BD24
local ACTORS    = 0x801C9370
local ROSTER    = 0x8007BD10
local RECORDS   = 0x80084708
local REC_SIZE  = 0x414
local RAGE_BIT  = 0x2000

local function u8(a)  return probe.read_u8(a)  or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function in_ram(a) return a >= 0x80000000 and a < 0x80200000 end

local rage_seats = {}
for s in SEATS:gmatch("%d+") do rage_seats[#rage_seats + 1] = tonumber(s) end
local status_seats = {}
for s in STATUS_SEATS:gmatch("%d+") do status_seats[#status_seats + 1] = tonumber(s) end

local pad_events = {}
for item in SCRIPT:gmatch("[^;]+") do
    local at, name, hold = item:match("^%s*(%d+):(%a+):?(%d*)%s*$")
    local btn = name and pad.BTN[name:upper()]
    if btn then
        pad_events[#pad_events + 1] = { at = tonumber(at), btn = btn,
            hold = tonumber(hold) or 4, name = name:upper() }
    end
end
table.sort(pad_events, function(a, b) return a.at < b.at end)

local g_elapsed = 0
local wcsv = probe.csv_open(probe.out_path("writes.csv"), "tick,label,addr,pc,ra,value")
local w = watch.new{
    csv = wcsv, detail_path = probe.out_path("writes.detail.txt"),
    max_detail = 24, elapsed = function() return g_elapsed end,
}
local ccsv = probe.csv_open(probe.out_path("calls.csv"), "tick,site,pc,ra,a0,ctx7,seat274")
local tl = probe.csv_open(probe.out_path("timeline.csv"),
    "vsync,ctx6,ctx7,seat274,s0_16e,s0_1de,s0_1dd,s0_1db,s0_stream," ..
    "s1_16e,s1_1de,s1_1dd,s1_1db,s1_stream,s2_16e,s2_1de,s2_1dd,s2_1db,s2_stream")

local function actor_of(seat)
    local a = u32(ACTORS + seat * 4)
    return in_ram(a) and a or 0
end

local function stream_hex(a)
    local t = {}
    for i = 0, 15 do t[#t + 1] = string.format("%02x", u8(a + 0x1DF + i)) end
    return table.concat(t)
end

local poked, watched, last_key = false, false, ""
local pad_idx, pad_release_at, pad_held = 1, nil, nil

local function poke_records()
    for _, seat in ipairs(rage_seats) do
        local id = u8(ROSTER + seat)
        if id >= 1 and id <= 4 then
            local va = RECORDS + (id - 1) * REC_SIZE + 0xF8
            local before = u32(va)
            probe.write_u32(va, bit.bor(before, RAGE_BIT))
            PCSX.log(string.format("[rage] seat %d roster %d record+0xF8 %08X -> %08X",
                seat, id, before, u32(va)))
        end
    end
end

local function poke_status()
    for _, seat in ipairs(status_seats) do
        local a = actor_of(seat)
        if a ~= 0 then
            local before = u16(a + 0x16E)
            probe.write_u16(a + 0x16E, bit.bor(before, 0x380))
            PCSX.log(string.format("[rage] seat %d actor+0x16E %04X -> %04X",
                seat, before, u16(a + 0x16E)))
        end
    end
end

local function arm_watches()
    for seat = 0, 2 do
        local a = actor_of(seat)
        if a ~= 0 then
            w:arm(a + 0x1DD, 1, string.format("s%d_1dd", seat))
            w:arm(a + 0x1DE, 1, string.format("s%d_1de", seat))
            for i = 0, 7 do
                w:arm(a + 0x1DF + i, 1, string.format("s%d_1df+%d", seat, i))
            end
        end
    end
end

local function call_bp(addr, site)
    probe.arm_breakpoint(addr, "Exec", 4, site, function()
        local r = PCSX.getRegisters()
        local c = u32(CTX_PTR)
        ccsv:row("%d,%s,0x%08X,0x%08X,0x%08X,0x%02X,%d", g_elapsed, site,
            tonumber(r.pc), tonumber(r.GPR.n.ra), tonumber(r.GPR.n.a0),
            in_ram(c) and u8(c + 7) or 0, in_ram(c) and u8(c + 0x274) or 0)
    end)
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        call_bp(0x801F0450, "FUN_801F0450")
        call_bp(0x801EED1C, "FUN_801EED1C")
        call_bp(0x801E2C7C, "seed_0x0C_jal_builder")
        call_bp(0x801E2CA0, "seed_0x0C_jal_7320")
        return {}
    end,
    on_capture = function(_, elapsed)
        g_elapsed = elapsed
        if not poked and elapsed >= 2 then
            poke_records(); poke_status(); poked = true
        end
        if not watched and elapsed >= 2 then
            arm_watches(); watched = true
        end
        if pad_held and elapsed >= pad_release_at then
            pad.release(pad_held); pad_held = nil
        end
        local ev = pad_events[pad_idx]
        if ev and elapsed >= ev.at and not pad_held then
            pad.force(ev.btn); pad_held = ev.btn
            pad_release_at = elapsed + ev.hold
            pad_idx = pad_idx + 1
            PCSX.log(string.format("[rage] vsync %d press %s", elapsed, ev.name))
        end
        local c = u32(CTX_PTR)
        if not in_ram(c) then return end
        local cols = { string.format("0x%02X,0x%02X,%d", u8(c + 6), u8(c + 7), u8(c + 0x274)) }
        for seat = 0, 2 do
            local a = actor_of(seat)
            if a ~= 0 then
                cols[#cols + 1] = string.format("0x%04X,%d,%d,0x%02X,%s",
                    u16(a + 0x16E), u8(a + 0x1DE), u8(a + 0x1DD), u8(a + 0x1DB), stream_hex(a))
            else
                cols[#cols + 1] = ",,,,"
            end
        end
        local key = table.concat(cols, ",")
        if key ~= last_key then
            tl:row("%d,%s", elapsed, key)
            last_key = key
        end
    end,
    on_done = function()
        PCSX.log(string.format("[rage] writes logged: %d", w:total()))
        wcsv:close(); ccsv:close(); tl:close()
    end,
})
