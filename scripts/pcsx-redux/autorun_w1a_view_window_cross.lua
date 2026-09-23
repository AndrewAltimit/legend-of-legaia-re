-- autorun_w1a_view_window_cross.lua
--
-- Does a scripted camera visible-tile window survive into the NEXT SCENE?
--
-- [`autorun_w3b_view_window.lua`](autorun_w3b_view_window.lua) measured the
-- per-region half: within one scene the four signed bytes at scratchpad
-- `0x1F8003E8..EB` alternate as the player crosses camera regions. What no
-- capture had crossed is a scene boundary. This probe does exactly that:
-- it holds one direction into a scene-changing portal and writes EVERY
-- vsync - the four window bytes, the scene name and the game mode - so the
-- frame the scene word changes and the frames either side of it are all in
-- the record.
--
-- The answer is one of: the next scene's entry re-stamps the window (what
-- the port does, via the field draw-context primer `FUN_801DE37C`), or the
-- window carries the last region's values across the load.
--
-- Note on doors: a Rim Elm "house door" is NOT a scene change - the
-- catalogued `door_warp_rim_elm_to_mei_house` / `mei_house_inside` pair
-- stays in `town0c`, and the S4 anchor's front door is an intra-`town01`
-- warp. Use an overworld -> town portal (`overworld_into_town_man_load`)
-- or another field-to-field op-`0x3F` door.
--
-- Env:
--   LEGAIA_HOLD      pad direction to hold: up|down|left|right (default down)
--   LEGAIA_HOLD_FROM vsync to start holding (default 30)
--   LEGAIA_HOLD_FOR  vsyncs to hold (default 90; 0 = hold to the end)
--   LEGAIA_FRAMES    capture window (default 900)
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh \
--       --iso <a PPF-free copy of the disc> \
--       --scenario overworld_into_town_man_load \
--       --lua scripts/pcsx-redux/autorun_w1a_view_window_cross.lua --frames 900
--
-- Outputs: w1a_view_window_cross.csv (one row per vsync), .log (summary)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE    = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES    = probe.getenv_num("LEGAIA_FRAMES", 900)
local HOLD_NAME = probe.getenv("LEGAIA_HOLD", "down")
local HOLD_FROM = probe.getenv_num("LEGAIA_HOLD_FROM", 30)
local HOLD_FOR  = probe.getenv_num("LEGAIA_HOLD_FOR", 90)

local OUT_CSV = probe.out_path("w1a_view_window_cross.csv")
local OUT_LOG = probe.out_path("w1a_view_window_cross.log")

local WINDOW = 0x1F8003E8
local SCENE  = 0x8007050C
local MODE   = 0x8007B83C

local DIRS = {
  up = probe.BTN.UP, down = probe.BTN.DOWN,
  left = probe.BTN.LEFT, right = probe.BTN.RIGHT,
}
local HOLD = DIRS[HOLD_NAME] or probe.BTN.DOWN

local lines = {}
local function logf(fmt, ...)
  local s = string.format(fmt, ...)
  lines[#lines + 1] = s
  PCSX.log("[w1a_vw] " .. s)
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
    out[#out + 1] = string.char(b)
  end
  return table.concat(out)
end

local csv, held, last_scene, last_key = nil, false, nil, nil

probe.run({
  sstate = SSTATE,
  capture_frames = FRAMES,
  on_arm = function()
    csv = probe.csv_open(OUT_CSV, "vsync,scene,mode,w0,w1,w2,w3,holding")
    probe.env.write_manifest("autorun_w1a_view_window_cross.lua",
      { sstate = SSTATE, frames = FRAMES, hold = HOLD_NAME,
        hold_from = HOLD_FROM, hold_for = HOLD_FOR })
    logf("holding %s from vsync %d for %d vsync(s)", HOLD_NAME, HOLD_FROM, HOLD_FOR)
    return {}
  end,
  on_capture = function(_, el)
    local w0 = sb(probe.read_scratch_u8(WINDOW))
    local w1 = sb(probe.read_scratch_u8(WINDOW + 1))
    local w2 = sb(probe.read_scratch_u8(WINDOW + 2))
    local w3 = sb(probe.read_scratch_u8(WINDOW + 3))
    local sc = scene_name()
    local md = probe.read_u8(MODE) or 0

    -- Every vsync goes in the CSV: the question is which side of the scene
    -- word's change the window's own change lands on, and a change-only
    -- record cannot answer that.
    csv:row("%d,%s,%d,%d,%d,%d,%d,%d", el, sc, md, w0, w1, w2, w3,
      held and 1 or 0)

    local key = string.format("%s|%d|%d,%d,%d,%d", sc, md, w0, w1, w2, w3)
    if key ~= last_key then
      last_key = key
      logf("f=%d scene=%s mode=%d window=(%d,%d,%d,%d)", el, sc, md, w0, w1, w2, w3)
    end
    if sc ~= last_scene then
      if last_scene ~= nil then
        logf("SCENE CHANGE at vsync %d: %s -> %s", el, last_scene, sc)
      end
      last_scene = sc
    end

    if el == HOLD_FROM then
      probe.pad_force(HOLD); held = true
    elseif HOLD_FOR > 0 and el == HOLD_FROM + HOLD_FOR then
      probe.pad_release(HOLD); held = false
    end
  end,
  on_summary = function()
    local fh = io.open(OUT_LOG, "w")
    if fh then fh:write(table.concat(lines, "\n") .. "\n"); fh:close() end
    if csv then csv:close() end
  end,
})
