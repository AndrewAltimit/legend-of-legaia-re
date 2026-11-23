-- autorun_field_cd_trace.lua
--
-- Where a field frame's time goes while a voice line plays: the field twin of
-- autorun_cast_sequence.lua's LEGAIA_CD_TRACE (breakpoint core). Per row, the
-- CPU cycle of: each libcd command call (FUN_8005CF80 / FUN_8005D0B8 /
-- FUN_8005CCB4: a0 the command, the first parameter byte, ra), the response
-- dispatcher FUN_8005DAB0's entry and exit (0x8005DB74), every exception at
-- 0x80000080 (a0 the cause), the frame end's VSync(n) return (0x800171C8)
-- and the CPU-done VSync(1) (0x800174CC), plus the clip player's one-shot
-- FUN_8003D53C. scripts/ci/field-cd-frames.py prices each frame's work apart
-- from its CD interrupt time.
--
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES, LEGAIA_OUT[_DIR],
--      LEGAIA_MASH_EVERY (Cross held 2 vsyncs every N vsyncs from N; 0 none).
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local every = probe.getenv_num("LEGAIA_MASH_EVERY", 20)
local csv = probe.csv_open(probe.out_path("cd.csv"), "vsync,kind,cycle,a0,a1,ra")
local vsync = 0
local function u32(x) return (tonumber(x) or 0) % 0x100000000 end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 1200),
    on_arm = function()
        local descs = {}
        local function arm(addr, name, fn)
            local d = { addr = addr, name = name, hits_ref = { n = 0 } }
            probe.arm_breakpoint(addr, "Exec", 4, name, function()
                d.hits_ref.n = d.hits_ref.n + 1
                local R = PCSX.getRegisters()
                local a0, a1 = fn(R)
                csv:row("%d,%s,%.0f,0x%08X,0x%08X,0x%08X", vsync, name,
                    tonumber(PCSX.getCPUCycles()), a0 or 0, a1 or 0, u32(R.GPR.n.ra))
            end)
            descs[#descs + 1] = d
        end
        for _, f in ipairs({ { 0x8005CF80, "ctl" }, { 0x8005D0B8, "ctlb" }, { 0x8005CCB4, "ctlf" } }) do
            arm(f[1], f[2], function(R)
                local p = u32(R.GPR.n.a1)
                local b = (p >= 0x80000000 and p < 0x80200000) and probe.read_u8(p) or 0
                return u32(R.GPR.n.a0), b
            end)
        end
        arm(0x8005DAB0, "irq", function() return 0, 0 end)
        arm(0x8005DB74, "irq_end", function() return 0, 0 end)
        arm(0x80000080, "exc", function(R)
            local ok, cause = pcall(function() return R.CP0.n.Cause end)
            return ok and u32(cause or 0) or 0, 0
        end)
        arm(0x800171C8, "vsyncret", function() return 0, 0 end)
        arm(0x800174CC, "cpudone", function() return 0, 0 end)
        arm(0x8003D53C, "oneshot", function(R) return u32(R.GPR.n.a0), u32(R.GPR.n.a2) end)
        return descs
    end,
    on_capture = function(_, elapsed)
        vsync = elapsed
        if every > 0 then
            if elapsed % every == 0 then
                probe.pad_force(probe.BTN.CROSS)
            elseif elapsed % every == 2 then
                probe.pad_release(probe.BTN.CROSS)
            end
        end
    end,
    on_done = function() csv:close() end,
})
