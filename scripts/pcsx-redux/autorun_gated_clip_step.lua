-- autorun_gated_clip_step.lua
--
-- Measure the field clip tick FUN_800204F8's per-call cursor step on live
-- actors, split by the bound clip's blend gate (clip byte +1 bit 0) and its
-- divisor (clip byte +6). The disassembly (0x800205B4..0x800205EC) says a
-- gated clip steps (rate*2 + div - 1) / div and an ungated one steps the
-- rate (+0x6A), each times the frame-step byte DAT_1F800393; this probe
-- checks that on every call it sees.
--
-- Entry bp 0x800204F8: a0 = actor. Records the cursor +0x68, the control
-- word +0x62, the requested / bound ids +0x5C / +0x5E and the frame step.
-- Exit bp 0x80020730 (the shared epilogue, s0 = actor): records the cursor
-- after, the bound clip +0x4C, its header bytes +1 / +2 / +6 and the rate.
-- One CSV row per call whose entry and exit pair up.
--
--   timeout --kill-after=30s 300s bash scripts/pcsx-redux/run_probe.sh \
--       --isolate-config --lua scripts/pcsx-redux/autorun_gated_clip_step.lua \
--       --scenario s3_rimelm_freeroam --frames 600
--
-- Output: <out>/gated_clip_step.csv. Pure RAM observation - no Sony bytes
-- beyond header bit/divisor values and cursor integers.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 600)
local MAX_ROWS = probe.getenv_num("LEGAIA_MAX_ROWS", 20000)

local function tou32(v)
    v = tonumber(v) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end
local function u8(a) return probe.mem.read_u8(a) or -1 end
local function u16(a) return probe.mem.read_u16(a) or -1 end
local function u32(a) return probe.mem.read_u32(a) or 0 end

local out = probe.out_path("gated_clip_step.csv")
local CSV = io.open(out, "w")
CSV:write("vsync,actor,id5c,id5e,flags62,step393,cursor_in,cursor_out,clip,gate,frames,div,rate\n")

local pending = nil
local rows = 0
local vs = 0

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,
    on_arm = function()
        probe.arm_breakpoint(0x800204F8, "Exec", 4, "clip_tick_in", function()
            local n = PCSX.getRegisters().GPR.n
            local a = tou32(n.a0)
            if not probe.in_ram(a + 0x6A) then pending = nil return end
            pending = {
                actor = a,
                id5c = u16(a + 0x5C),
                id5e = u16(a + 0x5E),
                flags = u16(a + 0x62),
                step = probe.mem.read_scratch_u8(0x1F800393),
                cin = u16(a + 0x68),
            }
        end)
        probe.arm_breakpoint(0x80020730, "Exec", 4, "clip_tick_out", function()
            if pending == nil or rows >= MAX_ROWS then return end
            local n = PCSX.getRegisters().GPR.n
            local a = tou32(n.s0)
            if a ~= pending.actor then pending = nil return end
            local clip = u32(a + 0x4C)
            local gate, frames, div = -1, -1, -1
            if probe.in_ram(clip + 6) then
                gate = bit.band(u8(clip + 1), 1)
                frames = u16(clip + 2)
                div = u8(clip + 6)
            end
            CSV:write(string.format("%d,0x%08X,%d,%d,0x%04X,%d,%d,%d,0x%08X,%d,%d,%d,%d\n",
                vs, a, pending.id5c, pending.id5e, pending.flags, pending.step,
                pending.cin, u16(a + 0x68), clip, gate, frames, div, u16(a + 0x6A)))
            rows = rows + 1
            pending = nil
        end)
        return {}
    end,
    on_capture = function(ctx, e)
        vs = e
        if e % 100 == 0 then
            CSV:flush()
            PCSX.log(string.format("[gated_clip_step] vsync %d rows %d", e, rows))
        end
    end,
    on_done = function()
        CSV:close()
        PCSX.log(string.format("[gated_clip_step] done: %d rows -> %s", rows, out))
    end,
})
