package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local f = assert(io.open(probe.out_path("boundary.txt"), "w"))
local function w(s) f:write(s.."\n"); f:flush() end
local gframe=0; local hits={}; local nlog=0
local WATCH = {
  {"a1_tail_EE8", 0x8007AEE8, 0x18},  -- 0x8007AEE8..0x8007AF00 (24B) arena1 tail
  {"a1_AF00",     0x8007AF00, 0x40},  -- 0x8007AF00..0x8007AF40 (suspected live)
  {"cand_78a87",  0x80078A87, 0x45},  -- backup slot for C1
}
probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate1"),
  capture_frames = probe.getenv_num("LEGAIA_FRAMES", 220),
  on_arm = function()
    for _,a in ipairs(WATCH) do local name=a[1]
      probe.arm_breakpoint(a[2], "Read", a[3], "b_"..name, function()
        local r=PCSX.getRegisters(); local pc=(tonumber(r.pc) or 0)%0x100000000
        local key=string.format("%08X->%s",pc,name); hits[key]=(hits[key] or 0)+1
        if nlog<12 then w(string.format("  READ %s by pc=%08X f=%d",name,pc,gframe)); nlog=nlog+1 end
      end)
    end
    return {}
  end,
  on_capture = function(ctx, el)
    gframe=el
    if el==4 or el==24 or el==44 or el==120 then probe.pad_force(probe.BTN.CROSS) end
    if el==8 or el==28 or el==48 or el==124 then probe.pad_release(probe.BTN.CROSS) end
    if el==215 then
      w("-- reader pc -> slot (count) --"); local any=false
      for k,v in pairs(hits) do w(string.format("  %s : %d",k,v)); any=true end
      if not any then w("  (no reads of any watched slot)") end
      w("done"); f:close(); ctx.request_quit=true
    end
  end,
})
