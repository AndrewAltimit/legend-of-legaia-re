-- probe/bp.lua  -- breakpoint helper.
--
-- Thin wrapper around PCSX.addBreakpoint that namespaces the label,
-- protects the callback in a pcall (a thrown error from inside a
-- breakpoint callback silently disables the probe), and registers the
-- bp into a module-level list so disarm() can clean up at the end of
-- a capture run.
--
-- The module-level list is per-Lua-state, so multiple probes
-- requiring this submodule will share the same _bps list; that's
-- intentional - disarm() at the end of a probe run cleans up
-- everything, including bps added through the umbrella probe module.
--
-- Usage:
--   local bp = require("probe.bp")
--   bp.arm(0x80017EC8, "Exec", 4, "world_map_tick", function() ... end)
--   ...
--   bp.disarm()  -- at end of capture

local M = {}

local _bps = {}

function M.arm(addr, kind, width, label, cb)
    local bp = PCSX.addBreakpoint(addr, kind, width, "probe:" .. label,
        function(...)
            local ok, err = pcall(cb, ...)
            if not ok then
                PCSX.log(string.format(
                    "[probe] callback error in %s: %s", label, tostring(err)))
            end
        end)
    _bps[#_bps + 1] = bp
    return bp
end

function M.disarm()
    for _, bp in ipairs(_bps) do
        pcall(function() bp:remove() end)
    end
    _bps = {}
end

return M
