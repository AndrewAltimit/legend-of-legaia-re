package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local OUT = probe.out_path("widget_dump.txt")
local f = assert(io.open(OUT,"w"))
probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate4"),
  capture_frames = 8,
  on_arm=function() return {} end,
  on_capture=function(ctx,el)
    if el==3 then
      -- widget array spans ~0x800A8000..0x800A8400; dump every 0x40 record's key fields
      for va=0x800A8000,0x800A8400,0x20 do
        local t1c = probe.read_u8(va+0x1c) or 0
        local sty = probe.read_u16(va+0x12) or 0
        local sp  = probe.read_u32(va+0x18) or 0
        local f10 = probe.read_u16(va+0x10) or 0
        local f14 = probe.read_u16(va+0x14) or 0
        if sty~=0 or sp~=0 or t1c~=0 then
          f:write(string.format("0x%08X type1c=0x%02X style12=0x%04X f10=0x%04X f14=0x%04X str18=0x%08X\n",
            va,t1c,sty,f10,f14,sp))
        end
      end
      f:close()
      ctx.request_quit=true
    end
  end,
})
