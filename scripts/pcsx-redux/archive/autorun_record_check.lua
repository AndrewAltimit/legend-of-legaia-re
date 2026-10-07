package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local f=assert(io.open(probe.out_path("record_check.txt"),"w"))
probe.run({
  sstate=probe.getenv("LEGAIA_SSTATE",os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate7"),
  capture_frames=8,
  on_arm=function() return {} end,
  on_capture=function(ctx,el)
    if el==4 then
      f:write(string.format("SHINY_CAST_FLAG(0x80078358)=0x%02X\n",probe.read_u8(0x80078358) or 0))
      for n=0,3 do
        local rec=0x80084708+n*0x414
        local cnt=probe.read_u8(rec+0x13C) or 0
        local ids,lv,sh={},{},{}
        for i=0,7 do
          ids[#ids+1]=string.format("%02X",probe.read_u8(rec+0x13D+i) or 0)
          lv[#lv+1]=string.format("%02X",probe.read_u8(rec+0x161+i) or 0)
          sh[#sh+1]=string.format("%02X",probe.read_u8(rec+0x1C0+i) or 0)
        end
        f:write(string.format("char%d count=%d\n  ids   = %s\n  levels= %s\n  shiny = %s\n",
          n,cnt,table.concat(ids," "),table.concat(lv," "),table.concat(sh," ")))
      end
      f:close(); ctx.request_quit=true
    end
  end,
})
