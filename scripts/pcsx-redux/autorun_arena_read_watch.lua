-- autorun_arena_read_watch.lua
-- Clean pre-item save (slot1, pure shiny-only). Set READ watchpoints on the
-- shiny arenas, then drive the Healing Leaf. A `j` into an arena is an
-- instruction FETCH (not a data Read), so any Read-watchpoint hit means some
-- game code is reading our routine/data bytes as DATA -> the "zero is not dead"
-- bug. Logs the reader PC + which arena. Confirms whether arena2 (sound-table
-- tail) or another arena is read during the item use.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local f = assert(io.open(probe.out_path("arena_read_watch.txt"), "w"))
local function w(s) f:write(s.."\n"); f:flush() end
local gframe = 0
local hits = {}      -- key "pc->arena" -> count
local nlog = 0

local WATCH = {
  {"arena1", 0x8007AE00, 0x100},
  {"arena2", 0x8007AFF8, 0x48},
  {"arena3", 0x8007075C, 0x30},
  {"arena4", 0x80079340, 0x24},
  {"arena5", 0x80079509, 0x3B},
}

probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate1"),
  capture_frames = probe.getenv_num("LEGAIA_FRAMES", 260),
  on_arm = function()
    for _, a in ipairs(WATCH) do
      local name = a[1]
      probe.arm_breakpoint(a[2], "Read", a[3], "rd_"..name, function()
        local r = PCSX.getRegisters()
        local pc = (tonumber(r.pc) or 0) % 0x100000000
        local key = string.format("%08X->%s", pc, name)
        hits[key] = (hits[key] or 0) + 1
        if nlog < 30 then w(string.format("  READ %s by pc=%08X (f=%d)", name, pc, gframe)); nlog = nlog + 1 end
      end)
    end
    return {}
  end,
  on_capture = function(ctx, el)
    gframe = el
    if el == 4 or el == 24 or el == 44 or el == 120 or el == 200 then probe.pad_force(probe.BTN.CROSS) end
    if el == 8 or el == 28 or el == 48 or el == 124 or el == 204 then probe.pad_release(probe.BTN.CROSS) end
    if el == 255 then
      w("-- unique reader pc -> arena (count) --")
      for k, v in pairs(hits) do w(string.format("  %s : %d", k, v)) end
      if nlog == 0 then w("  (no data reads of any arena during the item use)") end
      w("done"); f:close(); ctx.request_quit = true
    end
  end,
})
