-- autorun_locomotion_run_pin.lua
--
-- Sibling of autorun_locomotion_clip_pin.lua aimed at the still-unpinned
-- RUN clip: hold a walk direction far longer than the original probe's
-- 240 frames (retail's dash state is a counter at player+0x5C whose
-- non-zero value switches the walk-animation select - see
-- docs/subsystems/field-locomotion.md), and log BOTH the anim-record
-- pointer (+0x4C) and the dash counter (+0x5C) - every pointer change,
-- plus a periodic sample - so the capture can answer "does a longer hold
-- ever leave the walk record, and does +0x5C ever rise?". Also tries a
-- direction with SQUARE / CIRCLE held (candidate dash chords).
--
--   LEGAIA_SSTATE=saves/library/pcsx-redux/<s3_rimelm_freeroam>.sstate \
--   LEGAIA_OUT_DIR=/tmp/runpin \
--   LEGAIA_LUA=<this file> \
--       timeout --kill-after=30s 900s bash scripts/pcsx-redux/run_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local PLAYER_PTR = 0x8007C364
local FIELD_BP   = 0x8001698C

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/runpin")
local START_DELAY = tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/runpin.log", "w")
local function log(s)
    PCSX.log("[runpin] " .. s)
    if LOG then LOG:write(s .. "\n"); LOG:flush() end
end
local CSV = io.open(OUT_DIR .. "/records.csv", "w")
CSV:write("tick,phase,anm_ptr,dash_5c,mult_72,frame_ctr,kind\n")

-- Long holds + chord candidates. CROSS-free so no dialogue arms.
local PHASES = {
    { "idle1",   nil,                              120 },
    { "holdD",   { pad.BTN.DOWN },                 900 },
    { "idle2",   nil,                              120 },
    { "holdDsq", { pad.BTN.DOWN, pad.BTN.SQUARE }, 420 },
    { "idle3",   nil,                              90  },
    { "holdDci", { pad.BTN.DOWN, pad.BTN.CIRCLE }, 420 },
    { "idle4",   nil,                              120 },
}

local g_tick = 0
local phase_i = 1
local phase_left = PHASES[1][3]
local cur = nil
local last_ptr = -1
local done = false

local function press_set(set)
    if cur then
        for _, b in ipairs(cur) do pad.release(b) end
    end
    if set then
        for _, b in ipairs(set) do pad.force(b) end
    end
    cur = set
end

local function field_tick()
    if done then return end
    g_tick = g_tick + 1
    local pi = PHASES[phase_i]
    if pi == nil then
        press_set(nil)
        log("phases complete; quitting")
        if LOG then LOG:close() end
        CSV:close()
        done = true
        PCSX.quit(0)
        return
    end
    if pi[2] ~= cur then press_set(pi[2]) end
    local base = mem.in_ram(PLAYER_PTR) and mem.read_u32(PLAYER_PTR) or nil
    if base and mem.in_ram(base + 0x4C) then
        local ptr  = mem.read_u32(base + 0x4C) or 0
        local dash = mem.read_u16(base + 0x5C) or 0
        local mult = mem.read_u16(base + 0x72) or 0
        local fc   = mem.read_u16(base + 0x68) or 0
        local changed = ptr ~= last_ptr
        if changed or (g_tick % 60 == 0) then
            CSV:write(string.format("%d,%s,0x%08X,%d,%d,0x%04X,%s\n",
                g_tick, pi[1], ptr, dash, mult, fc,
                changed and "change" or "sample"))
            CSV:flush()
            if changed then
                log(string.format("[tick %d] %s: anm_ptr -> 0x%08X (dash %d)",
                    g_tick, pi[1], ptr, dash))
            end
            last_ptr = ptr
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
log("armed field tick; phases: idle/holdDOWNx900/idle/DOWN+SQ/idle/DOWN+CI/idle")
-- keep the handle: a GC'd listener object deletes the C++ listener
-- (silently unregisters; GC mid-dispatch can segfault the emulator)
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] = PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
