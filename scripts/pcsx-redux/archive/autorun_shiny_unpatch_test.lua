-- autorun_shiny_unpatch_test.lua
-- On the frozen battle state, restore every shiny hook site to its ORIGINAL two
-- instructions (un-patch in RAM) and watch the battle SM mode counter. If the SM
-- starts advancing after un-patching, a live shiny hook was sustaining the
-- freeze; if it stays frozen, the corruption is baked (need a pre-item save).
-- No exec breakpoints -> fast.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local f = assert(io.open(probe.out_path("unpatch_test.txt"), "w"))
local function w(s) f:write(s.."\n"); f:flush() end
local u16 = function(a) return probe.read_u16(a) or 0xFFFF end
local u32 = function(a) return probe.read_u32(a) or 0 end

-- {va, word0, word1} original (clean) instructions per hook site.
local ORIG = {
  {0x80051A20, 0x3C028008, 0x3C038008}, -- B
  {0x800321D4, 0x8E840018, 0x24020007}, -- J
  {0x8004AD0C, 0x92220226, 0x00000000}, -- K
  {0x801EE2E8, 0xA0820269, 0x8E630000}, -- C1
  {0x801E93B4, 0xA0430729, 0xAC4005D0}, -- C2
  {0x801E9320, 0x90460704, 0x00000000}, -- K2
  {0x801DDB08, 0x90420729, 0x8CC30000}, -- D
  {0x801D1B00, 0x90430729, 0x92220002}, -- H
}
local function wr32(a, v) probe.write_u16(a, v % 0x10000); probe.write_u16(a + 2, math.floor(v / 0x10000) % 0x10000) end

local function mode()
  local c = u32(0x8007BD24)
  if c >= 0x80000000 and c < 0x80200000 then return c, u16(c + 0x28A) end
  return c, 0xFFFF
end

local samples = {}
local unpatched = false
probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate2"),
  capture_frames = probe.getenv_num("LEGAIA_FRAMES", 70),
  on_arm = function() return {} end,
  on_capture = function(ctx, el)
    if el == 2 or el == 8 then local c, m = mode(); samples[#samples+1] = string.format("pre  f%-3d modeCtr=%04X", el, m) end
    if el == 10 and not unpatched then
      for _, s in ipairs(ORIG) do wr32(s[1], s[2]); wr32(s[1] + 4, s[3]) end
      unpatched = true
      w("UNPATCHED all 8 shiny hook sites at f10")
    end
    if el == 20 or el == 40 or el == 65 then local c, m = mode(); samples[#samples+1] = string.format("post f%-3d modeCtr=%04X", el, m) end
    if el == 66 then
      for _, s in ipairs(samples) do w(s) end
      w("done"); f:close(); ctx.request_quit = true
    end
  end,
})
