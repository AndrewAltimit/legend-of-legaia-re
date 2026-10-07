-- Log every battle HUD widget drawn this frame (sstate4 = box+text on screen):
-- at the FUN_80031d00 dispatch (jr v0 @0x800321ac), record the widget ptr s4,
-- its type (+0x1c), and position-ish fields, to identify the top box + its Y.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local DISPATCH = 0x800321AC
local f = assert(io.open(probe.out_path("widget_dispatch.txt"),"w"))
local seen = {}
probe.run({
  sstate=probe.getenv("LEGAIA_SSTATE",os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate4"),
  capture_frames=10,
  on_arm=function()
    probe.arm_breakpoint(DISPATCH,"Exec",4,"disp",function()
      local r=PCSX.getRegisters()
      local s4=(tonumber(r.GPR.n.s4) or 0)%0x100000000
      local s8=(tonumber(r.GPR.n.s8) or 0)%0x100000000
      if s4<0x80000000 or s4>=0x80200000 then return end
      local key=string.format("%08X",s4)
      if seen[key] then return end
      seen[key]=true
      local ty=probe.read_u8(s4+0x1c) or 0
      local fields={}
      for o=0x0,0x1e,2 do fields[#fields+1]=string.format("%02X=%04X",o,probe.read_u16(s4+o) or 0) end
      f:write(string.format("s4=0x%08X type=0x%02X s8(X)=%d | %s\n",s4,ty,s8,table.concat(fields," ")))
      f:flush()
    end)
    return {}
  end,
  on_capture=function(ctx,el) if el>=6 then ctx.request_quit=true end end,
  on_summary=function() f:write("done\n"); f:close() end,
})
