-- autorun_field_frame_profile.lua
--
-- Where a field frame's time goes: load a state, leave the pad alone, and
-- count the CPU cycles spent inside each of a list of routines, vsync by
-- vsync, beside the frame pacer's newest frame time. An exec breakpoint on
-- each routine's entry stamps the cycle counter and arms one on the return
-- address it was called from; the return stamps the difference. Times are
-- inclusive (a routine's callees count in it). The field frame-cost model's
-- missing terms are read off it (`legaia_engine_core::world::field_frame_cost`).
--
--   bash scripts/pcsx-redux/run_probe.sh --headless \
--     --lua scripts/pcsx-redux/autorun_field_frame_profile.lua \
--     --scenario son_field_door_hop --frames 60 --out-dir <dir>
--
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES, LEGAIA_OUT_DIR,
--      LEGAIA_PROFILE_FUNCS  comma-separated hex entries (default below),
--      LEGAIA_PRESS_AT  comma-separated vsyncs Cross goes down on, held 4
--                       (a walk film's `--pad N-N+4:CROSS` presses).
-- Output: frame_profile.csv (`vsync,frame_cost,step,func,calls,cycles,irq_cycles`,
-- one row per routine per vsync it ran in). `irq_cycles` is the part of
-- `cycles` spent in the exception handler on an interrupt that landed inside
-- the call (an exec breakpoint on the vector 0x80000080 with Cause's ExcCode 0,
-- closed when the run reaches the EPC it saved): `cycles - irq_cycles` is the
-- routine's own work, the quantity crates/retail-exec's `field-frame` runner
-- reproduces. A pseudo-row `func = IRQ` carries each vsync's interrupt total
-- (`calls` = interrupts). A syscall's trip through the vector is the caller's
-- work and stays in. Interpreter mode required (exec BP); not for --fast.
-- calls.csv (`func,entry_cycle,cycles,irq_cycles`) logs every call and every
-- interrupt (`func = IRQ`) with the cycle counter at its entry, so the calls
-- can be grouped by game frame (the field handler 0x80025EEC's entries)
-- where a vsync's rows mix the end of one frame with the start of the next.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 60)
local DEFAULT = table.concat({
    "80016444", -- master frame driver
    "8002519C", -- an actor-list tick pass
    "8001D140", -- an actor-list render pass
    "8001ADA4", -- one actor's draw
    "801F7088", -- the decoration pass
    "801F6D48", -- the ground emitter
    "801F69EC", -- its _DAT_8007BB4C twin
    "801DE840", -- the field VM
    "801D6058", -- the ambient emitter
    "8003F3FC", -- the fog walk
    "8005FB84", -- libetc VSync
}, ",")
local LIST = probe.getenv("LEGAIA_PROFILE_FUNCS", DEFAULT)
local RING, RING_CURSOR = 0x80084098, 0x8007B758
local PRESS = {}
for h in string.gmatch(probe.getenv("LEGAIA_PRESS_AT", ""), "[^,]+") do
    PRESS[tonumber(h)] = true
end

local fh = assert(io.open(probe.out_path("frame_profile.csv"), "w"))
fh:write("vsync,frame_cost,step,func,calls,cycles,irq_cycles\n")

local funcs = {}
for h in string.gmatch(LIST, "[^,]+") do funcs[#funcs + 1] = tonumber(h, 16) end
local ch = assert(io.open(probe.out_path("calls.csv"), "w"))
ch:write("func,entry_cycle,cycles,irq_cycles\n")
local calls, cycles, irqc, open, ret_bp = {}, {}, {}, {}, {}
local irq_open, irq_total, irq_count, epc_bp = nil, 0, 0, {}
local irq_open_t = 0

local function now() return tonumber(PCSX.getCPUCycles()) end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_field_frame_profile.lua",
            { sstate = SSTATE, frames = FRAMES, funcs = LIST })
        probe.arm_breakpoint(0x80000080, "Exec", 4, "prof_vector", function()
            local r = PCSX.getRegisters()
            local cause = tonumber(r.CP0.r[13]) % 0x100000000
            if math.floor(cause / 4) % 32 ~= 0 or irq_open then return end
            local epc = tonumber(r.CP0.r[14]) % 0x100000000
            irq_open = { epc = epc, t = now() }
            irq_open_t = irq_open.t
            if not epc_bp[epc] then
                epc_bp[epc] = true
                probe.arm_breakpoint(epc, "Exec", 4, string.format("irq_ret_%08X", epc), function()
                    if not irq_open or irq_open.epc ~= epc then return end
                    local dt = now() - irq_open.t
                    irq_open = nil
                    irq_total, irq_count = irq_total + dt, irq_count + 1
                    ch:write(string.format("IRQ,%.0f,%.0f,%.0f\n", irq_open_t, dt, dt))
                    for _, g in ipairs(funcs) do
                        for _, c in ipairs(open[g]) do c.irq = c.irq + dt end
                    end
                end)
            end
        end)
        for _, f in ipairs(funcs) do
            calls[f], cycles[f], irqc[f], open[f] = 0, 0, 0, {}
            probe.arm_breakpoint(f, "Exec", 4, string.format("prof_%08X", f), function()
                local ra = PCSX.getRegisters().GPR.n.ra
                if ra < 0 then ra = ra + 0x100000000 end
                local st = open[f]
                st[#st + 1] = { ra = ra, t = now(), irq = 0 }
                local key = string.format("%08X:%08X", f, ra)
                if not ret_bp[key] then
                    ret_bp[key] = true
                    probe.arm_breakpoint(ra, "Exec", 4, "ret_" .. key, function()
                        local s = open[f]
                        -- Pop the newest open call made from this site.
                        for i = #s, 1, -1 do
                            if s[i].ra == ra then
                                cycles[f] = cycles[f] + (now() - s[i].t)
                                irqc[f] = irqc[f] + s[i].irq
                                calls[f] = calls[f] + 1
                                ch:write(string.format("%08X,%.0f,%.0f,%.0f\n", f, s[i].t,
                                    now() - s[i].t, s[i].irq))
                                table.remove(s, i)
                                break
                            end
                        end
                    end)
                end
            end)
        end
        return {}
    end,
    on_capture = function(_, el)
        if PRESS[el] then
            probe.pad_force(probe.BTN.CROSS)
        elseif PRESS[el - 4] then
            probe.pad_release(probe.BTN.CROSS)
        end
        local cur = probe.read_u32(RING_CURSOR) or 1
        local cost = probe.read_u16(RING + ((cur - 1) % 16) * 2) or 0
        local step = probe.read_scratch_u8(0x1F800393) or 0
        for _, f in ipairs(funcs) do
            if calls[f] > 0 then
                fh:write(string.format("%d,%d,%d,%08X,%d,%.0f,%.0f\n", el, cost, step, f, calls[f],
                    cycles[f], irqc[f]))
            end
            calls[f], cycles[f], irqc[f] = 0, 0, 0
        end
        if irq_count > 0 then
            fh:write(string.format("%d,%d,%d,IRQ,%d,%.0f,%.0f\n", el, cost, step, irq_count,
                irq_total, irq_total))
        end
        irq_total, irq_count = 0, 0
        fh:flush()
        ch:flush()
    end,
})
