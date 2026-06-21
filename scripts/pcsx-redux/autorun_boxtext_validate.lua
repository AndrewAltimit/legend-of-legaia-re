-- Validate option (a): inject "+35% DMG!" as the empty top box's OWN text
-- (FUN_8003cc98, the case-0xD message-window text drawer). If the text appears
-- inside the box, it will track the box's animated position automatically.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local BOXTEXT = 0x8003CC98   -- box/message-window text drawer (a0 = string)
local STR = 0x80078200       -- "+35% DMG!" already in the patched SCUS gap
local nlog, swapped, shot = 0, false, 0
local function fb(stem)
  local ok,ss=pcall(function() return PCSX.GPU.takeScreenShot() end); if not ok or ss==nil then return end
  local bpp=(tonumber(ss.bpp) or 0)>16 and 24 or 16
  local h=io.open(probe.out_path(stem..".raw"),"wb"); if h then h:write(tostring(ss.data)); h:close() end
  local m=io.open(probe.out_path(stem..".meta"),"w"); if m then m:write(string.format("width=%d\nheight=%d\nbpp=%d\n",tonumber(ss.width),tonumber(ss.height),bpp)); m:close() end
end
probe.run({
  sstate=probe.getenv("LEGAIA_SSTATE",os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate4"),
  capture_frames=40,
  on_arm=function()
    probe.arm_breakpoint(BOXTEXT,"Exec",4,"box",function()
      local r=PCSX.getRegisters()
      local a0=(tonumber(r.GPR.n.a0) or 0)%0x100000000
      local a1=(tonumber(r.GPR.n.a1) or 0)%0x100000000
      local a3=(tonumber(r.GPR.n.a3) or 0)%0x100000000
      if nlog<8 then PCSX.log(string.format("[box] FUN_8003cc98 a0=0x%08X a1=0x%08X a3(X)=%d",a0,a1,a3)); nlog=nlog+1 end
      if a0==0 then r.GPR.n.a0=STR; swapped=true end  -- fill the empty box
    end)
    return {}
  end,
  on_capture=function(ctx,el)
    if swapped then fb("boxtext_"..shot); shot=shot+1; swapped=false; if shot>=3 then ctx.request_quit=true end end
    if el>=30 then fb("boxtext_final"); ctx.request_quit=true end
  end,
})
