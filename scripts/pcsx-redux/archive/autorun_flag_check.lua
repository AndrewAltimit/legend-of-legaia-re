package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local f=assert(io.open(probe.out_path("flag_check.txt"),"w"))
probe.run({
  sstate=probe.getenv("LEGAIA_SSTATE",os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate4"),
  capture_frames=8,
  on_arm=function() return {} end,
  on_capture=function(ctx,el)
    if el==4 then
      local flag=probe.read_u8(0x80078358) or 0
      f:write(string.format("SHINY_CAST_FLAG(0x80078358)=0x%02X\n",flag))
      -- captured Gimard spell-level byte: char0 SC 0x80084140+0x729
      for n=0,3 do
        local lv=probe.read_u8(0x80084140 + n*0x414 + 0x729) or 0
        f:write(string.format("char%d +0x729=0x%02X\n",n,lv))
      end
      -- banner string at 0x80078200
      local s={}
      for i=0,9 do s[#s+1]=string.format("%02X",probe.read_u8(0x80078200+i) or 0) end
      f:write("str@0x80078200="..table.concat(s," ").."\n")
      f:close(); ctx.request_quit=true
    end
  end,
})
