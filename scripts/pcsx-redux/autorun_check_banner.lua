package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local DRAW=0x80036888
local f=assert(io.open(probe.out_path("check_banner.txt"),"w"))
local n,shot,sawbanner=0,0,false
local function fb(s) local ok,ss=pcall(function() return PCSX.GPU.takeScreenShot() end); if not ok or not ss then return end
  local bpp=(tonumber(ss.bpp) or 0)>16 and 24 or 16
  local h=io.open(probe.out_path(s..".raw"),"wb"); h:write(tostring(ss.data)); h:close()
  local m=io.open(probe.out_path(s..".meta"),"w"); m:write(string.format("width=%d\nheight=%d\nbpp=%d\n",tonumber(ss.width),tonumber(ss.height),bpp)); m:close() end
probe.run({
  sstate=probe.getenv("LEGAIA_SSTATE",os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate4"),
  capture_frames=60,
  on_arm=function()
    probe.arm_breakpoint(DRAW,"Exec",4,"d",function()
      local r=PCSX.getRegisters()
      local a0=(tonumber(r.GPR.n.a0) or 0)%0x100000000
      local a1=(tonumber(r.GPR.n.a1) or 0)%0x100000000
      local a3=(tonumber(r.GPR.n.a3) or 0)%0x100000000
      local sp=(tonumber(r.GPR.n.sp) or 0)%0x100000000
      local y=probe.read_u32(sp+0x10) or 0
      if a1==0x801C then sawbanner=true
        if n<6 then f:write(string.format("BANNER a0=0x%08X b0=0x%02X X=%d Y=%d\n",a0,probe.read_u8(a0) or 0,a3,y)); f:flush(); n=n+1 end
      end
    end)
    return {}
  end,
  on_capture=function(ctx,el)
    if sawbanner and shot<3 then fb("chk_"..shot); shot=shot+1 end
    if el>=40 then if not sawbanner then f:write("NO BANNER DRAWN in 40 frames\n") end fb("chk_final"); f:close(); ctx.request_quit=true end
  end,
})
