-- autorun_w1a_audio_clock.lua
--
-- [`autorun_audio_trace.lua`](autorun_audio_trace.lua) with a wall-clock
-- stamp per captured vsync.
--
-- What it answers: PCSX-Redux's SPU runs on its own thread
-- (`PCSX::SPU::impl::MainThread`), and that thread is paced by
-- `m_audioOut.feedStreamData` - i.e. by the audio device consuming samples
-- in REAL time - not by the emulated CPU's cycle budget. The ADSR envelope
-- (`PCSX::SPU::ADSR::mix`, one call per produced sample) therefore advances
-- in wall-clock seconds, while the capture's frame index advances in
-- emulated vsyncs. A per-vsync save-state capture writes ~19 MiB per frame,
-- so the emulator runs well below real time and each captured "frame"
-- spans several real frames of envelope motion.
--
-- The sidecar CSV this probe writes makes that testable inside one run: the
-- gap between consecutive captures varies frame to frame (GC, disk), so the
-- per-frame envelope decay can be regressed against the measured gap. If
-- the decay tracks the wall-clock gap, the envelope is on wall-clock; if it
-- is constant per captured frame, it is on emulated time.
--
-- Outputs:
--   <out>            same binary stream as autorun_audio_trace.lua
--                    ("LEGSPU01" + u32 frame_count + per-frame
--                     [u32 vsync][u32 size][SPU sub-message bytes])
--   <out>.clock.csv  vsync,monotonic_ns,delta_ns - one row per capture
--
-- Env vars: as autorun_audio_trace.lua (LEGAIA_SSTATE / LEGAIA_OUT /
-- LEGAIA_FRAMES / LEGAIA_INTERVAL / LEGAIA_BOOT_DELAY), plus
--   LEGAIA_PAD_MS   busy-wait this many milliseconds after each capture
--                   (default 0). Raising it stretches the wall-clock gap
--                   without changing the emulated one, which is the
--                   controlled version of the same test.
--
-- Run:
--   bash scripts/pcsx-redux/run_probe.sh --isolate-config \
--       --lua scripts/pcsx-redux/autorun_w1a_audio_clock.lua \
--       --scenario s3_rimelm_freeroam --frames 150 --out <path>.bin

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local ffi   = require("ffi")

ffi.cdef [[
typedef long __w1a_time_t;
struct __w1a_timespec { __w1a_time_t tv_sec; long tv_nsec; };
int clock_gettime(int clk_id, struct __w1a_timespec *tp);
]]

local CLOCK_MONOTONIC = 1
local ts = ffi.new("struct __w1a_timespec[1]")

-- Monotonic nanoseconds as two numbers (seconds, nanoseconds) so no value
-- ever exceeds Lua's exact-integer range.
local function mono()
    ffi.C.clock_gettime(CLOCK_MONOTONIC, ts)
    return tonumber(ts[0].tv_sec), tonumber(ts[0].tv_nsec)
end

local function slice_size(w)
    if type(w.size) == "number" then return w.size end
    local ok, n = pcall(function() return #w end)
    if ok and type(n) == "number" then return n end
    return 0
end

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local OUT_PATH    = probe.out_path("w1a_audio_clock.bin")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 120)
local INTERVAL    = math.max(1, probe.getenv_num("LEGAIA_INTERVAL", 1))
local BOOT_DELAY  = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
local PAD_MS      = probe.getenv_num("LEGAIA_PAD_MS", 0)

local function read_varint(ptr, off, size)
    local v, sh = 0, 0
    while off < size do
        local b = ptr[off]
        off = off + 1
        v = v + bit.band(b, 0x7F) * (2 ^ sh)
        if b < 0x80 then return v, off end
        sh = sh + 7
        if sh > 35 then return nil, off end
    end
    return nil, off
end

local function find_field_range(ptr, size, target)
    local off = 0
    while off < size do
        local tag, np = read_varint(ptr, off, size)
        if tag == nil then return nil, nil end
        off = np
        local field = math.floor(tag / 8)
        local wt    = tag % 8
        if wt == 0 then
            local _, np2 = read_varint(ptr, off, size)
            off = np2
        elseif wt == 2 then
            local ln, np2 = read_varint(ptr, off, size)
            if ln == nil then return nil, nil end
            off = np2
            if field == target then return off, ln end
            off = off + ln
        elseif wt == 5 then off = off + 4
        elseif wt == 1 then off = off + 8
        else return nil, nil
        end
    end
    return nil, nil
end

local out_fh = io.open(OUT_PATH, "wb")
if out_fh == nil then
    PCSX.log(string.format("[w1a_clock] FATAL: cannot open %s", OUT_PATH))
    PCSX.quit(2)
    return
end
out_fh:write("LEGSPU01")
out_fh:write(string.char(0, 0, 0, 0))

local csv_fh = io.open(OUT_PATH .. ".clock.csv", "w")
if csv_fh ~= nil then csv_fh:write("vsync,monotonic_ns,delta_ns\n") end

local captured, errors = 0, 0
local last_s, last_ns = nil, nil

local function u32_le(v)
    return string.char(
        bit.band(v, 0xFF),
        bit.band(bit.rshift(v, 8), 0xFF),
        bit.band(bit.rshift(v, 16), 0xFF),
        bit.band(bit.rshift(v, 24), 0xFF))
end

local function capture_frame(vsync_idx)
    local wrapper = PCSX.createSaveState()
    if wrapper == nil then return false end
    local size = tonumber(slice_size(wrapper)) or 0
    local ptr  = ffi.cast("const uint8_t*", wrapper.data)
    if ptr == nil or size == 0 then return false end
    local off, ln = find_field_range(ptr, size, 6)
    if off == nil then return false end
    local spu = ffi.string(ptr + off, ln)
    out_fh:write(u32_le(vsync_idx))
    out_fh:write(u32_le(#spu))
    out_fh:write(spu)
    captured = captured + 1

    local s, ns = mono()
    if csv_fh ~= nil then
        local d = 0
        if last_s ~= nil then d = (s - last_s) * 1000000000 + (ns - last_ns) end
        csv_fh:write(string.format("%d,%d,%d\n", vsync_idx, s * 1000000000 + ns, d))
        last_s, last_ns = s, ns
    end

    wrapper = nil
    spu = nil
    collectgarbage("collect")

    if PAD_MS > 0 then
        local ps, pns = mono()
        local target = PAD_MS * 1000000
        while true do
            local cs, cns = mono()
            if (cs - ps) * 1000000000 + (cns - pns) >= target then break end
        end
    end
    return true
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES * INTERVAL + 30,
    boot_delay     = BOOT_DELAY,
    snapshot_every = 30,

    on_arm = function()
        PCSX.log(string.format(
            "[w1a_clock] %d frame(s), interval %d, pad %d ms -> %s",
            FRAMES, INTERVAL, PAD_MS, OUT_PATH))
        return {}
    end,

    on_capture = function(ctx, elapsed)
        if captured >= FRAMES then
            ctx.request_quit = true
            return
        end
        if elapsed % INTERVAL == 0 then
            local ok, err = pcall(capture_frame, elapsed)
            if not ok then
                errors = errors + 1
                if errors <= 3 then
                    PCSX.log(string.format("[w1a_clock] vsync %d threw: %s",
                        elapsed, tostring(err)))
                end
            end
        end
    end,

    on_done = function()
        out_fh:seek("set", 8)
        out_fh:write(u32_le(captured))
        out_fh:close()
        if csv_fh ~= nil then csv_fh:close() end
        PCSX.log(string.format("[w1a_clock] wrote %d frame(s) (%d error(s))",
            captured, errors))
    end,
})
