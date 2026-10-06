-- autorun_zero45_test.lua
-- Decisive test: arena4 (0x80079340) + arena5 (0x80079509) sit in live SsAPI
-- sound/effect-table space (0x80079xxx) read by game code (0x8005C0C8 /
-- 0x8005D0B8). Restoring them to clean (zeros) should make the item-use sound/
-- effect behave like the clean game and the Healing Leaf complete. The routines
-- there (H) + the bitmap/string don't execute/aren't needed during an item heal,
-- so zeroing them is safe (pure data -> no I-cache issue). If the freeze goes
-- away, arena4/arena5 clobbering live tables is the bug.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local f = assert(io.open(probe.out_path("zero45_test.txt"), "w"))
local function w(s) f:write(s.."\n"); f:flush() end
local u16 = function(a) return probe.read_u16(a) or 0xFFFF end
local u32 = function(a) return probe.read_u32(a) or 0 end
local function mode()
  local c = u32(0x8007BD24)
  if c >= 0x80000000 and c < 0x80200000 then return c, u16(c + 0x28A) end
  return c, 0xFFFF
end
local function vahn_hp()
  local c = u32(0x8007BD24)
  if c >= 0x80000000 and c < 0x80200000 then return u16(0x800EC9E8 + 0x14C) end
  return 0xFFFF
end
local zeroed = false
probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate1"),
  capture_frames = probe.getenv_num("LEGAIA_FRAMES", 460),
  on_arm = function() return {} end,
  on_capture = function(ctx, el)
    if el == 2 and not zeroed then
      for a = 0x80079340, 0x80079378 - 2, 2 do probe.write_u16(a, 0) end -- arena4 (H + flag)
      for a = 0x80079509, 0x80079545 - 1, 1 do probe.write_u8(a, 0) end  -- arena5 (bitmap+string)
      zeroed = true
      w("zeroed arena4 (0x80079340..78) + arena5 (0x80079509..45) to clean")
    end
    if el == 4 or el == 24 or el == 44 or el == 120 or el == 200 or el == 300 then probe.pad_force(probe.BTN.CROSS) end
    if el == 8 or el == 28 or el == 48 or el == 124 or el == 204 or el == 304 then probe.pad_release(probe.BTN.CROSS) end
    if el % 40 == 0 then local c, m = mode(); w(string.format("f%-3d modeCtr=%04X vahnHP=%04X", el, m, vahn_hp())) end
    if el == 455 then
      local ss = PCSX.GPU.takeScreenShot()
      local fh = io.open(probe.out_path("zero45_test.raw"), "wb"); fh:write(tostring(ss.data)); fh:close()
      local mh = io.open(probe.out_path("zero45_test.raw.meta"), "w"); mh:write("width=320\nheight=228\nbpp=16\n"); mh:close()
      w("done"); f:close(); ctx.request_quit = true
    end
  end,
})
