-- probe/env.lua  -- environment-variable + output-path helpers.
--
-- Pulled out of the umbrella `probe` module so probes that only need
-- env-var plumbing don't drag in the PCSX runtime helpers.
--
-- Usage:
--   local env = require("probe.env")
--   local sstate = env.getenv("LEGAIA_SSTATE", "default.sstate7")
--   local out    = env.out_path("my_probe.csv")
--
-- The umbrella `probe` module re-exports these as probe.getenv /
-- probe.getenv_num / probe.out_path for backwards compat.

local M = {}

function M.getenv(name, fallback)
    local v = os.getenv(name)
    if v == nil or v == "" then return fallback end
    return v
end

function M.getenv_num(name, fallback)
    local v = os.getenv(name)
    if v == nil or v == "" then return fallback end
    return tonumber(v) or fallback
end

-- Resolve a probe's primary output path. Precedence:
--   1. $LEGAIA_OUT      - explicit user override, used as-is.
--   2. $LEGAIA_OUT_DIR  - runner-provided per-run dir; appends <default_name>.
--   3. <default_name>   - fallback: bare filename in CWD (legacy behaviour).
-- The runner script (scripts/pcsx-redux/run_probe.sh) sets LEGAIA_OUT_DIR
-- to captures/<probe-stem>/<iso-timestamp>/ when neither LEGAIA_OUT nor
-- LEGAIA_OUT_DIR is supplied, so by default every probe drops its
-- artefacts into a per-run subtree.
function M.out_path(default_name)
    local explicit = os.getenv("LEGAIA_OUT")
    if explicit ~= nil and explicit ~= "" then return explicit end
    local dir = os.getenv("LEGAIA_OUT_DIR")
    if dir ~= nil and dir ~= "" then
        if dir:sub(-1) == "/" then
            return dir .. default_name
        else
            return dir .. "/" .. default_name
        end
    end
    return default_name
end

return M
