-- autorun_battle_moveimage_trace.lua
--
-- Trace every libgpu MoveImage request during idle battle frames, to pin the
-- writer of the small facial-texel overwrite the live captures show inside
-- the battle party texture bands (the face rows of the head section's rect):
-- the alternate face frames are VRAM-resident lower in the same band, so the
-- per-frame facial animator is expected to be a VRAM->VRAM MoveImage. An exec
-- breakpoint on the MoveImage wrapper FUN_80058490 logs caller RA + the RECT
-- (source x,y,w,h) + destination (x,y) per fire.
--
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate5 \
--   LEGAIA_FRAMES=900 \
--   LEGAIA_OUT_DIR=/tmp/faceprobe \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_battle_moveimage_trace.lua \
--       timeout --kill-after=30s 700s bash scripts/pcsx-redux/run_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem = require("probe.mem")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate5")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 900)
local OUT_DIR = probe.getenv("LEGAIA_OUT_DIR", "/tmp/faceprobe")

-- libgpu MoveImage(RECT *rect, int dst_x, int dst_y) wrapper.
local MOVE_IMAGE = 0x80058490
-- libgpu LoadImage(RECT *rect, u_long *data) wrapper.
local LOAD_IMAGE = 0x800583C8

local HOLD_NAME = probe.getenv("LEGAIA_HOLD", "")
local HOLD_FR   = probe.getenv_num("LEGAIA_HOLD_FRAMES", 0)
local pad = require("probe.pad")
local HOLD_BTN = (HOLD_NAME ~= "" and pad.BTN[HOLD_NAME]) or nil

os.execute(string.format("mkdir -p %q", OUT_DIR))

local csv = probe.csv_open(OUT_DIR .. "/moveimage.csv",
    "tick,kind,ra,src_x,src_y,w,h,dst_x,dst_y")
local hits = 0
local tick = 0

local function on_fire(kind)
    return function()
        local r = PCSX.getRegisters()
        local ra = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF)
        local a0 = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
        local a1 = bit.band(tonumber(r.GPR.n.a1) or 0, 0xFFFFFFFF)
        local a2 = bit.band(tonumber(r.GPR.n.a2) or 0, 0xFFFFFFFF)
        local sx = mem.read_u16(a0) or 0xFFFF
        local sy = mem.read_u16(a0 + 2) or 0xFFFF
        local w = mem.read_u16(a0 + 4) or 0xFFFF
        local h = mem.read_u16(a0 + 6) or 0xFFFF
        hits = hits + 1
        csv:row("%d,%s,0x%08X,%d,%d,%d,%d,%d,%d",
            tick, kind, ra, sx, sy, w, h,
            bit.band(a1, 0xFFFF), bit.band(a2, 0xFFFF))
    end
end

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,
    hold_button    = HOLD_BTN,
    hold_frames    = HOLD_FR,
    out_path       = OUT_DIR .. "/moveimage.csv",

    on_arm = function()
        probe.arm_breakpoint(MOVE_IMAGE, "Exec", 4, "moveimage_entry", on_fire("move"))
        -- The LoadImage wrapper fires every frame on the overworld (CLUT-cell
        -- cycling) and slows interpreter+BP emulation badly; opt in only when
        -- the upload side is the target.
        if probe.getenv_num("LEGAIA_TRACE_LOADIMAGE", 0) ~= 0 then
            probe.arm_breakpoint(LOAD_IMAGE, "Exec", 4, "loadimage_entry", on_fire("load"))
        end
        return {}
    end,

    on_capture = function(_ctx, elapsed) tick = elapsed end,

    on_done = function()
        csv:close()
        PCSX.log(string.format("=== battle_moveimage_trace: %d hit(s) ===", hits))
    end,
})
