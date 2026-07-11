-- probe/mem.lua  -- PSX memory readers for probes.
--
-- Wraps PCSX.getMemoryAsFile() (main RAM, 2 MiB) and
-- PCSX.getScratchPtr() (1 KiB scratchpad at 0x1F800000) behind
-- KSEG-virtual-address-aware helpers. All readers return nil on
-- out-of-range / unmapped addresses, never throw.
--
-- Usage:
--   local mem = require("probe.mem")
--   if mem.in_ram(addr, 4) then
--       local w = mem.read_u32(addr)
--   end
--
-- The umbrella `probe` module re-exports the surface as probe.read_u32
-- etc. for backwards compat.

local M = {}

local RAM_SIZE = 2 * 1024 * 1024  -- 2 MiB main RAM
M.RAM_SIZE = RAM_SIZE

local mem_file = nil

local function get_mem_file()
    if mem_file == nil then mem_file = PCSX.getMemoryAsFile() end
    return mem_file
end

-- Convert a KSEG0 / KSEG1 / USEG virtual address to a main-RAM byte
-- offset. Returns nil if the address can't possibly hit main RAM
-- (scratchpad 0x1F8003xx, hardware regs 0x1F801xxx, BIOS, etc.).
function M.ram_offset(addr)
    local off = bit.band(addr, 0x1FFFFFFF)
    if off < 0 or off >= RAM_SIZE then return nil end
    return off
end

function M.in_ram(addr, width)
    local off = M.ram_offset(addr)
    if off == nil then return false end
    return off + (width or 1) <= RAM_SIZE
end

function M.read_u32(addr)
    local off = M.ram_offset(addr)
    if off == nil or off + 4 > RAM_SIZE then return nil end
    local mf = get_mem_file()
    local ok, v = pcall(function() return mf:readU32At(off) end)
    if not ok then return nil end
    return tonumber(v)
end

function M.read_u8(addr)
    local off = M.ram_offset(addr)
    if off == nil or off + 1 > RAM_SIZE then return nil end
    local mf = get_mem_file()
    local ok, v = pcall(function() return mf:readU8At(off) end)
    if not ok then return nil end
    return tonumber(v)
end

function M.read_u16(addr)
    local off = M.ram_offset(addr)
    if off == nil or off + 2 > RAM_SIZE then return nil end
    local mf = get_mem_file()
    local ok, v = pcall(function() return mf:readU16At(off) end)
    if not ok then return nil end
    return tonumber(v)
end

function M.read_bytes(addr, len)
    local off = M.ram_offset(addr)
    if off == nil or off + len > RAM_SIZE then return nil end
    local mf = get_mem_file()
    local ok, v = pcall(function() return mf:readAt(len, off) end)
    if not ok then return nil end
    return v
end

-- Convert a LuaBuffer (cdata) or Lua string into uppercase hex.
function M.bytes_to_hex(buf)
    local s = tostring(buf)
    local out = {}
    for i = 1, #s do out[i] = string.format("%02X", s:byte(i)) end
    return table.concat(out)
end

-- GameShark-style RAM pokes. `write_u8(addr, value)` is the equivalent of a
-- `30XXXXXX 00YY` code: an 8-bit store to the given KSEG virtual address.
-- Returns true on success, false if the address is out of main RAM range.
function M.write_u8(addr, value)
    local off = M.ram_offset(addr)
    if off == nil or off + 1 > RAM_SIZE then return false end
    local mf = get_mem_file()
    local ok = pcall(function() mf:writeU8At(bit.band(value, 0xFF), off) end)
    return ok
end

function M.write_u16(addr, value)
    local off = M.ram_offset(addr)
    if off == nil or off + 2 > RAM_SIZE then return false end
    local mf = get_mem_file()
    local ok = pcall(function() mf:writeU16At(bit.band(value, 0xFFFF), off) end)
    return ok
end

-- Scratchpad reader. The 1 KiB scratchpad sits at 0x1F800000 and is
-- accessible via PCSX.getScratchPtr() as a uint8_t*. Any virtual
-- address in 0x1F800000..0x1F8003FF maps to that buffer.
local scratch_u32_ptr = nil
function M.read_scratch_u32(addr)
    if scratch_u32_ptr == nil then
        scratch_u32_ptr = ffi.cast("uint32_t*", PCSX.getScratchPtr())
    end
    local off = bit.band(addr, 0x3FF)
    return tonumber(scratch_u32_ptr[off / 4]) or 0
end

-- Byte-granular scratchpad reader (the u32 form is word-aligned only; use
-- this for byte cells like the frame-step DAT_1F800393).
local scratch_u8_ptr = nil
function M.read_scratch_u8(addr)
    if scratch_u8_ptr == nil then
        scratch_u8_ptr = ffi.cast("uint8_t*", PCSX.getScratchPtr())
    end
    return tonumber(scratch_u8_ptr[bit.band(addr, 0x3FF)]) or 0
end

return M
