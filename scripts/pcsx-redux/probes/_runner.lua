-- _runner.lua  -- autorun shim that runs a declarative .probe.toml.
--
-- Usage:
--   LEGAIA_PROBE_SPEC=scripts/pcsx-redux/probes/<name>.probe.toml \
--     LEGAIA_LUA=scripts/pcsx-redux/probes/_runner.lua \
--     ./scripts/pcsx-redux/run_probe.sh --scenario <label>
--
-- Or, simpler, via the run_probe.sh --spec flag (which sets both env
-- vars + handles scenario resolution):
--
--   ./scripts/pcsx-redux/run_probe.sh --spec scripts/pcsx-redux/probes/<name>.probe.toml
--
-- The runner does nothing PCSX-specific itself: it parses the spec via
-- lib/probe/toml.lua, hands the table to lib/probe/spec.lua, and lets
-- the spec module call probe.sm.run().

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"

local toml = require("probe.toml")
local spec = require("probe.spec")

local SPEC_PATH = os.getenv("LEGAIA_PROBE_SPEC")
if SPEC_PATH == nil or SPEC_PATH == "" then
    PCSX.log("[_runner] FATAL: LEGAIA_PROBE_SPEC not set; pass via run_probe.sh --spec <path>")
    PCSX.quit(64)
    return
end

PCSX.log(string.format("[_runner] loading spec: %s", SPEC_PATH))

local parsed
do
    local ok, err = pcall(function()
        parsed = toml.parse_file(SPEC_PATH)
    end)
    if not ok then
        PCSX.log("[_runner] FATAL: TOML parse error: " .. tostring(err))
        PCSX.quit(65)
        return
    end
end

spec.run(parsed)
