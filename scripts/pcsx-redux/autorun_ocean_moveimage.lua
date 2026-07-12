-- autorun_ocean_moveimage.lua
--
-- Trace every libgpu image-transfer wrapper call while the overworld idles
-- (the capture that confirmed the kingdom-slot-5 CLUT-walk cadence:
-- constant ceil(hold/dt)*dt intervals, reset-to-zero accumulators, shared
-- spawn epoch; see docs/subsystems/world-map.md "Ocean animation"):
--   MoveImage  FUN_80058490  (RECT *src, int dst_x, int dst_y)
--   LoadImage  FUN_800583C8  (RECT *dst, u_long *data)
--   StoreImage FUN_8005842C  (RECT *src, u_long *data)
-- The head-walk / ring / row-508 mirror / row-509 / park-fade writers all
-- bottom out in these three (docs/subsystems/world-map.md), so the per-call
-- log (vsync tick + rect + dst) IS the cadence + phase evidence.
--
-- Requires interpreter+debugger mode (Lua exec BPs): do NOT pass --fast.
--
-- Also logs the adaptive frame-step byte dt = scratchpad 0x1F800393 once
-- per vsync into ticks.csv, and saves a PCSX save state at a few capture
-- ticks so head CLUT content can be ground-truthed offline via
-- extract_vram_from_sstate.py.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem = require("probe.mem")
local sstate = require("probe.sstate")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 450)
local OUT_DIR = probe.getenv("LEGAIA_OUT_DIR", "/tmp/ocean-cadence")
local HOLD_NAME = probe.getenv("LEGAIA_HOLD", "")
local HOLD_FR = probe.getenv_num("LEGAIA_HOLD_FRAMES", 0)
local pad = require("probe.pad")
local HOLD_BTN = (HOLD_NAME ~= "" and pad.BTN[HOLD_NAME]) or nil

-- Save-state snapshot ticks (comma-separated vsync indices), for offline
-- VRAM ground truth. Default: start + two mid-cycle points.
local SNAP_TICKS = {}
for tok in probe.getenv("LEGAIA_SNAP_TICKS", "2,50,100"):gmatch("[^,]+") do
    SNAP_TICKS[tonumber(tok)] = true
end

local MOVE_IMAGE = 0x80058490
local LOAD_IMAGE = 0x800583C8
local STORE_IMAGE = 0x8005842C

os.execute(string.format("mkdir -p %q", OUT_DIR))

local xfer = probe.csv_open(OUT_DIR .. "/xfer.csv",
    "tick,kind,ra,x,y,w,h,a1,a2")
local ticks = probe.csv_open(OUT_DIR .. "/ticks.csv", "tick,dt")
local hits = 0
local tick = -1

-- RECT pointers from the CLUT-fx family live in the SCRATCHPAD
-- (0x1F8000xx), which mem.read_u16 can't see. Read either space.
local function read_u16_any(addr)
    local base = bit.band(addr, 0xFFFFFC00)
    if base == 0x1F800000 then
        return mem.read_scratch_u8(addr) + mem.read_scratch_u8(addr + 1) * 256
    end
    return mem.read_u16(addr) or 0xFFFF
end

local function on_fire(kind)
    return function()
        local r = PCSX.getRegisters()
        local ra = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF)
        local a0 = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
        local a1 = bit.band(tonumber(r.GPR.n.a1) or 0, 0xFFFFFFFF)
        local a2 = bit.band(tonumber(r.GPR.n.a2) or 0, 0xFFFFFFFF)
        local x = read_u16_any(a0)
        local y = read_u16_any(a0 + 2)
        local w = read_u16_any(a0 + 4)
        local h = read_u16_any(a0 + 6)
        hits = hits + 1
        xfer:row("%d,%s,0x%08X,%d,%d,%d,%d,0x%08X,0x%08X",
            tick, kind, ra, x, y, w, h, a1, a2)
    end
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    hold_button = HOLD_BTN,
    hold_frames = HOLD_FR,

    on_arm = function()
        probe.arm_breakpoint(MOVE_IMAGE, "Exec", 4, "moveimage", on_fire("move"))
        probe.arm_breakpoint(LOAD_IMAGE, "Exec", 4, "loadimage", on_fire("load"))
        probe.arm_breakpoint(STORE_IMAGE, "Exec", 4, "storeimage", on_fire("store"))
        return {}
    end,

    on_capture = function(_ctx, elapsed)
        tick = elapsed
        ticks:row("%d,%d", elapsed, mem.read_scratch_u8(0x1F800393))
        if SNAP_TICKS[elapsed] then
            sstate.save(string.format("%s/snap_t%04d.sstate", OUT_DIR, elapsed))
        end
    end,

    on_done = function()
        xfer:close()
        ticks:close()
        PCSX.log(string.format("=== ocean_moveimage: %d hit(s) ===", hits))
    end,
})
