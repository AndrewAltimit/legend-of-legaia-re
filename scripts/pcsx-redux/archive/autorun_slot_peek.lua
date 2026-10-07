-- autorun_slot_peek.lua: screenshot + basic battle state for whatever sstate is
-- loaded (to identify a usable pre-item battle save). Set LEGAIA_SSTATE.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local stem = probe.getenv("LEGAIA_STEM", "slot_peek")
local f = assert(io.open(probe.out_path(stem..".txt"), "w"))
local function w(s) f:write(s.."\n"); f:flush() end
local u16 = function(a) return probe.read_u16(a) or 0xFFFF end
local u32 = function(a) return probe.read_u32(a) or 0 end
probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate1"),
  capture_frames = probe.getenv_num("LEGAIA_FRAMES", 12),
  on_arm = function() return {} end,
  on_capture = function(ctx, el)
    if el == 8 then
      local gm = u32(0x8007078C)
      local c = u32(0x8007BD24)
      local mode = (c>=0x80000000 and c<0x80200000) and u16(c+0x28A) or 0xFFFF
      w(string.format("%s: battle_ctx=%08X modeCtr=%04X", stem, c, mode))
      for i=0,3 do
        local a=0x800EC9E8 + i*0x2D4
        w(string.format("  actor[%d] HP=%04X act=%02X", i, u16(a+0x14C), probe.read_u8(a+0x1df) or 0))
      end
      local ss=PCSX.GPU.takeScreenShot()
      local fh=io.open(probe.out_path(stem..".raw"),"wb"); fh:write(tostring(ss.data)); fh:close()
      w("done"); f:close(); ctx.request_quit=true
    end
  end,
})
