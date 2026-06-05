-- autorun_battle_render_capture.lua
--
-- Capture the ground-truth battle-render parameters the clean-room engine has
-- been eyeballing: the orbit camera state, the func_0x801d02c0 flat ground-grid
-- setup (the grass tile), and the live battle actor formation (world positions
-- + scale). Run against a battle save state (game mode 0x15), ideally the
-- map01 overworld Vahn-vs-Gobu-Gobu fight paused on the Begin/Run menu.
--
--   LEGAIA_SSTATE=<path-to-battle.sstate> \
--     bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_battle_render_capture.lua
--
-- Output: log lines (camera / grid / actors) to the probe log.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate5")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 8)

-- Camera-state globals (see project_battle_camera_re).
local PITCH = 0x8007B790 -- _DAT_8007b790  camera pitch (12-bit, 4096 = 360)
local YAW   = 0x8007B792 -- _DAT_8007b792  orbit azimuth
local ROLL  = 0x8007B794 -- _DAT_8007b794  roll / wm-azimuth
local TRX   = 0x800840B8 -- _DAT_800840b8  eye-space TR.x
local TRY   = 0x800840BC -- _DAT_800840bc  eye-space TR.y
local TRZ   = 0x800840C0 -- _DAT_800840c0  eye-space TR.z
local ZOOM  = 0x8007B6F4 -- _DAT_8007b6f4  GTE projection H
local GMODE = 0x8007B83C -- _DAT_8007b83c  game mode (battle = 0x15)
local BD24  = 0x8007BD24 -- _DAT_8007bd24  -> battle context struct
local DOME  = 0x8007680C -- dome descriptor (mesh slot at +4 = DAT_80076810)

-- func_0x801d02c0 = the flat ground-grid renderer (battle overlay). The grass
-- tile texpage/clut + per-cell colour constants are written to the scratchpad
-- at the top of the function; grid dims live at 0x1f8003f8/fa.
local GRID_FN = 0x801D02C0
local SCRATCH = 0x1F800034 -- first of the 16 tile constant words
local GRID_W  = 0x1F8003F8
local GRID_H  = 0x1F8003FA

local function s16(addr)
  local v = probe.read_u16(addr) or 0
  if v >= 0x8000 then v = v - 0x10000 end
  return v
end

local function dump_camera()
  PCSX.log(string.format(
    "[cam] mode=0x%02X pitch=%d yaw=%d roll=%d  TR=(%d,%d,%d)  H=%d",
    probe.read_u8(GMODE) or 0,
    s16(PITCH), s16(YAW), s16(ROLL),
    s16(TRX), s16(TRY), s16(TRZ), s16(ZOOM)))
end

local function dump_grid()
  PCSX.log(string.format("[grid] dims = %d x %d cells (0x200 pitch)",
    s16(GRID_W), s16(GRID_H)))
  local words = {}
  for i = 0, 15 do
    words[#words + 1] = string.format("%08X", probe.read_u32(SCRATCH + i * 4) or 0)
  end
  PCSX.log("[grid] tile constants 0x1f800034..70: " .. table.concat(words, " "))
end

local function dump_actors()
  local ctx = probe.read_u32(BD24) or 0
  if ctx < 0x80000000 or ctx >= 0x80200000 then
    PCSX.log("[actors] battle ctx not resident")
    return
  end
  PCSX.log(string.format("[actors] ctx=0x%08X party_count=%d",
    ctx, probe.read_u8(ctx + 0x275) or 0))
  for n = 0, 7 do
    local ap = probe.read_u32(ctx + 0x1114 + n * 4) or 0
    if ap < 0x80000000 or ap >= 0x80200000 then break end
    local f = {}
    for off = 0x10, 0x30, 2 do f[#f + 1] = string.format("%+d", s16(ap + off)) end
    PCSX.log(string.format("[actor%d] @0x%08X id+0x5a=%d  [+0x10..0x30 i16]: %s",
      n, ap, s16(ap + 0x5a), table.concat(f, " ")))
  end
  PCSX.log(string.format("[dome] descriptor@0x%08X mesh_slot(+4)=%d",
    DOME, s16(DOME + 4)))
end

probe.run({
  sstate = SSTATE_PATH,
  capture_frames = FRAMES,
  on_arm = function()
    PCSX.log("== battle render capture ==")
    if (probe.read_u8(GMODE) or 0) ~= 0x15 then
      PCSX.log(string.format("[warn] game mode is 0x%02X, expected 0x15 (battle)",
        probe.read_u8(GMODE) or 0))
    end
    dump_camera()
    dump_grid()
    dump_actors()
    -- Catch the grid renderer live so the scratchpad read is the value the GTE
    -- actually consumed this frame (it's rewritten every frame).
    probe.arm_breakpoint(GRID_FN, "Exec", 4, "grid_fn", function()
      PCSX.log("[grid] (live, inside func_0x801d02c0)")
      dump_grid()
      probe.disarm_all()
    end)
    return { { addr = GRID_FN, name = "grid_fn" } }
  end,
  on_capture = function() end,
})
