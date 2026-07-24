-- autorun_gaza2_range_wedge.lua
--
-- Attribution replay for the state-0x19 attack-approach park (the endless
-- camera orbit caught LIVE by the park hunter on the Gaza 2 fight; library
-- save 814dce6b...). The parked shape: acting seat 3 (Gaza, category 3
-- physical attack) targeting seat 2 (Gala), state 0x19 re-polling the range
-- check FUN_8004E2F0 forever - the arm has no movement code and its
-- not-in-range path only bumps the stall counter ctx+0x6D4
-- (`LAB_801e35d0`), so a target out of reach parks the fight.
--
-- This probe measures WHY the check never passes:
--   * Exec bp on FUN_8004E2F0 entry - a0 (acting seat), a1 (target seat)
--   * Exec bp on its epilogue jr ra (0x8004E560) - v0 = the metric
--     (0 = in range; the 0x19 arm branches on != 0)
--   * per-frame: both actors' current (+0x34/+0x38) and home (+0x3C/+0x40)
--     positions, sizes, and ctx+0x6D4
--
-- Run (interpreter; breakpoints):
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_gaza2_range_wedge.lua \
--     --sstate saves/library/pcsx-redux/814dce6b90da114a8d8d37386a90623c4f871f7e380e14c010889ab4414c9dd8.sstate \
--     --frames 900

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 900)

local CTX_PTR = 0x8007BD24
local ACTORS  = 0x801C9370
local ENTRY   = 0x8004E2F0
local RET     = 0x8004E560

local function u8(a)  return probe.read_u8(a)  or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function i16(a) local v = u16(a); return v >= 0x8000 and v - 0x10000 or v end
local function in_ram(a) return a >= 0x80000000 and a < 0x80200000 end

local vsync = 0
local calls, rets = 0, 0
local last_args = { a0 = -1, a1 = -1 }
local v0_hist = {}

local csv = probe.csv_open(probe.out_path("range_calls.csv"),
    "vsync,a0,a1,v0,act_x,act_z,tgt_x,tgt_z,tgt_hx,tgt_hz,c6d4")

local function actor_of(seat)
    local a = u32(ACTORS + seat * 4)
    return in_ram(a) and a or 0
end

probe.run{
    sstate         = SSTATE,
    capture_frames = FRAMES,
    boot_delay     = 60,
    on_arm = function()
        probe.arm_breakpoint(ENTRY, "Exec", 4, "range_entry", function()
            local r = PCSX.getRegisters()
            last_args.a0 = tonumber(r.GPR.n.a0) % 0x100
            last_args.a1 = tonumber(r.GPR.n.a1) % 0x100
            calls = calls + 1
        end)
        probe.arm_breakpoint(RET, "Exec", 4, "range_ret", function()
            local r = PCSX.getRegisters()
            local v0 = tonumber(r.GPR.n.v0) or -1
            rets = rets + 1
            v0_hist[v0] = (v0_hist[v0] or 0) + 1
            local act = actor_of(last_args.a0)
            local tgt = actor_of(last_args.a1)
            local c = u32(CTX_PTR)
            if rets <= 40 or rets % 32 == 0 then
                csv:row("%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d",
                    vsync, last_args.a0, last_args.a1, v0,
                    act ~= 0 and i16(act + 0x34) or -1,
                    act ~= 0 and i16(act + 0x38) or -1,
                    tgt ~= 0 and i16(tgt + 0x34) or -1,
                    tgt ~= 0 and i16(tgt + 0x38) or -1,
                    tgt ~= 0 and i16(tgt + 0x3C) or -1,
                    tgt ~= 0 and i16(tgt + 0x40) or -1,
                    in_ram(c) and u16(c + 0x6D4) or -1)
            end
        end)
        return {}
    end,
    on_capture = function(_, v) vsync = v end,
    on_done = function()
        local lines = {
            string.format("=== gaza2 range-wedge replay: %d entries, %d returns over %d vsyncs ===",
                calls, rets, vsync),
            "v0 histogram (0 = in range; the 0x19 arm requires 0 to advance):",
        }
        for v0, n in pairs(v0_hist) do
            lines[#lines + 1] = string.format("  v0=%-6d x%d", v0, n)
        end
        probe.write_snapshot(probe.out_path("summary.txt"), table.concat(lines, "\n"))
        for _, l in ipairs(lines) do PCSX.log("[range] " .. l) end
    end,
    on_summary = function() end,
}
