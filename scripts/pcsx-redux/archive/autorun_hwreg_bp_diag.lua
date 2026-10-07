-- autorun_hwreg_bp_diag.lua -- do Lua write-BPs fire on GPU/DMA hardware
-- registers at all? Counts raw fires on GP0 (0x1F801810), DMA2 MADR/CHCR,
-- for 60 vsyncs from a field state. Diagnostic for the F-variant hunt.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 60)

local counts = { gp0 = 0, madr = 0, chcr = 0, gp0_k1 = 0 }

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function()
        probe.arm_breakpoint(0x1F801810, "Write", 4, "gp0",
            function() counts.gp0 = counts.gp0 + 1 end)
        probe.arm_breakpoint(0xBF801810, "Write", 4, "gp0_k1",
            function() counts.gp0_k1 = counts.gp0_k1 + 1 end)
        probe.arm_breakpoint(0x1F8010A0, "Write", 4, "madr",
            function() counts.madr = counts.madr + 1 end)
        probe.arm_breakpoint(0x1F8010A8, "Write", 4, "chcr",
            function() counts.chcr = counts.chcr + 1 end)
        return {}
    end,

    on_done = function()
        PCSX.log(string.format(
            "=== hwreg_bp_diag: gp0=%d gp0_k1=%d madr=%d chcr=%d ===",
            counts.gp0, counts.gp0_k1, counts.madr, counts.chcr))
    end,
})
