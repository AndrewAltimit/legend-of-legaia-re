-- autorun_gaza2_summon_displacement.lua
--
-- Measure who moves during a battle action - built to answer "does a summon
-- (Kemaro) displace actors, and does the displacement get COMMITTED as the
-- home position?" after the 0x19 park was root-caused to drifted geometry
-- (target beyond a walk-less boss's reach).
--
-- Poll-only (no breakpoints), so it runs at dynarec speed while a human
-- plays: load your fight savestate in the GUI, cast the spell/summon once,
-- let the action finish, close the emulator. Offline, the CSV shows every
-- actor's current position (+0x34/+0x38) and home position (+0x3C/+0x40)
-- per change - a summon that fails to restore shows up as a home delta that
-- survives past the action's end.
--
-- Output (captures/gaza2_summon_displacement/<ts>/positions.csv):
--   vsync,ctx7,acting, per seat 0..3: cx,cz,hx,hz, then the enemy's anim
--   bytes a3_1da (staged clip index) / a3_1d9 (playing clip index) - the
--   pair that separates a working approach (Move clip engaged) from the
--   parked drive-idle shape (0/0). Rows are change-triggered + heartbeat.
--
-- Directed experiment (the summon-staging-aftermath hypothesis): cast a
-- summon, then STALL (defend / items, don't kill the boss) so his next
-- action is a melee soon after the staging round-trip. Repeat. Healthy
-- approaches show the anim pair leave 0/0 while he slides in state 0x19;
-- a reproduction shows the pair stuck at 0/0 with the position frozen.
--
-- Launch (dynarec ON, YOUR config + savestates so slot hotkeys work):
--   LEGAIA_NO_SSTATE=1 bash scripts/pcsx-redux/run_probe.sh \
--     --fast --no-isolate-config \
--     --lua scripts/pcsx-redux/autorun_gaza2_summon_displacement.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local HEARTBEAT = probe.getenv_num("LEGAIA_HEARTBEAT", 64)

local MODE_VA = 0x8007B83C
local CTX_PTR = 0x8007BD24
local ACTORS  = 0x801C9370
local SEATS   = { 0, 1, 2, 3 }

local function u8(a) return probe.read_u8(a) or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function i16(a) local v = u16(a); return v >= 0x8000 and v - 0x10000 or v end
local function in_ram(a) return a >= 0x80000000 and a < 0x80200000 end

local header = { "vsync", "ctx7", "acting" }
for _, s in ipairs(SEATS) do
    header[#header + 1] = "cx" .. s
    header[#header + 1] = "cz" .. s
    header[#header + 1] = "hx" .. s
    header[#header + 1] = "hz" .. s
end
header[#header + 1] = "a3_1da"
header[#header + 1] = "a3_1d9"
local csv = probe.csv_open(probe.out_path("positions.csv"), table.concat(header, ","))

local vsync = 0
local last_sig = ""

local function on_vsync()
    vsync = vsync + 1
    if u8(MODE_VA) ~= 0x15 then return end
    local c = u32(CTX_PTR)
    if not in_ram(c) then return end

    local row = { vsync, string.format("0x%02X", u8(c + 7)), u8(c + 0x13) }
    for _, s in ipairs(SEATS) do
        local a = u32(ACTORS + s * 4)
        if in_ram(a) then
            row[#row + 1] = i16(a + 0x34)
            row[#row + 1] = i16(a + 0x38)
            row[#row + 1] = i16(a + 0x3C)
            row[#row + 1] = i16(a + 0x40)
        else
            row[#row + 1] = -1; row[#row + 1] = -1
            row[#row + 1] = -1; row[#row + 1] = -1
        end
    end
    local a3 = u32(ACTORS + 3 * 4)
    if in_ram(a3) then
        row[#row + 1] = u8(a3 + 0x1DA)
        row[#row + 1] = u8(a3 + 0x1D9)
    else
        row[#row + 1] = -1; row[#row + 1] = -1
    end

    local sig = table.concat(row, ",", 2, #row)
    if sig ~= last_sig or vsync % HEARTBEAT == 0 then
        last_sig = sig
        csv:row("%s", table.concat(row, ","))
    end
end

-- keep the handle: a GC'd listener object deletes the C++ listener
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
PCSX.log("[displace] position recorder armed (poll-only, dynarec-safe)")
PCSX.log("[displace] load your fight state, cast the summon, let the action end, then close the emulator")
