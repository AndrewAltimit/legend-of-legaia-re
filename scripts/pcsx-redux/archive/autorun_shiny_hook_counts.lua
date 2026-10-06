-- autorun_shiny_hook_counts.lua
-- On a (frozen) battle state, count executions of every shiny-Seru hook site to
-- see which shiny code is actually running. Also samples the battle SM mode
-- counter to confirm whether the SM is advancing. Reveals which detour is
-- implicated in the post-Healing-Leaf idle freeze.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local f = assert(io.open(probe.out_path("shiny_hook_counts.txt"), "w"))
local function w(s) f:write(s.."\n"); f:flush() end

local SITES = {
  {"B",  0x80051A20}, {"J",  0x800321D4}, {"K",  0x8004AD0C},
  {"C1", 0x801EE2E8}, {"C2", 0x801E93B4}, {"K2", 0x801E9320},
  {"D",  0x801DDB08}, {"H",  0x801D1B00},
}
local counts = {}
for _, s in ipairs(SITES) do counts[s[1]] = 0 end

local u16 = function(a) return probe.read_u16(a) or 0xFFFF end
local u32 = function(a) return probe.read_u32(a) or 0 end
local function mode()
  local c = u32(0x8007BD24)
  if c >= 0x80000000 and c < 0x80200000 then return c, u16(c + 0x28A) end
  return c, 0xFFFF
end

probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate2"),
  capture_frames = probe.getenv_num("LEGAIA_FRAMES", 30),
  on_arm = function()
    for _, s in ipairs(SITES) do
      local name = s[1]
      probe.arm_breakpoint(s[2], "Exec", 4, "hk_"..name, function()
        counts[name] = counts[name] + 1
      end)
    end
    return {}
  end,
  on_capture = function(ctx, el)
    if el == 3 then local c, m = mode(); w(string.format("f3   ctx=%08X modeCtr=%04X", c, m)) end
    if el == 24 then
      local c, m = mode(); w(string.format("f24 ctx=%08X modeCtr=%04X", c, m))
      w("-- hook execution counts over ~125 frames --")
      for _, s in ipairs(SITES) do w(string.format("  %-3s @ %08X : %d", s[1], s[2], counts[s[1]])) end
      w("done"); f:close(); ctx.request_quit = true
    end
  end,
})
