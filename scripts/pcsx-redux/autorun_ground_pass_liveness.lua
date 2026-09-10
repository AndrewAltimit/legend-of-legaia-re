-- autorun_ground_pass_liveness.lua
--
-- Which of PROT 0900's two per-cell ground emitters runs, and on which
-- frames? Counts raw exec hits on both gates and both packet-commit sites
-- plus their shared caller, logging the running totals every 30 vsyncs.
-- The diagnostic behind "the ground pass hit on the field-init -> field-run
-- transition frame and never again".
--
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 600)

local SITES = {
    { name = "caller_801F79A0", addr = 0x801F79A0 },
    { name = "gate_801F69EC",   addr = 0x801F6AB4 },
    { name = "emit_801F69EC",   addr = 0x801F6CE0 },
    { name = "gate_801F6D48",   addr = 0x801F6E10 },
    { name = "emit_801F6D48",   addr = 0x801F7020 },
}
local counts = {}
local prev = {}

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function()
        for _, s in ipairs(SITES) do
            counts[s.name] = 0
            prev[s.name] = 0
            probe.bp.arm(s.addr, "Exec", 4, s.name, function()
                counts[s.name] = counts[s.name] + 1
            end)
        end
        return {}
    end,

    on_capture = function(_, elapsed)
        if elapsed % 30 ~= 0 then return end
        local parts = {}
        for _, s in ipairs(SITES) do
            parts[#parts + 1] = string.format("%s=%d(+%d)", s.name,
                counts[s.name], counts[s.name] - prev[s.name])
            prev[s.name] = counts[s.name]
        end
        PCSX.log(string.format("[liveness] vsync=%d mode=0x%02X sel=0x%08X %s",
            elapsed, probe.read_u8(0x8007B83C) or 0,
            probe.read_u32(0x8007BB4C) or 0, table.concat(parts, " ")))
    end,
})
