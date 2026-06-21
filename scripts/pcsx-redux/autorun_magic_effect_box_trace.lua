-- autorun_magic_effect_box_trace.lua
--
-- Slot 8 has the native "Magic effect: Target DMG down 5%" box on screen,
-- overlapping our +35% banner. Identify that boxed-announcement widget: at the
-- FUN_80031d00 per-widget dispatch (jr v0 @0x800321AC), dump each drawn widget's
-- type (+0x1c), style (+0x12), position fields, lifetime-ish fields, string ptr
-- (+0x18) and the ASCII at it. The row whose text starts "Magic effect" is the
-- box we want to clone for "+35% DMG!" at a lower Y.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local DISPATCH = 0x800321AC
local OUT = probe.out_path("magic_effect_box_trace.txt")
local f = assert(io.open(OUT, "w"))
local seen = {}

local function str_at(p)
  if not p or p < 0x80000000 or p >= 0x80200000 then return "(nul)", "" end
  local hex, asc = {}, {}
  for i = 0, 31 do
    local b = probe.read_u8(p + i) or 0
    hex[#hex+1] = string.format("%02X", b)
    asc[#asc+1] = (b >= 0x20 and b < 0x7F) and string.char(b) or "."
    if b == 0 and i > 0 then break end
  end
  return table.concat(asc), table.concat(hex, " ")
end

probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate8"),
  capture_frames = probe.getenv_num("LEGAIA_FRAMES", 8),
  on_arm = function()
    probe.arm_breakpoint(DISPATCH, "Exec", 4, "disp", function()
      local r = PCSX.getRegisters()
      local s4 = (tonumber(r.GPR.n.s4) or 0) % 0x100000000
      if s4 < 0x80000000 or s4 >= 0x80200000 then return end
      local key = string.format("%08X", s4)
      if seen[key] then return end
      seen[key] = true
      local ty  = probe.read_u8(s4 + 0x1c) or 0
      local sty = probe.read_u16(s4 + 0x12) or 0
      local sp  = probe.read_u32(s4 + 0x18) or 0
      local fields = {}
      for o = 0x06, 0x1e, 2 do
        fields[#fields+1] = string.format("%02X=%04X", o, probe.read_u16(s4 + o) or 0)
      end
      local asc, hx = str_at(sp)
      f:write(string.format("s4=0x%08X ty=0x%02X sty=0x%04X str=0x%08X | %s\n    txt=[%s]\n",
        s4, ty, sty, sp, table.concat(fields, " "), asc))
      f:flush()
    end)
    return {}
  end,
  on_capture = function(ctx, el) if el >= 5 then ctx.request_quit = true end end,
  on_summary = function() f:write("done\n"); f:close() end,
})
