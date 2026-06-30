-- autorun_s3_endtest.lua
--
-- Observe what selecting "End" on the name-entry screen does, so the Yes/No
-- confirm can be driven correctly. Resumes the town01 stall (cursor already on
-- End, idx 116), presses one clean CROSS at frame AT, then logs the name-entry +
-- dialog/picker + completion state every few frames so the Yes/No prompt shape
-- (and which input confirms "Yes") is visible.
--
-- Env: LEGAIA_SSTATE, LEGAIA_OUT_DIR, LEGAIA_AT (press frame), LEGAIA_BTN.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_BP = 0x8001698C
local CURSOR   = 0x8007BB88
local SCENE_PTR= 0x801C6EA4
local PLAYER   = 0x8007C364
local SR_STATE = 0x8007B450

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/s3_end")
local AT         = tonumber(env.getenv("LEGAIA_AT", "760")) or 760
local BTN        = env.getenv("LEGAIA_BTN", "CROSS")
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/end.log", "w")
local function log(s) PCSX.log("[end] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or nil end
local function ru16(a) return mem.in_ram(a) and ((ru8(a) or 0)+0x100*(ru8(a+1) or 0)) or nil end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or nil end

local vsync, loaded = 0, false
PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync = vsync + 1
    if not loaded and START_SAVE ~= "" and vsync >= START_DELAY then
        loaded = true
        log(sstate.load(START_SAVE) and ("resumed "..START_SAVE) or ("FAILED "..START_SAVE))
    end
end)

local function snap(tag)
    local sp = ru32(SCENE_PTR); local pp = ru32(PLAYER)
    local fl = pp and ru32(pp + 0x10) or nil
    log(string.format("%s cursor=%s scene3E=%s sr=%08X eng=%s dlg62=%s pick0c=%s scene40=%s",
        tag,
        ru16(CURSOR) ~= nil and string.format("0x%04X", ru16(CURSOR)) or "nil",
        sp and string.format("0x%04X", ru16(sp+0x3E) or 0) or "nil",
        ru32(SR_STATE) or 0,
        fl and tostring(math.floor(fl/0x80000)%2==1) or "nil",
        sp and string.format("0x%02X", ru8(sp+0x62) or 0) or "nil",
        sp and string.format("0x%02X", ru8(sp+0x0C) or 0) or "nil",
        sp and string.format("0x%04X", ru16(sp+0x40) or 0) or "nil"))
end

local frame, pressed_until = 0, 0
bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    frame = frame + 1
    if frame == AT then
        snap(string.format("[f%d PRE-press %s]", frame, BTN))
        pad.force(pad.BTN[BTN]); pressed_until = frame + 4
    end
    if pressed_until > 0 and frame >= pressed_until then
        pad.release(pad.BTN[BTN]); pressed_until = 0
    end
    if frame >= AT and frame < AT + 360 and ((frame - AT) % 10) == 0 then
        snap(string.format("[f%d +%d]", frame, frame - AT))
    end
    if frame >= AT + 360 then
        if LOG then LOG:close() end
        PCSX.quit(0)
    end
end)

log("s3 end-test armed (btn=" .. BTN .. ")")
