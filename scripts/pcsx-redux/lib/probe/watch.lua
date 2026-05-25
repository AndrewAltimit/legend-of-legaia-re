-- probe/watch.lua  -- write/read watchpoint logging helper.
--
-- Most "what writes this address?" probes hand-roll the same closure: arm a
-- Write breakpoint, and in the callback read the CPU registers, log
-- (elapsed, label, addr, pc, ra, new_value) to a CSV, and dump the call
-- context for the first N hits. This factors that out so a new probe is a few
-- lines instead of a ~40-line copy (see autorun_player_pos_watch.lua,
-- autorun_prim_pool_writers.lua, autorun_title_overlay_writer_hunt.lua, ...).
--
-- It composes the already-proven probe primitives (probe.bp / probe.mem /
-- probe.snapshot); it adds no new emulator interaction of its own.
--
-- Usage:
--   local watch = require("probe.watch")
--   local csv = probe.csv_open(probe.out_path("hits.csv"),
--                              "tick,label,addr,pc,ra,value")
--   local w = watch.new{
--       csv         = csv,
--       detail_path = probe.out_path("hits.detail.txt"),  -- optional
--       max_detail  = 16,
--       elapsed     = function() return g_elapsed end,     -- current frame
--   }
--   w:arm(addr, 2, "playerX")          -- width 1/2/4; kind defaults "Write"
--   ...
--   print("total hits:", w:total())

local bp       = require("probe.bp")
local mem      = require("probe.mem")
local snapshot = require("probe.snapshot")

local M = {}

local _readers = { [1] = mem.read_u8, [2] = mem.read_u16, [4] = mem.read_u32 }

local Watch = {}
Watch.__index = Watch

-- Create a watch logger. `opts.csv` is a Csv (from probe.csv_open) whose
-- header should be "tick,label,addr,pc,ra,value"; rows are written in that
-- order. `opts.elapsed` is a function returning the current frame counter
-- (defaults to a constant 0). `opts.detail_path` (optional) receives the
-- call context for the first `opts.max_detail` (default 16) hits.
function M.new(opts)
    opts = opts or {}
    assert(opts.csv, "watch.new: opts.csv is required")
    return setmetatable({
        csv         = opts.csv,
        detail_path = opts.detail_path,
        max_detail  = opts.max_detail or 16,
        elapsed     = opts.elapsed or function() return 0 end,
        hits        = 0,
    }, Watch)
end

-- Arm a watchpoint at `addr`. `width` is 1/2/4 bytes (selects the value
-- reader and the breakpoint width). `kind` defaults to "Write". Returns the
-- breakpoint handle from probe.bp.
function Watch:arm(addr, width, label, kind)
    width = width or 4
    kind = kind or "Write"
    local reader = _readers[width] or mem.read_u32
    local self_ = self
    return bp.arm(addr, kind, width, label, function()
        local r  = PCSX.getRegisters()
        local pc = bit.band(tonumber(r.pc), 0xFFFFFFFF)
        local ra = bit.band(tonumber(r.GPR.n.ra), 0xFFFFFFFF)
        local val = reader(addr) or 0
        self_.csv:row("%d,%s,0x%08X,0x%08X,0x%08X,%d",
            self_.elapsed(), label, addr, pc, ra, val)
        self_.hits = self_.hits + 1
        if self_.detail_path and self_.hits <= self_.max_detail then
            snapshot.append_call_context(self_.detail_path,
                snapshot.capture_call_context(string.format(
                    "%s write #%d addr=0x%08X elapsed=%d",
                    label, self_.hits, addr, self_.elapsed())))
        end
    end)
end

-- Total hits logged across all armed watchpoints on this logger.
function Watch:total()
    return self.hits
end

return M
