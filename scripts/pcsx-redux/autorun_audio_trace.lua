-- autorun_audio_trace.lua
--
-- Per-vsync SPU snapshot capture for the I1b(b) audio-trace parity oracle.
--
-- PCSX-Redux's Lua API exposes `PCSX.createSaveState()`, which returns the
-- full state as a wrapped LuaSlice (protobuf-serialised PCSX-Redux state,
-- ~20 MiB at full size). The SPU sub-message lives at top-level field 6
-- (~600 KiB) and contains everything the AudioTraceFrame oracle needs:
--
--   - 512 KiB SPU RAM
--   - 512-byte raw SPU register file (incl. MainVol_L / MainVol_R / Reverb_Mode)
--   - 24 × Channel sub-messages (Chan::Data + ADSRInfo + ADSRInfoEx)
--
-- The probe walks the slice via FFI pointer arithmetic to find field 6,
-- then materialises only the ~600 KiB SPU payload as a Lua string for
-- the disk write. Materialising the full 20 MiB slice per capture
-- pressures Lua GC enough to disrupt `GPU::Vsync` event delivery (same
-- shape as the `readAt(2 MiB)` caveat in lib/probe/mem.lua).
--
-- Output format (single binary stream, processed offline by
-- extract_audio_trace_from_sstates.py):
--
--   magic                  = "LEGSPU01"            (8 bytes)
--   frame_count            = u32 LE                (filled at end)
--   repeated `frame_count` times:
--     vsync_index          = u32 LE                (absolute vsync at capture)
--     spu_section_size     = u32 LE
--     spu_section_bytes    = (raw PCSX-Redux SPU sub-message; field-6 inner)
--
-- Env vars:
--   LEGAIA_SSTATE     save state to load (must be parked mid-BGM for a
--                     useful trace; default sstate1)
--   LEGAIA_OUT        output stream file (default audio_trace.bin)
--   LEGAIA_FRAMES     captures to take (default 60)
--   LEGAIA_INTERVAL   vsyncs between captures (default 1 = every vsync)
--   LEGAIA_BOOT_DELAY vsyncs to wait before loading state (default 60)
--
-- Run:
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_audio_trace.lua \
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate7 \
--   LEGAIA_OUT=/tmp/audio_trace_sstate7.bin \
--   LEGAIA_FRAMES=60 LEGAIA_INTERVAL=1 \
--       bash scripts/pcsx-redux/run_probe.sh
--
-- Offline extraction:
--   python3 scripts/pcsx-redux/extract_audio_trace_from_sstates.py \
--       /tmp/audio_trace_sstate7.bin /tmp/audio_trace_sstate7.jsonl

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local ffi   = require("ffi")

ffi.cdef[[
    typedef struct { char opaque[64]; } LuaSlice;
    const void* getSliceData(LuaSlice* slice);
    uint64_t getSliceSize(LuaSlice* slice);
]]

local SSTATE_PATH  = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local OUT_PATH     = probe.out_path("audio_trace.bin")
local FRAMES       = probe.getenv_num("LEGAIA_FRAMES", 60)
local INTERVAL     = math.max(1, probe.getenv_num("LEGAIA_INTERVAL", 1))
local BOOT_DELAY   = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)

-- Read a protobuf varint from `ptr[off..off+size)`; returns (value, new_off)
-- or (nil, off) on truncation.
local function read_varint(ptr, off, size)
    local v = 0
    local sh = 0
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

-- Walk a top-level protobuf message in `ptr[0..size)` and return
-- (offset, length) for the first length-delimited field whose number is
-- `target`. Returns (nil, nil) if not present.
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
            if field == target then
                return off, ln
            end
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
    PCSX.log(string.format("[audio_trace] FATAL: cannot open %s", OUT_PATH))
    PCSX.quit(2)
    return
end
out_fh:write("LEGSPU01")
out_fh:write(string.char(0, 0, 0, 0))  -- placeholder for frame_count

local captured = 0

local function u32_le(v)
    return string.char(
        bit.band(v, 0xFF),
        bit.band(bit.rshift(v, 8), 0xFF),
        bit.band(bit.rshift(v, 16), 0xFF),
        bit.band(bit.rshift(v, 24), 0xFF))
end

local function capture_frame(vsync_idx)
    local wrapper = PCSX.createSaveState()
    if wrapper == nil then
        PCSX.log("[audio_trace] createSaveState returned nil")
        return false
    end
    -- The Lua wrapper has `._wrapper` pointing at the raw LuaSlice*.
    local raw   = wrapper._wrapper
    local size  = tonumber(ffi.C.getSliceSize(raw))
    local ptr_v = ffi.C.getSliceData(raw)
    local ptr   = ffi.cast("const uint8_t*", ptr_v)
    if ptr == nil or size == 0 then
        PCSX.log("[audio_trace] createSaveState returned empty slice")
        return false
    end
    local off, ln = find_field_range(ptr, size, 6)
    if off == nil then
        PCSX.log(string.format(
            "[audio_trace] vsync %d: no SPU field in %d-byte sstate",
            vsync_idx, size))
        return false
    end
    -- Materialise ~600 KiB SPU section as a Lua string and write.
    local spu = ffi.string(ptr + off, ln)
    out_fh:write(u32_le(vsync_idx))
    out_fh:write(u32_le(#spu))
    out_fh:write(spu)
    captured = captured + 1
    if captured <= 3 then
        PCSX.log(string.format(
            "[audio_trace] vsync %d: %d-byte SPU section captured (sstate=%d B)",
            vsync_idx, #spu, size))
    end
    -- Drop references and run a GC cycle so the 20 MiB sstate slice + the
    -- 600 KiB SPU string don't accumulate across the capture window.
    wrapper = nil
    spu     = nil
    collectgarbage("collect")
    return true
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES * INTERVAL + 30,
    boot_delay     = BOOT_DELAY,
    snapshot_every = 30,

    on_arm = function()
        PCSX.log(string.format(
            "[audio_trace] capturing every %d vsync(s) for %d frame(s) into %s",
            INTERVAL, FRAMES, OUT_PATH))
        return {}
    end,

    on_capture = function(ctx, elapsed)
        if captured >= FRAMES then
            ctx.request_quit = true
            return
        end
        if elapsed % INTERVAL == 0 then
            capture_frame(elapsed)
        end
    end,

    on_done = function()
        -- Backfill the frame_count header.
        out_fh:seek("set", 8)
        out_fh:write(u32_le(captured))
        out_fh:close()
        PCSX.log(string.format(
            "[audio_trace] wrote %d frame(s) to %s", captured, OUT_PATH))
    end,
})
