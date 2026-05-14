-- symbols.lua  -- Ghidra-resolved function-name -> address map for probes.
--
-- The actual table is auto-generated at ghidra/scripts/symbols.lua
-- (committed; regenerate via scripts/pcsx-redux/build-symbols.py).
-- This wrapper provides:
--   * a default search path that picks up the ghidra/scripts/ table from
--     a probe running in repo root,
--   * a missing-symbol guard that loudly fails closed if a probe asks
--     for a symbol that's not in the table (so a typo / overlay-only
--     symbol doesn't silently arm a breakpoint at address 0).
--
-- Usage from an autorun:
--   package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
--   local symbols = require("symbols").load()  -- default path
--   probe.arm_breakpoint(symbols.FUN_801DA51C, "Exec", 4, "world_map_sm", cb)
--
-- Or pass an explicit path if the probe runs from a different cwd:
--   local symbols = require("symbols").load("/abs/path/to/symbols.lua")

local M = {}

-- Default candidate paths, tried in order. The first that loads wins.
-- The repo-root form is used when probes are launched via the
-- run_world_map_probe.sh harness from the repo root. The fallback paths
-- cover working-directory variations.
local DEFAULTS = {
    "ghidra/scripts/symbols.lua",
    "../../ghidra/scripts/symbols.lua",
}

local function try_load(path)
    local f = io.open(path, "r")
    if not f then return nil end
    f:close()
    -- dofile evaluates the table-literal `return { ... }` body.
    local ok, t = pcall(dofile, path)
    if ok and type(t) == "table" then return t end
    return nil
end

function M.load(path)
    if path then
        local t = try_load(path)
        if t then return M.wrap(t, path) end
        error("symbols: cannot load " .. path, 2)
    end
    for _, p in ipairs(DEFAULTS) do
        local t = try_load(p)
        if t then return M.wrap(t, p) end
    end
    error("symbols: no symbols.lua found in any of: "
        .. table.concat(DEFAULTS, ", "), 2)
end

-- Wrap the raw {name=addr} table in a metatable that fails loudly on
-- missing keys. A typo'd symbol name otherwise returns nil; arming a
-- breakpoint at nil silently does nothing and the probe captures zero
-- hits with no diagnostic.
function M.wrap(table_, source_path)
    return setmetatable({}, {
        __index = function(_, k)
            local v = rawget(table_, k)
            if v == nil then
                error(string.format(
                    "symbols.%s not found (loaded from %s; "
                    .. "regenerate via scripts/pcsx-redux/build-symbols.py)",
                    tostring(k), tostring(source_path)), 2)
            end
            return v
        end,
        __pairs = function() return pairs(table_) end,
        __len = function()
            local n = 0
            for _ in pairs(table_) do n = n + 1 end
            return n
        end,
    })
end

return M
