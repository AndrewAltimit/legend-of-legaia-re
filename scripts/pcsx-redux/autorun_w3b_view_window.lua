-- autorun_w3b_view_window.lua
--
-- Does retail re-stamp the camera's visible-tile window at every field
-- entry, or does a scene's scripted window survive into the next scene?
--
-- The window is four SIGNED bytes at scratchpad `0x1F8003E8..EB`, written
-- by the field draw-context primer `FUN_801DE37C` on scene entry and
-- overwritten per scene by field-VM op `0x46` (`VIEW_WINDOW`). This probe
-- polls them alongside the scene name and game mode, and logs one line per
-- distinct `(scene, mode, window)` tuple, while a d-pad ladder walks the
-- player through doors.
--
-- Poll-only: no breakpoints, so it also runs under --timing.
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh \
--       --iso <a PPF-free copy of the disc> \
--       --scenario s4_rimelm_door_transition \
--       --lua scripts/pcsx-redux/autorun_w3b_view_window.lua --frames 3000
--
-- Outputs: w3b_view_window.csv (one row per change), .log (summary)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 3000)

local OUT_CSV = probe.out_path("w3b_view_window.csv")
local OUT_LOG = probe.out_path("w3b_view_window.log")

local WINDOW = 0x1F8003E8
local SCENE  = 0x8007050C
local MODE   = 0x8007B83C

local lines = {}
local function logf(fmt, ...)
  local s = string.format(fmt, ...)
  lines[#lines+1] = s
  PCSX.log("[viewwin] " .. s)
end
local function sb(v)
  v = bit.band(v or 0, 0xFF)
  if v >= 0x80 then return v - 0x100 end
  return v
end
local function scene_name()
  local out = {}
  for i = 0, 7 do
    local b = probe.read_u8(SCENE + i)
    if b == nil or b < 0x20 or b >= 0x7F then break end
    out[#out+1] = string.char(b)
  end
  return table.concat(out)
end

-- Walk the pad so the player finds a door: hold one cardinal per block,
-- pulsing CROSS so a dialogue or prompt does not park the run.
local HOLDS = { probe.BTN.UP, probe.BTN.LEFT, probe.BTN.DOWN, probe.BTN.RIGHT }
local hold_i, held = 0, nil

local csv, rows, last = nil, 0, nil
local seen = {}

probe.run({
  sstate = SSTATE,
  capture_frames = FRAMES,
  on_arm = function()
    csv = probe.csv_open(OUT_CSV, "seq,vsync,scene,mode,w0,w1,w2,w3")
    probe.env.write_manifest("autorun_w3b_view_window.lua",
      { sstate = SSTATE, frames = FRAMES })
    return {}
  end,
  on_capture = function(ctx, el)
    local w = {
      sb(probe.read_scratch_u8(WINDOW)),
      sb(probe.read_scratch_u8(WINDOW + 1)),
      sb(probe.read_scratch_u8(WINDOW + 2)),
      sb(probe.read_scratch_u8(WINDOW + 3)),
    }
    local sc = scene_name()
    local md = probe.read_u8(MODE) or 0
    local key = string.format("%s|%d|%d,%d,%d,%d", sc, md, w[1], w[2], w[3], w[4])
    if key ~= last then
      last = key
      seen[key] = (seen[key] or 0) + 1
      rows = rows + 1
      if rows <= 4000 then
        csv:row("%d,%d,%s,%d,%d,%d,%d,%d", rows, el, sc, md, w[1], w[2], w[3], w[4])
      end
      logf("f=%d scene=%s mode=%d window=(%d,%d,%d,%d)", el, sc, md,
        w[1], w[2], w[3], w[4])
    end
    if el >= 30 then
      local want = math.floor((el - 30) / 200) % #HOLDS + 1
      if want ~= hold_i then
        if held then probe.pad_release(held) end
        hold_i = want; held = HOLDS[hold_i]
        probe.pad_force(held)
      end
      if (el % 47) == 0 then probe.pad_force(probe.BTN.CROSS)
      elseif (el % 47) == 5 then probe.pad_release(probe.BTN.CROSS) end
    end
  end,
  on_summary = function()
    logf("--- distinct (scene, mode, window) tuples ---")
    local ks = {}
    for k, n in pairs(seen) do ks[#ks+1] = { k, n } end
    table.sort(ks, function(a, b) return a[2] > b[2] end)
    for i = 1, math.min(#ks, 30) do
      logf("  %s : %d visit(s)", ks[i][1], ks[i][2])
    end
    local fh = io.open(OUT_LOG, "w")
    if fh then fh:write(table.concat(lines, "\n") .. "\n"); fh:close() end
    if csv then csv:close() end
  end,
})
