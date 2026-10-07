package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local LV=0x80084140+0x729
local CLEAR = probe.getenv_num("LEGAIA_CLEAR",1)==1
local function fb(s) local ok,ss=pcall(function() return PCSX.GPU.takeScreenShot() end); if not ok or not ss then return end
  local bpp=(tonumber(ss.bpp) or 0)>16 and 24 or 16
  local h=io.open(probe.out_path(s..".raw"),"wb"); h:write(tostring(ss.data)); h:close()
  local m=io.open(probe.out_path(s..".meta"),"w"); m:write(string.format("width=%d\nheight=%d\nbpp=%d\n",tonumber(ss.width),tonumber(ss.height),bpp)); m:close() end
probe.run({
  sstate=probe.getenv("LEGAIA_SSTATE",os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate6"),
  capture_frames=240,
  on_arm=function() return {} end,
  on_capture=function(ctx,el)
    if CLEAR then probe.write_u8(LV,0x02) end
    for _,t in ipairs({185,200,215,230}) do if el==t then fb("up_"..t) end end
    if el>=235 then ctx.request_quit=true end
  end,
})
