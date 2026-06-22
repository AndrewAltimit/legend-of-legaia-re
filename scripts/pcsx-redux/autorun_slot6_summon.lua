package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local f = assert(io.open(probe.out_path("slot6_summon.txt"), "w"))
local function w(s) f:write(s.."\n"); f:flush() end
local gframe=0; local hits={}; local nlog=0
local WATCH = {
  {"slot6_78a87", 0x80078A87, 0x45},  -- candidate for K
  {"a1_tail",     0x8007AEE8, 0x18},  -- arena1 extension (H/F land here)
}
probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate3"),
  capture_frames = probe.getenv_num("LEGAIA_FRAMES", 200),
  on_arm = function()
    for _,a in ipairs(WATCH) do local name=a[1]
      probe.arm_breakpoint(a[2], "Read", a[3], "s_"..name, function()
        local r=PCSX.getRegisters(); local pc=(tonumber(r.pc) or 0)%0x100000000
        local key=string.format("%08X->%s",pc,name); hits[key]=(hits[key] or 0)+1
        if nlog<14 then w(string.format("  READ %s by pc=%08X f=%d",name,pc,gframe)); nlog=nlog+1 end
      end)
    end
    return {}
  end,
  on_capture = function(ctx, el)
    gframe=el
    -- tap CROSS to advance the cast / any prompts
    if el==10 or el==40 or el==80 or el==120 then probe.pad_force(probe.BTN.CROSS) end
    if el==14 or el==44 or el==84 or el==124 then probe.pad_release(probe.BTN.CROSS) end
    if el==195 then
      w("-- reader pc -> slot (count) --"); local any=false
      for k,v in pairs(hits) do w(string.format("  %s : %d",k,v)); any=true end
      if not any then w("  (no reads of either slot during the summon scenario)") end
      w("done"); f:close(); ctx.request_quit=true
    end
  end,
})
