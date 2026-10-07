package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local f = assert(io.open(probe.out_path("endpad_verify.txt"), "w"))
local function w(s) f:write(s.."\n"); f:flush() end
local gframe=0; local hits={}; local nlog=0
local WATCH = {
  {"endpad",  0x8007B6BC, 0x144},   -- 0x8007B6BC..0x8007B800
  {"bigrun",  0x800797D0, 0x400},   -- first 1KB of the 4.2KB run (sample)
}
probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate1"),
  capture_frames = probe.getenv_num("LEGAIA_FRAMES", 300),
  on_arm = function()
    for _,a in ipairs(WATCH) do local name=a[1]
      probe.arm_breakpoint(a[2], "Read", a[3], "ep_"..name, function()
        local r=PCSX.getRegisters(); local pc=(tonumber(r.pc) or 0)%0x100000000
        local key=string.format("%08X->%s",pc,name); hits[key]=(hits[key] or 0)+1
        if nlog<20 then w(string.format("  READ %s by pc=%08X f=%d",name,pc,gframe)); nlog=nlog+1 end
      end)
    end
    return {}
  end,
  on_capture = function(ctx, el)
    gframe=el
    if el==4 or el==24 or el==44 or el==120 or el==200 then probe.pad_force(probe.BTN.CROSS) end
    if el==8 or el==28 or el==48 or el==124 or el==204 then probe.pad_release(probe.BTN.CROSS) end
    if el==295 then
      w("-- reader pc -> slot (count) --"); local any=false
      for k,v in pairs(hits) do w(string.format("  %s : %d",k,v)); any=true end
      if not any then w("  (no reads of either candidate -> both safe)") end
      w("done"); f:close(); ctx.request_quit=true
    end
  end,
})
