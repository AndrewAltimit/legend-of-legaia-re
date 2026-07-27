-- autorun_muscle_arena_shots.lua -- run the Muscle Dome load-transition
-- save forward and screenshot the live arena at intervals, tapping Cross
-- to advance any intro prompt. Vsync-event only (works under --fast).
--
--   LEGAIA_SSTATE = the minigame_muscle_dome_pcsx library state
--   LEGAIA_OUT    = output dir (shots land as shot_<vsync>.screen[.meta])
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local SHOT_EVERY = 240
local TOTAL = tonumber(probe.getenv("LEGAIA_FRAMES", "3600"))

local function shot(name)
  local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
  if ok and ss then
    local bpp = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
    local h = io.open(probe.out_path(name .. ".screen"), "wb")
    if not h then
      PCSX.log("[probe] cannot open shot output " .. name)
      return
    end
    h:write(tostring(ss.data)); h:close()
    local m = io.open(probe.out_path(name .. ".screen.meta"), "w")
    if m then
      m:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
        tonumber(ss.width), tonumber(ss.height), bpp))
      m:close()
    end
  end
end

probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate4"),
  capture_frames = TOTAL + 8,
  on_arm = function() return {} end,
  on_capture = function(ctx, el)
    -- Tap Cross for 4 vsyncs out of every 120 to advance prompts.
    local phase = el % 120
    if phase == 40 then pad.force(pad.BTN.CROSS) end
    if phase == 44 then pad.release(pad.BTN.CROSS) end
    if el > 0 and el % SHOT_EVERY == 0 then
      pcall(function() shot(string.format("shot_%05d", el)) end)
    end
    if el >= TOTAL then ctx.request_quit = true end
  end,
})
