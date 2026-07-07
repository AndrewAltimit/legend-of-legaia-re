-- autorun_s4_padmap.lua
--
-- Measure the pad-direction -> world-displacement mapping for the field
-- locomotion controller (camera-relative, but fixed per town area), so a
-- deterministic navigator can move in consistent world directions. From the
-- s3_rimelm_freeroam anchor it holds each of UP/RIGHT/DOWN/LEFT for a window
-- and records the net change in the player position (player+0x14 = X,
-- player+0x18 = Z, read signed), settling between directions.
--
-- Env: LEGAIA_SSTATE, LEGAIA_OUT_DIR, LEGAIA_HOLD, LEGAIA_GAP.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_BP = 0x8001698C
local PLAYER   = 0x8007C364

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/s4_padmap")
local HOLD       = tonumber(env.getenv("LEGAIA_HOLD", "70")) or 70
local GAP        = tonumber(env.getenv("LEGAIA_GAP", "30")) or 30
local SETTLE0    = tonumber(env.getenv("LEGAIA_SETTLE0", "40")) or 40
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/padmap.log", "w")
local function log(s) PCSX.log("[pad] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or nil end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or nil end
local function s32(v)  if v == nil then return nil end; if v >= 0x80000000 then return v - 0x100000000 end; return v end
local function ppos()
    local pp = ru32(PLAYER); if pp == nil then return nil end
    return s32(ru32(pp+0x14)), s32(ru32(pp+0x18))
end

local vsync, loaded = 0, false
-- keep the handle: a GC'd listener object deletes the C++ listener
-- (silently unregisters; GC mid-dispatch can segfault the emulator)
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] = PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync = vsync + 1
    if not loaded and START_SAVE ~= "" and vsync >= START_DELAY then
        loaded = true
        log(sstate.load(START_SAVE) and ("resumed "..START_SAVE) or ("FAILED "..START_SAVE))
    end
end)

local DIRS = { "UP", "RIGHT", "DOWN", "LEFT" }
local frame = 0
local stage = 0      -- 0 = initial settle; then per-dir: 1..#DIRS
local stage_start = nil
local held = nil
local x0, z0 = nil, nil

bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    frame = frame + 1
    if frame <= SETTLE0 then return end
    if stage_start == nil then stage_start = frame; stage = 1; log("start measuring") end

    local cyc = HOLD + GAP
    local elapsed = frame - stage_start
    local idx = math.floor(elapsed / cyc) + 1
    if idx > #DIRS then
        if held then pad.release(pad.BTN[held]) end
        log("=== padmap done ===")
        if LOG then LOG:close() end; PCSX.quit(0)
    end
    local phase = elapsed % cyc
    local dir = DIRS[idx]

    if phase == 0 then
        -- begin hold: snapshot start position, press dir
        x0, z0 = ppos()
        if held then pad.release(pad.BTN[held]) end
        pad.force(pad.BTN[dir]); held = dir
    elseif phase == HOLD then
        -- end hold: release + report displacement
        pad.release(pad.BTN[dir]); held = nil
        local x1, z1 = ppos()
        if x0 and x1 then
            log(string.format("%-6s dX=%d dZ=%d  (%d,%d -> %d,%d)",
                dir, x1 - x0, z1 - z0, x0, z0, x1, z1))
        end
    end
end)

log("s4 padmap armed")
