-- autorun_arena_liveness.lua
-- "Zero is not dead" check for the shiny-Seru arenas. Dump the raw bytes at each
-- arena VA. If an arena holds game data (not our code/bitmap/string and not all
-- zero) at runtime, it is a LIVE region we are clobbering -> the bug. Pass the
-- save state via LEGAIA_SSTATE; works on any state (clean or patched).
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local f = assert(io.open(probe.out_path("arena_liveness.txt"), "w"))
local function w(s) f:write(s.."\n"); f:flush() end
local u8 = function(a) return probe.read_u8(a) or 0xFF end
local function row(name, va, n)
  local t = {}
  for i = 0, n - 1 do t[#t+1] = string.format("%02X", u8(va + i)) end
  w(string.format("%-14s %08X: %s", name, va, table.concat(t, " ")))
end
probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate2"),
  capture_frames = 8,
  on_arm = function() return {} end,
  on_capture = function(ctx, el)
    if el == 5 then
      row("ARENA3(F)",   0x8007075C, 0x30)
      row("ARENA4(H)",   0x80079340, 0x20)
      row("CAST_FLAG",   0x80079360, 0x04)
      row("ARENA5(bm)",  0x80079509, 0x3B)
      row("ARENA1(D..)", 0x8007AE00, 0x40)
      row("ARENA2(J)",   0x8007AFF8, 0x48)
      row("GAP1(B,C1)",  0x8007772C, 0x40)
      w("done"); f:close(); ctx.request_quit = true
    end
  end,
})
