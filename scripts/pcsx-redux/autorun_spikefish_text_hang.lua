-- autorun_spikefish_text_hang.lua - who calls the text renderer after a
-- Spikefish Flute escape, and with what string?
--
-- Companion of autorun_spikefish_flee_return.lua, which stops (fast core:
-- "Unrecoverable error while running recompiler", pc 0x80036868, inside
-- the substitution expander FUN_80036514 under the text renderer
-- FUN_80036888) a few seconds after the flute's escape teardown, with the
-- loader-B module already swapped from PROT 0925 to PROT 0978. Load the
-- escape_wait.rawsstate checkpoint that probe writes (gzip it first) and
-- run this under the INTERPRETER (no --fast) so the breakpoints fire:
--   * exec BP on FUN_80036888 entry: logs, once per distinct call, ra, a0 (the source string),
--     a1 / a2, the caller's caller from the stack, and 48 source bytes
--   * exec BP on FUN_80036514 entry: logs a0 (destination), a1 (source)
--   * from the first renderer call on, a [loop] line every 2000 passes of
--     the expander's copy loop at 0x800367E4 with t0 (read cursor) and a0
--     (write cursor), which shows a runaway copy
-- Plus the game mode / action state / loader-B id on change. Poll-only
-- otherwise; nothing is poked.
--
--   timeout 1500 xvfb-run -a bash scripts/pcsx-redux/run_probe.sh \
--     --iso "$LEGAIA_DISC_BIN" --sstate escape_wait.sstate \
--     --lua scripts/pcsx-redux/autorun_spikefish_text_hang.lua --frames 600

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")

local GM, LOADB, CTXP = 0x8007B83C, 0x8007BC4C, 0x8007BD24
local calls, ex_calls, loops = 0, 0, 0
local last = nil
local seen = {}

local function hexs(addr, n)
    local b = mem.read_bytes(addr, n)
    if not b then return "?" end
    local s, t = tostring(b), {}
    for i = 1, #s do t[#t + 1] = string.format("%02x", s:byte(i)) end
    return table.concat(t, " ")
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 600),
    on_arm = function()
        probe.arm_breakpoint(0x80036888, "Exec", 4, "text_render", function()
            calls = calls + 1
            local r = PCSX.getRegisters()
            local a0 = tonumber(r.GPR.n.a0)
            local sp = tonumber(r.GPR.n.sp)
            -- log each distinct (caller, string, first bytes) once
            local key = string.format("%08x/%08x/%s", tonumber(r.GPR.n.ra), a0, hexs(a0, 16))
            if seen[key] then return end
            seen[key] = true
            PCSX.log(string.format(
                "[render] #%d ra=%08x a0=%08x a1=%08x a2=%08x a3=%08x sp=%08x loadB=%d src=%s",
                calls, tonumber(r.GPR.n.ra), a0, tonumber(r.GPR.n.a1),
                tonumber(r.GPR.n.a2), tonumber(r.GPR.n.a3), sp,
                mem.read_u32(LOADB) or -1, hexs(a0, 48)))
            PCSX.log("[render] stack " .. hexs(sp, 64))
        end)
        probe.arm_breakpoint(0x80036514, "Exec", 4, "expand", function()
            ex_calls = ex_calls + 1
            if ex_calls > 12 then return end
            local r = PCSX.getRegisters()
            PCSX.log(string.format("[expand] #%d ra=%08x dst=%08x src=%08x",
                ex_calls, tonumber(r.GPR.n.ra), tonumber(r.GPR.n.a0),
                tonumber(r.GPR.n.a1)))
        end)
        probe.arm_breakpoint(0x800367E4, "Exec", 4, "copy_loop", function()
            loops = loops + 1
            if loops % 2000 == 0 and loops <= 200000 then
                local r = PCSX.getRegisters()
                PCSX.log(string.format("[loop] n=%d t0=%08x a0=%08x a1=%08x t2=%08x",
                    loops, tonumber(r.GPR.n.t0), tonumber(r.GPR.n.a0),
                    tonumber(r.GPR.n.a1), tonumber(r.GPR.n.t2)))
            end
        end)
        return {}
    end,
    on_capture = function(ctx, el)
        local gm = mem.read_u8(GM) or 0xFF
        local c = mem.read_u32(CTXP) or 0
        local st = -1
        if c >= 0x80000000 and c < 0x80200000 then st = mem.read_u8(c + 7) or 0 end
        local line = string.format("gm=%02x st=%02x loadB=%d renders=%d loops=%d",
            gm, st % 256, mem.read_u32(LOADB) or -1, calls, loops)
        if line ~= last and (el % 10 == 0 or last == nil) then
            PCSX.log(string.format("[st] f=%d %s", el, line)); last = line
        end
        if el >= probe.getenv_num("LEGAIA_FRAMES", 600) - 2 then ctx.request_quit = true end
    end,
})
