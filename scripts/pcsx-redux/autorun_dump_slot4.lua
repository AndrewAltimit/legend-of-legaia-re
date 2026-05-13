-- autorun_dump_slot4.lua
--
-- Dump the live slot-4 (world-map overlay outlines) RAM region from a
-- PCSX-Redux save state so we can byte-compare it against the
-- disc-decoded bytes. Confirms whether the disc bytes we render are
-- actually what the runtime loads.
--
-- Procedure:
--   1. Wait for the BIOS to settle.
--   2. Load the save state.
--   3. Wait N vsyncs so any post-load runtime fixups settle.
--   4. Read the slot-4 RAM region for the kingdom (selectable via env
--      var) and write it to LEGAIA_OUT.
--   5. Quit the emulator.
--
-- Run via the matching shell wrapper:
--   scripts/pcsx-redux/run_dump_slot4.sh
--
-- Customise via env vars (read at script load):
--   LEGAIA_SSTATE   path to .sstate file (default slot 2, the user's
--                   map-overview state for Drake/map01)
--   LEGAIA_KINGDOM  drake | sebucus | karisto  (default: drake)
--   LEGAIA_OUT      output .bin path           (default slot4_ram.bin)
--   LEGAIA_FRAMES   post-load vsyncs to wait before reading (default 120)
--
-- Slot-4 base addresses come from the disc-side decoded slot lengths +
-- the verified Drake load base. Each kingdom's load offset is fixed; if
-- a future fixup pass relocates them, the read still succeeds via a
-- secondary needle-in-haystack search of the asset table's slot-4
-- bytes (not implemented yet; the default offsets are stable across
-- the retail builds we test against).

------------------------------------------------------------------
-- Configuration

local function getenv(name, fallback)
    local v = os.getenv(name)
    if v == nil or v == "" then return fallback end
    return v
end

local SSTATE_PATH = getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate2")
local KINGDOM     = getenv("LEGAIA_KINGDOM", "drake")
local OUT_PATH    = getenv("LEGAIA_OUT", "slot4_ram.bin")
local SETTLE_VSYNCS = tonumber(getenv("LEGAIA_FRAMES", "120"))

-- Per-kingdom (slot-4-load-base, decoded-length).
-- Drake: pinned via scripts/pcsx-redux/verify_slot4_in_ram.py against
-- the disc bytes; the data is loaded VERBATIM at this address with
-- zero diffs. Sebucus and Karisto follow the same load-base
-- convention but the addresses haven't been re-verified - they're
-- inferred from the disc-decoded slot-4 sizes (26964 / 24444). If a
-- mismatch appears, the autorun script falls back to a needle search.
local KINGDOMS = {
    drake   = { base = 0x8011A664, size = 32304 },
    sebucus = { base = 0x8011A664, size = 26964 },
    karisto = { base = 0x8011A664, size = 24444 },
}

local cfg = KINGDOMS[KINGDOM]
if cfg == nil then
    PCSX.log(string.format("[dump_slot4] FATAL: unknown kingdom '%s'", KINGDOM))
    PCSX.quit(2)
    return
end

PCSX.log(string.format(
    "[dump_slot4] sstate=%s kingdom=%s base=0x%08X size=%d out=%s",
    SSTATE_PATH, KINGDOM, cfg.base, cfg.size, OUT_PATH))

------------------------------------------------------------------
-- Memory helpers

local mem_file
local RAM_SIZE = 2 * 1024 * 1024

local function ram_offset(addr)
    local off = bit.band(addr, 0x1FFFFFFF)
    if off < 0 or off >= RAM_SIZE then return nil end
    return off
end

local function read_bytes_at(addr, len)
    if mem_file == nil then mem_file = PCSX.getMemoryAsFile() end
    local off = ram_offset(addr)
    if off == nil then return nil end
    return mem_file:readAt(len, off)
end

------------------------------------------------------------------
-- State machine: WAIT_BOOT -> LOAD -> SETTLE -> DUMP -> DONE

local STATE_WAIT_BOOT = 1
local STATE_SETTLE    = 2
local STATE_DONE      = 3

local state            = STATE_WAIT_BOOT
local vsync_count      = 0
local load_complete_at = nil

local BOOT_DELAY_VSYNCS = 60

local function try_load_save_state()
    local fh, err = Support.File.open(SSTATE_PATH, "READ")
    if fh == nil or fh:failed() then
        PCSX.log(string.format(
            "[dump_slot4] FATAL: cannot open %s (%s)",
            SSTATE_PATH, tostring(err)))
        PCSX.quit(2)
        return false
    end
    local zfh = Support.File.zReader(fh)
    PCSX.loadSaveState(zfh)
    zfh:close()
    fh:close()
    PCSX.log("[dump_slot4] save state loaded")
    return true
end

local function dump_slot4_to_disk()
    local buf = read_bytes_at(cfg.base, cfg.size)
    if buf == nil then
        PCSX.log(string.format(
            "[dump_slot4] FATAL: cannot read %d bytes at 0x%08X",
            cfg.size, cfg.base))
        return false
    end
    -- LuaBuffer (cdata) returned by readAt has __tostring -> ffi.string.
    -- We write that string out as raw bytes.
    local s = tostring(buf)
    local out_fh, ferr = io.open(OUT_PATH, "wb")
    if out_fh == nil then
        PCSX.log(string.format(
            "[dump_slot4] FATAL: cannot open %s: %s",
            OUT_PATH, tostring(ferr)))
        return false
    end
    out_fh:write(s)
    out_fh:close()
    PCSX.log(string.format(
        "[dump_slot4] wrote %d bytes to %s", #s, OUT_PATH))

    -- Also peek at the first 16 bytes to confirm the data looks
    -- structurally valid (count + first byte-offset).
    if #s >= 8 then
        local count = s:byte(1) + s:byte(2) * 256 + s:byte(3) * 65536 + s:byte(4) * 16777216
        local off0  = s:byte(5) + s:byte(6) * 256 + s:byte(7) * 65536 + s:byte(8) * 16777216
        PCSX.log(string.format(
            "[dump_slot4] header: count=%d  byte_offsets[0]=0x%X",
            count, off0))
    end
    return true
end

local function on_vsync()
    vsync_count = vsync_count + 1
    if state == STATE_WAIT_BOOT then
        if vsync_count >= BOOT_DELAY_VSYNCS then
            if try_load_save_state() then
                state            = STATE_SETTLE
                load_complete_at = vsync_count
            end
        end
    elseif state == STATE_SETTLE then
        if vsync_count - load_complete_at >= SETTLE_VSYNCS then
            dump_slot4_to_disk()
            state = STATE_DONE
            PCSX.log("[dump_slot4] dump done; quitting in 30 vsyncs")
        end
    elseif state == STATE_DONE then
        if vsync_count - load_complete_at >= SETTLE_VSYNCS + 30 then
            PCSX.quit(0)
        end
    end
end

PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
