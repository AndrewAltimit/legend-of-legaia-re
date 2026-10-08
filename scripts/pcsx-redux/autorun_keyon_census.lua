-- autorun_keyon_census.lua
--
-- Counts retail KEY-ONS per emulated vsync straight off the CPU, with no SPU
-- state involved.
--
-- Why: the per-vsync SPU capture (`autorun_audio_trace.lua`) can only see a
-- key-on as a voice's envelope rising from zero. That edge is measured on
-- PCSX-Redux's SPU thread, which runs on the host's audio clock rather than
-- the emulated one (docs/subsystems/audio.md, "The envelope channel is not on
-- emulated time"): a short note can attack and drain between two captures and
-- never be seen, and a note re-keyed while the voice still rings is no edge at
-- all. A key-on itself is a call the score makes from the game's own vsync
-- handler, so counting the call is exact on both sides.
--
-- Hook: `FUN_8006B854(mode, mask24)` (libspu `SpuSetKey`) - the per-frame flush
-- `FUN_80065BAC` calls it with `a0 = 1` and the staged key-on accumulator
-- (`_DAT_801CDB48/4A`) as `a1` at `0x80065F74`, and with `a0 = 0` / the
-- key-off accumulator at `0x80065F54`. One row per call.
--
-- Output CSV: vsync,mode,mask,ra,records
--   vsync    post-load vsync index (the probe's elapsed counter)
--   mode     1 = KON, 0 = KOFF
--   mask     24-bit voice mask (hex)
--   ra       caller
--   records  for KON rows: `voice:tone_page/tone/vab_slot/owner` per keyed
--            voice, read off the libsnd note record at 0x801CDB50 + v*0x36
--
-- Run (no save-state dumps per frame, so the emulator stays near real time):
--   LEGAIA_FRAMES=4000 bash scripts/pcsx-redux/run_probe.sh \
--       --scenario s3_rimelm_freeroam \
--       --lua scripts/pcsx-redux/autorun_keyon_census.lua --out <path>.csv

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local ffi   = require("ffi")

ffi.cdef [[
typedef long __kc_time_t;
struct __kc_timespec { __kc_time_t tv_sec; long tv_nsec; };
int clock_gettime(int clk_id, struct __kc_timespec *tp);
]]
local ts = ffi.new("struct __kc_timespec[1]")
local function now_ms()
    ffi.C.clock_gettime(1, ts)
    return tonumber(ts[0].tv_sec) * 1000 + tonumber(ts[0].tv_nsec) / 1e6
end

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES     = probe.getenv_num("LEGAIA_FRAMES", 600)
local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
local OUT_PATH   = probe.out_path("keyon_census.csv")

local SPU_SET_KEY = 0x8006B854
local NOTE_REC    = 0x801CDB50
local NOTE_STRIDE = 0x36

local function n32(v) return (tonumber(v) or 0) % 0x100000000 end

local csv = probe.csv_open(OUT_PATH, "vsync,mode,mask,ra,records")
local vsync = -1
local hits = 0

local function records(mask)
    local parts = {}
    for v = 0, 23 do
        if bit.band(mask, bit.lshift(1, v)) ~= 0 then
            local base = NOTE_REC + v * NOTE_STRIDE
            parts[#parts + 1] = string.format("%d:%d/%d/%d/%04X", v,
                probe.read_u16(base + 0x12), probe.read_u16(base + 0x16),
                probe.read_u16(base + 0x18), probe.read_u16(base + 0x10))
        end
    end
    return table.concat(parts, " ")
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    boot_delay     = BOOT_DELAY,
    snapshot_every = 1000000,

    on_arm = function()
        probe.arm_breakpoint(SPU_SET_KEY, "Exec", 4, "spu_set_key", function()
            local r = PCSX.getRegisters()
            local mode = n32(r.GPR.n.a0)
            local mask = bit.band(n32(r.GPR.n.a1), 0xFFFFFF)
            if mask == 0 then return end
            hits = hits + 1
            local rec = (mode == 1) and records(mask) or ""
            if csv then
                pcall(function()
                    csv:row("%d,%d,%06X,%08X,%s", vsync, mode, mask,
                        n32(r.GPR.n.ra), rec)
                end)
            end
        end)
        -- Voice allocator verdict: at 0x80066C84 `s0` holds the winning
        -- voice, or the voice count `_DAT_801CE344` when every voice
        -- outranks the request and the note is DROPPED (FUN_80066B00).
        probe.arm_breakpoint(0x80066C84, "Exec", 4, "voice_alloc", function()
            local r = PCSX.getRegisters()
            local win = bit.band(n32(r.GPR.n.s0), 0xFF)
            local vmax = probe.read_u8(0x801CE344)
            local prior = probe.read_u8(0x801CE357)
            if csv then
                pcall(function()
                    csv:row("%d,alloc,%02X,%08X,win=%d vmax=%d prior=%d%s",
                        vsync, win, n32(r.GPR.n.ra), win, vmax, prior,
                        (win >= vmax) and " DROP" or "")
                end)
            end
        end)
        PCSX.log(string.format("[keyon] armed SpuSetKey hook for %d vsyncs -> %s",
            FRAMES, OUT_PATH))
        return {}
    end,

    on_capture = function(ctx, elapsed)
        vsync = elapsed
        -- Wall-clock pace, every 60 vsyncs: the libsnd allocator reads each
        -- voice's envelope back off the SPU, and PCSX-Redux runs that
        -- envelope on the host's audio clock, so a run far below real time
        -- allocates (and drops) differently from the hardware.
        if elapsed % 60 == 0 and csv then
            pcall(function()
                csv:row("%d,clock,0,0,ms=%.1f", elapsed, now_ms())
            end)
        end
        if elapsed >= FRAMES then ctx.request_quit = true end
    end,

    on_done = function()
        PCSX.log(string.format("[keyon] %d non-empty SpuSetKey call(s)", hits))
    end,
})
