-- autorun_arena2_test.lua
-- Hypothesis: arena2 (0x8007AFF8) is the zero pad after the live SsAPI sound
-- tables and is read as data during the item-use sound, so our J routine bytes
-- there corrupt the sound -> the sound-synced item banner never dismisses ->
-- freeze. Test: zero arena2 back to clean AND neutralize J's detour via an exec
-- BP (replicate J-transparent without executing arena2; no code edit -> no
-- I-cache issue), then drive the Healing Leaf and see if it completes.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local f = assert(io.open(probe.out_path("arena2_test.txt"), "w"))
local function w(s) f:write(s.."\n"); f:flush() end
local u16 = function(a) return probe.read_u16(a) or 0xFFFF end
local u32 = function(a) return probe.read_u32(a) or 0 end
local function mode()
  local c = u32(0x8007BD24)
  if c >= 0x80000000 and c < 0x80200000 then return c, u16(c + 0x28A) end
  return c, 0xFFFF
end
local zeroed = false
probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate1"),
  capture_frames = probe.getenv_num("LEGAIA_FRAMES", 320),
  on_arm = function()
    -- Neutralize J: when 0x800321D4 (its detour) is reached, do J-transparent
    -- (a0 = [s4+0x18], v0 = 7) and skip straight to hook+8, never touching arena2.
    probe.arm_breakpoint(0x800321D4, "Exec", 4, "J_neutralize", function()
      local r = PCSX.getRegisters()
      local s4 = (tonumber(r.GPR.n.s4) or 0) % 0x100000000
      r.GPR.n.a0 = u32(s4 + 0x18)
      r.GPR.n.v0 = 7
      r.pc = 0x800321DC
    end)
    return {}
  end,
  on_capture = function(ctx, el)
    if el == 2 and not zeroed then
      for a = 0x8007AFF8, 0x8007B040 - 2, 2 do probe.write_u16(a, 0) end
      zeroed = true
      w("zeroed arena2 0x8007AFF8..0x8007B040; J neutralized via BP")
    end
    if el == 4 or el == 24 or el == 44 or el == 120 or el == 200 then probe.pad_force(probe.BTN.CROSS) end
    if el == 8 or el == 28 or el == 48 or el == 124 or el == 204 then probe.pad_release(probe.BTN.CROSS) end
    if el % 40 == 0 then local c, m = mode(); w(string.format("f%-3d modeCtr=%04X", el, m)) end
    if el == 315 then
      local ss = PCSX.GPU.takeScreenShot()
      local fh = io.open(probe.out_path("arena2_test.raw"), "wb"); fh:write(tostring(ss.data)); fh:close()
      local mh = io.open(probe.out_path("arena2_test.raw.meta"), "w"); mh:write("width=320\nheight=228\nbpp=16\n"); mh:close()
      w("done"); f:close(); ctx.request_quit = true
    end
  end,
})
