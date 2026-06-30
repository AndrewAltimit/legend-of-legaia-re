-- autorun_s3_substate.lua
--
-- Identify the parked STATE_RESUME sub-state at the town01-opening stall.
-- The opening's effect-actor handler FUN_801F159C runs PTR_FUN_801F33B4[actor+0x50]
-- each frame and completes (writes _DAT_8007B450 = 1) only when the scene-control
-- struct field _DAT_801C6EA4 +0x3E reaches 0. This probe breakpoints FUN_801F159C
-- (a0 = the effect-actor) in a window at the stall and histograms actor+0x50 (the
-- inner sub-state), so the parked sub-handler is identified; it also samples
-- scene+0x3E and _DAT_8007B450 for context.
--
-- Env: LEGAIA_SSTATE (resume), LEGAIA_OUT_DIR, LEGAIA_WIN_LO/HI.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local SR_HANDLER = 0x801F159C -- STATE_RESUME effect-actor per-frame handler (a0 = actor)
local FIELD_BP   = 0x8001698C -- per-frame field tick (frame clock)
local SCENE_PTR  = 0x801C6EA4 -- -> scene/field-control struct
local SR_STATE   = 0x8007B450 -- STATE_RESUME outer state (Idle/Armed/Done)

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/s3_sub")
local WIN_LO     = tonumber(env.getenv("LEGAIA_WIN_LO", "560")) or 560
local WIN_HI     = tonumber(env.getenv("LEGAIA_WIN_HI", "1300")) or 1300
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/sub.log", "w")
local function log(s) PCSX.log("[s3sub] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or nil end
local function ru16(a) return mem.in_ram(a) and (ru8(a) + 0x100*(ru8(a+1) or 0)) or nil end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or nil end

local vsync, loaded = 0, false
PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync = vsync + 1
    if not loaded and START_SAVE ~= "" and vsync >= START_DELAY then
        loaded = true
        log(sstate.load(START_SAVE) and ("resumed "..START_SAVE) or ("FAILED "..START_SAVE))
    end
end)

local frame = 0
local hist = {}   -- substate -> count
local recording = false

bp.arm(SR_HANDLER, "Exec", 4, "sr_handler", function()
    if not recording then return end
    local r = PCSX.getRegisters()
    local a0 = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
    if a0 < 0 then a0 = a0 + 0x100000000 end
    local sub = mem.in_ram(a0 + 0x50) and ru16(a0 + 0x50) or nil
    if sub ~= nil then hist[sub] = (hist[sub] or 0) + 1 end
end)

bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    frame = frame + 1
    recording = (frame >= WIN_LO and frame <= WIN_HI)
    if (frame % 120) == 0 and recording then
        local sp = ru32(SCENE_PTR)
        local scene3e = sp and ru16(sp + 0x3E) or nil
        log(string.format("frame %d: scene+0x3E=%s _DAT_8007B450=%s",
            frame, scene3e ~= nil and string.format("0x%04X", scene3e) or "nil",
            string.format("0x%08X", ru32(SR_STATE) or 0)))
    end
    if frame == WIN_HI + 1 then
        local arr = {}
        for sub, n in pairs(hist) do arr[#arr+1] = { sub = sub, n = n } end
        table.sort(arr, function(a, b) return a.n > b.n end)
        log(string.format("=== STATE_RESUME sub-state (actor+0x50) histogram, frames %d..%d ===", WIN_LO, WIN_HI))
        for _, e in ipairs(arr) do
            log(string.format("  sub=0x%02X  n=%d", e.sub, e.n))
        end
        if LOG then LOG:close() end
        PCSX.quit(0)
    end
end)

log("s3-substate armed; resume + histogram FUN_801F159C actor+0x50 at the stall")
