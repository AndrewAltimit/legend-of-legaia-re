-- autorun_shiny_mouth_experiment.lua
--
-- Isolate the cause of Vahn's corrupted victory-pose mouth on the shiny ROM.
-- Loads slot 9 (victory pose) and, per LEGAIA_COND, neutralises one suspect,
-- then screenshots several frames so the mouth can be compared:
--   COND=baseline : touch nothing (should reproduce the corruption)
--   COND=flag     : clear SHINY_CAST_FLAG (0x80078358) -> disables K/J hooks
--   COND=rec1c0   : clear char0 record+0x1C0 shiny array
--   COND=both     : clear both
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SHINY_CAST_FLAG = 0x80078358
local REC0_1C0 = 0x80084708 + 0x1C0
local COND = probe.getenv("LEGAIA_COND", "baseline")

local function take_fb(stem)
  local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
  if not ok or ss == nil then PCSX.log("[mouth] no screenshot"); return end
  local bpp = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
  local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
  if fh then fh:write(tostring(ss.data)); fh:close() end
  local mh = io.open(probe.out_path(stem .. ".meta"), "w")
  if mh then mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
    tonumber(ss.width), tonumber(ss.height), bpp)); mh:close() end
  PCSX.log("[mouth] wrote " .. stem)
end

local applied = false
probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate9"),
  capture_frames = probe.getenv_num("LEGAIA_FRAMES", 200),
  on_arm = function() return {} end,
  on_capture = function(ctx, el)
    if not applied then
      if COND == "flag" or COND == "both" then probe.write_u8(SHINY_CAST_FLAG, 0) end
      if COND == "rec1c0" or COND == "both" then probe.write_u8(REC0_1C0, 0) end
      applied = true
      PCSX.log("[mouth] cond=" .. COND .. " applied")
    end
    -- re-assert the clear every frame in case the game rewrites it
    if COND == "flag" or COND == "both" then probe.write_u8(SHINY_CAST_FLAG, 0) end
    if COND == "rec1c0" or COND == "both" then probe.write_u8(REC0_1C0, 0) end
    if el == 60 then take_fb("mouth_"..COND.."_f60") end
    if el == 90 then take_fb("mouth_"..COND.."_f90") end
    if el == 130 then take_fb("mouth_"..COND.."_f130"); ctx.request_quit = true end
  end,
})
