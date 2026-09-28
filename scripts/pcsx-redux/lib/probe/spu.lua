-- probe/spu.lua  -- SPU sub-message capture from a live PCSX-Redux state.
--
-- `PCSX.createSaveState()` returns the whole serialised state; the SPU lives
-- at its top-level protobuf field 6 (register window 0x1F801C00..DFF + the 24
-- Channel sub-messages carrying the live ADSR envelope). This module finds
-- that field and hands back the raw bytes, so a probe can stream them in the
-- `LEGSPU01` layout `extract_audio_trace_from_sstates.py` decodes (per-voice
-- `env_level`, the only field that says whether a voice is sounding).
--
-- Same code path as autorun_audio_trace.lua, factored out for probes that
-- capture the SPU only at a few chosen vsyncs.
--
-- Usage:
--   local spu = require("probe.spu")
--   local w = spu.open("out.bin")      -- writes the LEGSPU01 header
--   spu.capture(w, vsync_index)        -- one frame; returns true/false, err
--   spu.close(w)                       -- backfills frame_count

local ffi = require("ffi")

local M = {}

local function slice_size(w)
    if type(w.size) == "number" then return w.size end
    local ok, n = pcall(function() return #w end)
    if ok and type(n) == "number" then return n end
    return 0
end

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
        local field, wt = math.floor(tag / 8), tag % 8
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
        else return nil, nil end
    end
    return nil, nil
end

local function u32_le(v)
    return string.char(bit.band(v, 0xFF), bit.band(bit.rshift(v, 8), 0xFF),
        bit.band(bit.rshift(v, 16), 0xFF), bit.band(bit.rshift(v, 24), 0xFF))
end

function M.open(path)
    local fh = io.open(path, "wb")
    if fh == nil then return nil end
    fh:write("LEGSPU01")
    fh:write(u32_le(0))
    return { fh = fh, n = 0 }
end

-- Capture the SPU section now and append it tagged with `vsync_index`.
function M.capture(w, vsync_index)
    local ok, err = pcall(function()
        local st = PCSX.createSaveState()
        local size = tonumber(slice_size(st)) or 0
        local ptr = ffi.cast("const uint8_t*", st.data)
        if size == 0 then error("empty state slice") end
        local off, ln = find_field_range(ptr, size, 6)
        if off == nil then error("no SPU field") end
        local s = ffi.string(ptr + off, ln)
        w.fh:write(u32_le(vsync_index))
        w.fh:write(u32_le(#s))
        w.fh:write(s)
        w.fh:flush()
        w.n = w.n + 1
        st, s = nil, nil
        collectgarbage("collect")
    end)
    return ok, err
end

function M.close(w)
    w.fh:seek("set", 8)
    w.fh:write(u32_le(w.n))
    w.fh:close()
end

return M
