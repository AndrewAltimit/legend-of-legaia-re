-- autorun_tetsu_item_catch.lua
-- From a save taken in the Tetsu tutorial with the ITEM list open and "Healing
-- Leaf" highlighted (shiny-only ROM), inject CROSS presses to use the item and
-- catch which shiny hook fires during the item action (the freeze cause). Logs
-- each hook firing with key registers, samples the battle SM mode counter, and
-- screenshots the end state.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local f = assert(io.open(probe.out_path("tetsu_item_catch.txt"), "w"))
local function w(s) f:write(s.."\n"); f:flush() end
local u16 = function(a) return probe.read_u16(a) or 0xFFFF end
local u32 = function(a) return probe.read_u32(a) or 0 end
local gframe = 0

-- Only the WRITING hooks (drop transparent J/K spam so the interpreter runs fast
-- enough to reach the freeze).
local SITES = {
  {"B",  0x80051A20},
  {"C1", 0x801EE2E8}, {"C2", 0x801E93B4}, {"K2", 0x801E9320},
  {"D",  0x801DDB08}, {"H",  0x801D1B00},
}
local counts = {}; local nlog = {}
for _, s in ipairs(SITES) do counts[s[1]] = 0; nlog[s[1]] = 0 end

local function regs()
  local r = PCSX.getRegisters().GPR.n
  local function g(x) return (tonumber(r[x]) or 0) % 0x100000000 end
  return g
end

local function mode()
  local c = u32(0x8007BD24)
  if c >= 0x80000000 and c < 0x80200000 then return c, u16(c + 0x28A) end
  return c, 0xFFFF
end

probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate4"),
  capture_frames = probe.getenv_num("LEGAIA_FRAMES", 520),
  on_arm = function()
    for _, s in ipairs(SITES) do
      local name = s[1]
      probe.arm_breakpoint(s[2], "Exec", 4, "hk_"..name, function()
        counts[name] = counts[name] + 1
        if nlog[name] < 6 then
          local g = regs()
          w(string.format("  [%s] f=%d v0=%08X v1=%08X a0=%08X a1=%08X a2=%08X s1=%08X CAST=%02X",
            name, gframe, g("v0"), g("v1"), g("a0"), g("a1"), g("a2"), g("s1"),
            probe.read_u8(0x80079360) or 0))
          nlog[name] = nlog[name] + 1
        end
      end)
    end
    return {}
  end,
  on_capture = function(ctx, el)
    gframe = el
    -- Tap CROSS a few times to use the item + confirm target.
    if el == 4 or el == 24 or el == 44 or el == 64 or el == 120 or el == 200 then probe.pad_force(probe.BTN.CROSS) end
    if el == 8 or el == 28 or el == 48 or el == 68 or el == 124 or el == 204 then probe.pad_release(probe.BTN.CROSS) end
    if el % 40 == 0 then local c, m = mode(); w(string.format("f%-3d modeCtr=%04X CAST=%02X", el, m, probe.read_u8(0x80079360) or 0)) end
    if el == 515 then
      w("-- final hook counts --")
      for _, s in ipairs(SITES) do w(string.format("  %-3s : %d", s[1], counts[s[1]])) end
      local ss = PCSX.GPU.takeScreenShot()
      local fh = io.open(probe.out_path("tetsu_item_catch.raw"), "wb"); fh:write(tostring(ss.data)); fh:close()
      local mh = io.open(probe.out_path("tetsu_item_catch.raw.meta"), "w"); mh:write("width=320\nheight=228\nbpp=16\n"); mh:close()
      w("done"); f:close(); ctx.request_quit = true
    end
  end,
})
