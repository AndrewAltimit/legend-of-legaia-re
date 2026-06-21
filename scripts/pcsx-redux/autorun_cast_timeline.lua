package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local function fb(stem)
  local ok,ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
  if not ok or ss==nil then return end
  local bpp=(tonumber(ss.bpp) or 0)>16 and 24 or 16
  local h=io.open(probe.out_path(stem..".raw"),"wb"); if h then h:write(tostring(ss.data)); h:close() end
  local m=io.open(probe.out_path(stem..".meta"),"w"); if m then m:write(string.format("width=%d\nheight=%d\nbpp=%d\n",tonumber(ss.width),tonumber(ss.height),bpp)); m:close() end
end
probe.run({
  sstate=probe.getenv("LEGAIA_SSTATE",os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate3"),
  capture_frames=probe.getenv_num("LEGAIA_FRAMES",300),
  on_arm=function() return {} end,
  on_capture=function(ctx,el)
    -- shoot the post-flash window where box+text should be visible
    for _,t in ipairs({140,160,180,200,220,240,260}) do
      if el==t then fb(string.format("cast_%03d",t)) end
    end
    if el>=270 then ctx.request_quit=true end
  end,
})
