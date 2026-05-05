-- PCSX-Redux Lua: dump the runtime overlay code region 0x801C0000-0x801EFFFF
-- to a timestamped file under /tmp/. Run from `Lua -> Run Lua File` in the
-- PCSX-Redux GUI, or paste into the Lua console.
--
-- The overlay window holds code that is NOT present in SCUS_942.54: per-mode
-- handlers, the script VM, and various subsystem modules loaded at runtime.
-- Static scanning of every extracted file failed to find this code (see
-- BACKLOG 4.3.1 / project_overlay_hunt memory). Dynamic capture is the only
-- known path.
--
-- Usage:
--   1. Boot the game in PCSX-Redux past the title screen (so overlays are
--      definitely loaded -- they're typically present from boot but better
--      to be safe).
--   2. Run this script. It dumps 0x801C0000..0x801EFFFF (192 KB) to a file
--      under /tmp/.
--   3. The console prints the output path. Copy that file into the Ghidra
--      container and import per docs/REVERSING.md.

local BASE       = 0x801C0000   -- KSEG0 virtual address
local END        = 0x801F0000   -- exclusive
local RAM_OFFSET = 0x001C0000   -- physical RAM offset (BASE & 0x1FFFFFFF)
local LENGTH     = END - BASE   -- 0x30000 = 192 KB

-- Optional second dump of the static SCUS_942.54 RAM window for sanity
-- check. Keep small enough not to be slow; main code section is 0x80010000..
-- 0x80080000 (~448 KB).
local DUMP_SCUS  = false        -- flip to true if you want the static dump too
local SCUS_BASE  = 0x80010000
local SCUS_LEN   = 0x80080000 - SCUS_BASE

local function timestamp()
    -- Lua's os.date isn't always available in PCSX sandbox; fall back to a
    -- millisecond counter if it isn't.
    if os and os.date then
        return os.date("%Y%m%d-%H%M%S")
    end
    return tostring(math.floor(os.time and os.time() or 0))
end

local function dump_range(out_path, ram_offset, length, label)
    local mem = PCSX.getMemPtr()
    if not mem then
        print("[overlay-dump] PCSX.getMemPtr() returned nil; emulation not running?")
        return false
    end
    local f = Support.File.open(out_path, "TRUNCATE")
    if not f then
        print("[overlay-dump] failed to open output file: " .. out_path)
        return false
    end
    -- Use writeMoveSlice for speed: PCSX-Redux exposes the RAM as a slice that
    -- can be written to a file in one call.
    local slice
    if PCSX.getMemoryAsFile then
        -- Newer API: file-like view of RAM. Read out the chunk and write it.
        local memfile = PCSX.getMemoryAsFile()
        memfile:rSeek(ram_offset)
        local chunk = memfile:read(length)
        f:write(chunk)
    else
        -- Fallback: byte-by-byte (slower but always works).
        for i = 0, length - 1 do
            local b = mem[ram_offset + i]
            f:write(string.char(b))
        end
    end
    f:close()
    print(string.format(
        "[overlay-dump] wrote %s: %s (0x%X bytes from %s = RAM +0x%X)",
        label, out_path, length,
        string.format("0x%X", ram_offset + 0x80000000), ram_offset
    ))
    return true
end

local ts = timestamp()
local overlay_path = string.format("/tmp/legaia_overlay_%s.bin", ts)
dump_range(overlay_path, RAM_OFFSET, LENGTH, "overlay")

if DUMP_SCUS then
    local scus_path = string.format("/tmp/legaia_scus_window_%s.bin", ts)
    dump_range(scus_path, SCUS_BASE - 0x80000000, SCUS_LEN, "scus_window")
end

print("[overlay-dump] done.")
print("[overlay-dump] To analyze: copy into the Ghidra container and import")
print("[overlay-dump] with -loader BinaryLoader -loader-baseAddr 0x801C0000")
print("[overlay-dump] -processor MIPS:LE:32:default. See docs/REVERSING.md.")
