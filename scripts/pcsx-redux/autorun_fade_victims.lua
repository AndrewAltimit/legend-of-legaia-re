package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local FADE_READER=0x8004AD0C
local TBL=0x801C9370
local f=assert(io.open(probe.out_path("fade_victims.txt"),"w"))
local seen={}
local function fb(s) local ok,ss=pcall(function() return PCSX.GPU.takeScreenShot() end); if not ok or not ss then return end
  local bpp=(tonumber(ss.bpp) or 0)>16 and 24 or 16
  local h=io.open(probe.out_path(s..".raw"),"wb"); h:write(tostring(ss.data)); h:close()
  local m=io.open(probe.out_path(s..".meta"),"w"); m:write(string.format("width=%d\nheight=%d\nbpp=%d\n",tonumber(ss.width),tonumber(ss.height),bpp)); m:close() end
probe.run({
  sstate=probe.getenv("LEGAIA_SSTATE",os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate5"),
  capture_frames=140,
  on_arm=function()
    probe.arm_breakpoint(FADE_READER,"Exec",4,"fr",function()
      local r=PCSX.getRegisters()
      local s1=(tonumber(r.GPR.n.s1) or 0)%0x100000000
      local s7=probe.read_u32(TBL+7*4) or 0
      local flag=probe.read_u8(0x80078358) or 0
      local key=string.format("%08X",s1)
      if seen[key] then return end; seen[key]=true
      local slot=-1; for i=0,15 do if (probe.read_u32(TBL+i*4) or 0)==s1 then slot=i break end end
      local match=(s1==s7 and bit.band(flag,0x80)~=0) and "  <== K FADES (==slot7, flag set)" or ""
      f:write(string.format("draw s1=0x%08X slot=%d flag=0x%02X s7=0x%08X%s\n",s1,slot,flag,s7,match))
      f:flush()
    end)
    return {}
  end,
  on_capture=function(ctx,el)
    if el==2 then
      local flag=probe.read_u8(0x80078358) or 0
      f:write(string.format("[post-load] flag=0x%02X slot7=0x%08X slot0=0x%08X\n",flag,probe.read_u32(TBL+28) or 0,probe.read_u32(TBL) or 0))
    end
    if el==60 then fb("victory_60") end
    if el==120 then fb("victory_120"); end
    if el>=130 then f:write("done\n"); f:close(); ctx.request_quit=true end
  end,
})
