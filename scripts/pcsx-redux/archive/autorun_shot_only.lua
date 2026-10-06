package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
probe.run({
  sstate=probe.getenv("LEGAIA_SSTATE",os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate4"),
  capture_frames=12,
  on_arm=function() return {} end,
  on_capture=function(ctx,el)
    if el==6 then
      local ok,ss=pcall(function() return PCSX.GPU.takeScreenShot() end)
      if ok and ss then
        local bpp=(tonumber(ss.bpp) or 0)>16 and 24 or 16
        local h=io.open(probe.out_path("s4.raw"),"wb"); h:write(tostring(ss.data)); h:close()
        local m=io.open(probe.out_path("s4.meta"),"w"); m:write(string.format("width=%d\nheight=%d\nbpp=%d\n",tonumber(ss.width),tonumber(ss.height),bpp)); m:close()
      end
      ctx.request_quit=true
    end
  end,
})
