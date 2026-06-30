-- autorun_s3_btnmask.lua
--
-- Read the name-entry button-config masks + verify pad injection reaches the
-- new-press state. The interactive name-entry handler (0x801F0480) selects/
-- confirms when `_DAT_8007B874 & *(0x800846D0)` is nonzero (and cancels/special
-- on `*(0x800846D4)` / `0x800`). This logs those masks and watches `_DAT_8007B874`
-- before/after a forced CROSS press, so the correct confirm button is known.
--
-- Env: LEGAIA_SSTATE, LEGAIA_OUT_DIR, LEGAIA_AT, LEGAIA_BTN.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_BP = 0x8001698C
local B874     = 0x8007B874 -- new-press button state the name handler ANDs
local MASK_SEL = 0x800846D0 -- select/confirm mask (s2+0x590)
local MASK_CAN = 0x800846D4 -- cancel/special mask (s2+0x594)

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/s3_mask")
local AT         = tonumber(env.getenv("LEGAIA_AT", "760")) or 760
local BTN        = env.getenv("LEGAIA_BTN", "CROSS")
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/mask.log", "w")
local function log(s) PCSX.log("[mask] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function ru16(a) return mem.in_ram(a) and ((mem.read_u8(a) or 0)+0x100*(mem.read_u8(a+1) or 0)) or nil end
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
    log(string.format("%s B874=%s mask_sel(46D0)=%s mask_can(46D4)=%s",
        tag,
        ru16(B874) ~= nil and string.format("0x%04X", ru16(B874)) or "nil",
        ru16(MASK_SEL) ~= nil and string.format("0x%04X", ru16(MASK_SEL)) or "nil",
        ru16(MASK_CAN) ~= nil and string.format("0x%04X", ru16(MASK_CAN)) or "nil"))
end

local frame, held = 0, 0
bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    frame = frame + 1
    if frame == AT - 1 then snap(string.format("[f%d no-input]", frame)) end
    if frame == AT then pad.force(pad.BTN[BTN]); held = frame + 3 end
    if frame >= AT and frame <= AT + 8 then snap(string.format("[f%d +%s held]", frame, BTN)) end
    if held > 0 and frame >= held then pad.release(pad.BTN[BTN]); held = 0 end
    if frame >= AT + 12 then
        if LOG then LOG:close() end
        PCSX.quit(0)
    end
end)

log("s3 btnmask armed (btn=" .. BTN .. ")")
