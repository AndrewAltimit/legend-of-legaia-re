-- probe.lua  -- umbrella re-export of the lib/probe/ submodules.
--
-- This module preserves the flat `probe.X` surface that every
-- autorun_*.lua probe already consumes (probe.read_u32, probe.run,
-- probe.arm_breakpoint, ...), while internally delegating to the
-- per-concern submodules under lib/probe/. New probes are encouraged
-- to require the submodules directly:
--
--   local mem = require("probe.mem")
--   local bp  = require("probe.bp")
--   local sm  = require("probe.sm")
--
-- ...but the flat form still works:
--
--   local probe = require("probe")
--   probe.read_u32(addr)
--   probe.arm_breakpoint(...)
--   probe.run({ ... })
--
-- Submodule map:
--   probe.env       lib/probe/env.lua       getenv, getenv_num, out_path
--   probe.mem       lib/probe/mem.lua       read_u8/16/32, read_bytes, in_ram, ...
--   probe.sstate    lib/probe/sstate.lua    load_save_state
--   probe.pad       lib/probe/pad.lua       BTN, force, release
--   probe.bp        lib/probe/bp.lua        arm, disarm
--   probe.csv       lib/probe/csv.lua       open (returns Csv with :row/:close)
--   probe.snapshot  lib/probe/snapshot.lua  write, capture_call_context, append_call_context
--   probe.sm        lib/probe/sm.lua        run (the WAIT_BOOT -> ARMED -> DONE driver)
--   probe.symbols   lib/probe/symbols.lua   FUN_*/DAT_* lookup with fail-closed guard
--   probe.step      lib/probe/step.lua      trace (instruction tracer) + find_writer (range write attribution)

local env      = require("probe.env")
local mem      = require("probe.mem")
local sstate   = require("probe.sstate")
local pad      = require("probe.pad")
local bp       = require("probe.bp")
local csv      = require("probe.csv")
local snapshot = require("probe.snapshot")
local sm       = require("probe.sm")
local watch    = require("probe.watch")
local step     = require("probe.step")
-- Lazy-require for symbols: it does a filesystem lookup on first access,
-- so don't pay that cost just because a probe required the umbrella.
local _symbols_cached = nil
local function get_symbols()
    if _symbols_cached == nil then
        _symbols_cached = require("probe.symbols")
    end
    return _symbols_cached
end

local M = {}

-- Sub-table exports for new-style probes.
M.env      = env
M.mem      = mem
M.sstate   = sstate
M.pad      = pad
M.bp       = bp
M.csv      = csv
M.snapshot = snapshot
M.sm       = sm
M.watch    = watch
M.step     = step
setmetatable(M, { __index = function(t, k)
    if k == "symbols" then return get_symbols() end
    return nil
end })

------------------------------------------------------------------
-- Flat re-exports of the legacy `probe.X` surface that every
-- existing autorun_*.lua probe consumes.

-- env
M.getenv      = env.getenv
M.getenv_num  = env.getenv_num
M.out_path    = env.out_path

-- mem
M.RAM_SIZE        = mem.RAM_SIZE
M.ram_offset      = mem.ram_offset
M.in_ram          = mem.in_ram
M.read_u32        = mem.read_u32
M.read_u8         = mem.read_u8
M.read_u16        = mem.read_u16
M.read_bytes      = mem.read_bytes
M.bytes_to_hex    = mem.bytes_to_hex
M.read_scratch_u32 = mem.read_scratch_u32

-- sstate
M.load_save_state = sstate.load

-- pad
M.BTN         = pad.BTN
M.pad_force   = pad.force
M.pad_release = pad.release

-- bp
M.arm_breakpoint = bp.arm
M.disarm_all     = bp.disarm

-- csv (Csv instances expose :row and :close via the metatable)
M.csv_open = csv.open

-- snapshot
M.write_snapshot       = snapshot.write
M.capture_call_context = snapshot.capture_call_context
M.append_call_context  = snapshot.append_call_context

-- sm
M.run = sm.run

return M
