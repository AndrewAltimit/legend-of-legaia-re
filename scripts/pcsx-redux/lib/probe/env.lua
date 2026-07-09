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

-- Write a run manifest (manifest.txt in the run dir) recording what this
-- capture WAS: which script, which save state it started from, every config
-- knob, and the arm-time context. Old run dirs otherwise go stale - a CSV
-- with no record of its targets/toggles can't be trusted months later, and
-- "which sstate did this run start from" is the resume/provenance chain
-- between runs. Call once at arm/baseline time (post version-guard) so the
-- context fields are live. `kv` is a flat table; keys are sorted for a
-- stable, diffable layout. Returns true on success.
function M.write_manifest(script, kv)
    local fh = io.open(M.out_path("manifest.txt"), "w")
    if fh == nil then return false end
    fh:write("script = " .. tostring(script) .. "\n")
    fh:write("written_utc = " .. os.date("!%Y-%m-%dT%H:%M:%SZ") .. "\n")
    local keys = {}
    for k in pairs(kv) do keys[#keys + 1] = k end
    table.sort(keys)
    for _, k in ipairs(keys) do
        fh:write(k .. " = " .. tostring(kv[k]) .. "\n")
    end
    fh:close()
    return true
end

return M
