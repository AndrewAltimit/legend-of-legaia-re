-- autorun_locomotion_clip_pin.lua
--
-- Pin the PROT 0874 §1 locomotion-bank clip roles empirically: resume the
-- s3_rimelm_freeroam anchor, sample the player actor's live anim-record
-- pointer (`*(0x8007C364) + 0x4C`) every field tick while driving pad
-- input phases (idle -> hold DOWN walk -> idle), and log every record-
-- pointer CHANGE with the input phase. The record pointer resolves to a
-- bank index via the container base (the 23-record offset table is
-- scanned backwards from the live pointer, same walk the RAM census
-- used); this probe just logs raw pointers - the host maps them to
-- record indices against the disc container offsets.
--
--   LEGAIA_SSTATE=saves/library/pcsx-redux/2fba9adf...sstate \
--   LEGAIA_OUT_DIR=/tmp/clippin \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_locomotion_clip_pin.lua \
--       timeout --kill-after=30s 600s bash scripts/pcsx-redux/run_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local PLAYER_PTR = 0x8007C364
local FIELD_BP   = 0x8001698C

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/clippin")
local START_DELAY = tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/clippin.log", "w")
local function log(s)
    PCSX.log("[clippin] " .. s)
    if LOG then LOG:write(s .. "\n"); LOG:flush() end
end
local CSV = io.open(OUT_DIR .. "/records.csv", "w")
CSV:write("tick,phase,anm_ptr,frame_ctr\n")

-- Input phases: settle, then each (label, button, frames). UP after DOWN
-- exercises a turn; CROSS-free so no dialogue arms.
local PHASES = {
    { "idle1", nil,          150 },
    { "walkD", pad.BTN.DOWN, 240 },
    { "idle2", nil,          150 },
    { "walkU", pad.BTN.UP,   240 },
    { "idle3", nil,          120 },
}

local g_tick = 0
local phase_i = 1
local phase_left = PHASES[1][3]
local cur_btn = nil
local last_ptr = -1
local done = false

local function field_tick()
    if done then return end
    g_tick = g_tick + 1
    local pi = PHASES[phase_i]
    if pi == nil then
        if cur_btn then pad.release(cur_btn) end
        log("phases complete; quitting")
        if LOG then LOG:close() end
        CSV:close()
        done = true
        PCSX.quit(0)
        return
    end
    -- phase input
    local want = pi[2]
    if want ~= cur_btn then
        if cur_btn then pad.release(cur_btn) end
        if want then pad.force(want) end
        cur_btn = want
    end
    -- sample the anim record pointer
    local base = mem.in_ram(PLAYER_PTR) and mem.read_u32(PLAYER_PTR) or nil
    if base and mem.in_ram(base + 0x4C) then
        local ptr = mem.read_u32(base + 0x4C) or 0
        local fc = mem.read_u16(base + 0x68) or 0
        if ptr ~= last_ptr then
            last_ptr = ptr
            CSV:write(string.format("%d,%s,0x%08X,0x%04X\n", g_tick, pi[1], ptr, fc))
            CSV:flush()
            log(string.format("[tick %d] %s: anm_ptr -> 0x%08X", g_tick, pi[1], ptr))
        end
    end
    phase_left = phase_left - 1
    if phase_left <= 0 then
        phase_i = phase_i + 1
        phase_left = PHASES[phase_i] and PHASES[phase_i][3] or 0
    end
end

local vsync = 0
local start_loaded = false
local function on_vsync()
    vsync = vsync + 1
    if not start_loaded and START_SAVE ~= "" and vsync >= START_DELAY then
        start_loaded = true
        if sstate.load(START_SAVE) then
            log("resumed from " .. START_SAVE)
        else
            log("FAILED to load " .. START_SAVE)
        end
    end
end

pcall(function() bp.arm(FIELD_BP, "Exec", 4, "field_tick", field_tick) end)
log("armed field tick; phases: idle/walkDOWN/idle/walkUP/idle")
PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
