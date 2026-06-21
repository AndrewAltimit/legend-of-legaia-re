-- Test whether the shiny level byte (0x82) is the root of the blank level-up box
-- (and maybe the mouth corruption). Clear char0's spell-level byte to 0x02 each
-- frame and screenshot the victory/level-up; compare to baseline (leave 0x82).
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local LV=0x80084140+0x729
local CLEAR = probe.getenv_num("LEGAIA_CLEAR",1)==1
local function fb(s) local ok,ss=pcall(function() return PCSX.GPU.takeScreenShot() end); if not ok or not ss then return end
  local bpp=(tonumber(ss.bpp) or 0)>16 and 24 or 16
  local h=io.open(probe.out_path(s..".raw"),"wb"); h:write(tostring(ss.data)); h:close()
  local m=io.open(probe.out_path(s..".meta"),"w"); m:write(string.format("width=%d\nheight=%d\nbpp=%d\n",tonumber(ss.width),tonumber(ss.height),bpp)); m:close() end
probe.run({
  sstate=probe.getenv("LEGAIA_SSTATE",os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate5"),
  capture_frames=160,
  on_arm=function() return {} end,
  on_capture=function(ctx,el)
    if CLEAR then probe.write_u8(LV,0x02) end   -- mask shiny bit each frame
    if el==60 then fb("lv_60") end
    if el==110 then fb("lv_110") end
    if el>=150 then fb("lv_final"); ctx.request_quit=true end
  end,
})
