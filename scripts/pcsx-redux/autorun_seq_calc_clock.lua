-- autorun_seq_calc_clock.lua
--
-- Is the sequencer clocked once a vsync, and does every key-on reach the KON
-- register inside the calc that staged it? Logs each entry of the libsnd calc
-- FUN_80062F98 (the root-counter callback) and each KON write
-- FUN_8006B854(1, mask), with the CPU cycle and the probe's vsync count. A
-- skipped or doubled calc shows as a cycle gap of 0 or 2 vsyncs; a KON parked
-- for a later flush shows as a KON row whose ra is not the flush (0x80065F7C)
-- or that falls in a later vsync than its calc.
--
-- Output CSV: kind,frame,cycle,mask,ra
--   calc  FUN_80062F98 entered
--   kon   KON write, 24-bit voice mask
--
-- Run:
--   bash scripts/pcsx-redux/run_probe.sh --headless --scenario rugi_field_card_boot \
--       --lua scripts/pcsx-redux/autorun_seq_calc_clock.lua --frames 1200 --out-dir <dir>
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local SSTATE = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1200)
local csv = probe.csv_open(probe.out_path("seq_calc_clock.csv"), "kind,frame,cycle,mask,ra")
local frame = 0
local function n32(v) return (tonumber(v) or 0) % 0x100000000 end
local function cycles() return tonumber(PCSX.getCPUCycles()) end
probe.run({
    sstate = SSTATE, capture_frames = FRAMES, boot_delay = 60, snapshot_every = 1000000,
    on_arm = function()
        probe.arm_breakpoint(0x80062F98, "Exec", 4, "calc", function()
            pcall(function() csv:row("calc,%d,%.0f,0,%08X", frame, cycles(), n32(PCSX.getRegisters().GPR.n.ra)) end)
        end)
        probe.arm_breakpoint(0x8006B854, "Exec", 4, "kon", function()
            local r = PCSX.getRegisters()
            local mask = bit.band(n32(r.GPR.n.a1), 0xFFFFFF)
            if n32(r.GPR.n.a0) == 1 and mask ~= 0 then
                pcall(function() csv:row("kon,%d,%.0f,%06X,%08X", frame, cycles(), mask, n32(r.GPR.n.ra)) end)
            end
        end)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        frame = elapsed
        if elapsed >= FRAMES then ctx.request_quit = true end
    end,
})
