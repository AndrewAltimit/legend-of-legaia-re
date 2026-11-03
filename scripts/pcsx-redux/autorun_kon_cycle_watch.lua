-- autorun_kon_cycle_watch.lua
--
-- Where inside a vsync a sequence's key-ons reach the SPU: the CPU cycle of each
-- per-frame flush FUN_80065BAC and each KON write through FUN_8006B854(1, mask),
-- against the cycle of the last one, so the spread of the gaps says whether the
-- flush sits on a fixed offset of the vsync (a sample is 768 cycles).
--
-- Output CSV: kind,cycle,mask,ra
--   flush  FUN_80065BAC entered
--   kon    KON write, 24-bit voice mask
--
-- Run:
--   LEGAIA_FRAMES=1200 bash scripts/pcsx-redux/run_probe.sh \
--       --scenario s3_rimelm_freeroam \
--       --lua scripts/pcsx-redux/autorun_kon_cycle_watch.lua --out <path>.csv

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1200)
local csv = probe.csv_open(probe.out_path("kon_cycle_watch.csv"), "kind,cycle,mask,ra")

local function n32(v) return (tonumber(v) or 0) % 0x100000000 end
local function cycles() return tonumber(PCSX.getCPUCycles()) end

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,
    boot_delay     = 60,
    snapshot_every = 1000000,
    on_arm = function()
        probe.arm_breakpoint(0x80065BAC, "Exec", 4, "flush", function()
            if csv then
                pcall(function()
                    csv:row("flush,%.0f,0,%08X", cycles(), n32(PCSX.getRegisters().GPR.n.ra))
                end)
            end
        end)
        probe.arm_breakpoint(0x8006B854, "Exec", 4, "kon", function()
            local r = PCSX.getRegisters()
            local mask = bit.band(n32(r.GPR.n.a1), 0xFFFFFF)
            if n32(r.GPR.n.a0) == 1 and mask ~= 0 and csv then
                pcall(function()
                    csv:row("kon,%.0f,%06X,%08X", cycles(), mask, n32(r.GPR.n.ra))
                end)
            end
        end)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        if elapsed >= FRAMES then ctx.request_quit = true end
    end,
})
