-- autorun_dump_full_ram.lua
--
-- Dump the full 2 MiB main RAM from a PCSX-Redux save state to disk.
-- Companion to autorun_dump_slot4.lua: the slot-4-only variant requires
-- a known load base, but several captured save states keep the kingdom
-- data at addresses that drift between revisions / builds. Dumping the
-- whole RAM lets us run the slot-4 signature search (count = 15,
-- byte_offsets[0] = 64, body-0 marker = 0x080C) in post.
--
-- Env vars (read at script load):
--   LEGAIA_SSTATE   path to .sstate file
--                   default: ~/Tools/pcsx-redux/SCUS94254.sstate2
--   LEGAIA_OUT      output .bin path
--                   default: ram_full.bin (in CWD)
--   LEGAIA_FRAMES   post-load vsyncs to wait before reading (default 120)

local function getenv(name, fallback)
    local v = os.getenv(name)
    if v == nil or v == "" then return fallback end
    return v
end

local SSTATE_PATH = getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate2")
local OUT_PATH    = getenv("LEGAIA_OUT", "ram_full.bin")
local SETTLE_VSYNCS = tonumber(getenv("LEGAIA_FRAMES", "120"))

local RAM_SIZE = 2 * 1024 * 1024

PCSX.log(string.format(
    "[dump_ram] sstate=%s out=%s size=%d",
    SSTATE_PATH, OUT_PATH, RAM_SIZE))

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
            "[dump_ram] FATAL: cannot open %s (%s)",
            SSTATE_PATH, tostring(err)))
        PCSX.quit(2)
        return false
    end
    local zfh = Support.File.zReader(fh)
    PCSX.loadSaveState(zfh)
    zfh:close()
    fh:close()
    PCSX.log("[dump_ram] save state loaded")
    return true
end

local function dump_full_ram()
    local mem_file = PCSX.getMemoryAsFile()
    local buf = mem_file:readAt(RAM_SIZE, 0)
    if buf == nil then
        PCSX.log("[dump_ram] FATAL: cannot read main RAM")
        return false
    end
    local s = tostring(buf)
    local out_fh, ferr = io.open(OUT_PATH, "wb")
    if out_fh == nil then
        PCSX.log(string.format(
            "[dump_ram] FATAL: cannot open %s: %s",
            OUT_PATH, tostring(ferr)))
        return false
    end
    out_fh:write(s)
    out_fh:close()
    PCSX.log(string.format("[dump_ram] wrote %d bytes to %s", #s, OUT_PATH))
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
            dump_full_ram()
            state = STATE_DONE
            PCSX.log("[dump_ram] dump done; quitting in 30 vsyncs")
        end
    elseif state == STATE_DONE then
        if vsync_count - load_complete_at >= SETTLE_VSYNCS + 30 then
            PCSX.quit(0)
        end
    end
end

PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
