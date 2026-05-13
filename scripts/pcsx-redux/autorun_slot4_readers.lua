-- autorun_slot4_readers.lua
--
-- Closed-loop probe for "what code reads kingdom slot-4 records".
--
-- The slot-4 container is solved (15 sub-bodies for Drake, byte-verified
-- against live RAM at 0x8011A624..0x80122454), but the record
-- interpretation is open. Static sweep across captured overlays has come
-- up empty - the consumer reads slot 4 through a runtime-loaded pointer,
-- not a LUI+ADDIU immediate-address pair, so the dump_funcs.py hunters
-- miss it.
--
-- This script sets Read breakpoints at strategic offsets across the
-- slot-4 region and logs the PC of every reader. The top PCs are the
-- consumer functions; cross-referencing against overlay_world_map_top
-- dumps identifies which function each PC sits inside, and walking one
-- body's records through that function pins the record semantics.
--
-- Probe offsets (relative to 0x8011A624) target structurally interesting
-- positions:
--   * 0x0040 - body 0's records start (skipping the 64-byte outer
--     header + 16-entry word-offset table for 15 sub-bodies)
--   * 0x0118 - body 0 mid-region (record 14 of the dense first body)
--   * 0x0188 - body 1's records start
--   * 0x0420 - body 4's records start (a kind=4 sub-body)
--   * 0x18A4 - body 12's records start (densest body, 1200+ records)
--   * 0x37CC - body 13's records start (kind=4 again)
--   * additional spread probes deeper in the range for body 14
--
-- Output CSV columns:
--   probe_idx, addr, pc, width, value, ra
--
-- Capture during the dev-menu top-view (world-map overlay loaded) +
-- separately during a kingdom-bundle scene-load transition - if the
-- dev-menu doesn't read slot 4 (perhaps only the scene-load path does),
-- the steady-state capture will be empty.
--
-- Run via the same wrapper, just override the script path:
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_slot4_readers.lua \
--     LEGAIA_OUT=slot4_readers.csv \
--     LEGAIA_FRAMES=300 \
--     ./scripts/pcsx-redux/run_world_map_probe.sh

------------------------------------------------------------------
-- Configuration

local function getenv(name, fallback)
    local v = os.getenv(name)
    if v == nil or v == "" then return fallback end
    return v
end

local SSTATE_PATH = getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate2")
local FRAMES      = tonumber(getenv("LEGAIA_FRAMES", "300"))
local OUT_PATH    = getenv("LEGAIA_OUT",  "slot4_readers.csv")
-- Optional pad button to force-hold post-load. Default 0 (none).
local HOLD_BUTTON = tonumber(getenv("LEGAIA_HOLD_BUTTON", "0"))
local HOLD_FRAMES = tonumber(getenv("LEGAIA_HOLD", "0"))

-- Slot-4 base for Drake / map01. Same RAM address on Sebucus / Karisto
-- per the dump_slot4 cached bases (kingdom loader writes to the same RAM
-- slot for all three).
local SLOT4_BASE = tonumber(getenv("LEGAIA_SLOT4_BASE", "0x8011A624"))

-- Offsets relative to SLOT4_BASE. Each probe arms one Read breakpoint.
-- Capping at ~14 probes keeps per-frame fire rate manageable when the
-- dev-menu render walks the slot every frame.
local PROBE_OFFSETS = {
    0x00000,   -- outer count word
    0x00004,   -- body 0 word_offset
    0x00040,   -- body 0 records start
    0x00118,   -- body 0 record 14 (mid-body)
    0x00188,   -- body 1 records start
    0x00420,   -- body 4 records start (kind=4)
    0x00800,   -- body 4 mid
    0x010C8,   -- body 9 region
    0x018A4,   -- body 12 records start (dense, ~1200 records)
    0x02000,   -- body 12 mid
    0x02800,   -- body 12 later
    0x037CC,   -- body 13 records start (kind=4)
    0x05400,   -- body 14 region
    0x07000,   -- near end
}
local MAX_HITS_PER_PROBE = 200

PCSX.log(string.format(
    "[s4r] sstate=%s frames=%d out=%s slot4_base=0x%08X probes=%d",
    SSTATE_PATH, FRAMES, OUT_PATH, SLOT4_BASE, #PROBE_OFFSETS))

------------------------------------------------------------------
-- CSV setup

local csv_fh, csv_err = io.open(OUT_PATH, "w")
if csv_fh then
    csv_fh:write("probe_idx,addr,pc,width,value,ra\n")
    csv_fh:flush()
else
    PCSX.log("[s4r] FATAL: cannot open " .. OUT_PATH .. ": " ..
        tostring(csv_err))
end

------------------------------------------------------------------
-- Probe state

local hits = {}
local bps  = {}
local PROBE_ADDRS = {}
for i, off in ipairs(PROBE_OFFSETS) do
    PROBE_ADDRS[i] = SLOT4_BASE + off
    hits[i] = 0
end

------------------------------------------------------------------
-- Memory helpers

local mem_file
local RAM_SIZE = 2 * 1024 * 1024

local function read_u32_safe(addr)
    if mem_file == nil then mem_file = PCSX.getMemoryAsFile() end
    local off = bit.band(addr, 0x1FFFFFFF)
    if off < 0 or off >= RAM_SIZE - 4 then return nil end
    local ok, v = pcall(function() return mem_file:readU32At(off) end)
    if not ok then return nil end
    return tonumber(v)
end

------------------------------------------------------------------
-- Arm + disarm

local function arm_probes()
    for i, addr in ipairs(PROBE_ADDRS) do
        local idx = i
        local watched_addr = addr
        local cb = function(_, _, _)
            hits[idx] = hits[idx] + 1
            if hits[idx] > MAX_HITS_PER_PROBE then return end
            local ok, info = pcall(function()
                local r = PCSX.getRegisters()
                local pc = tonumber(r.pc) or 0
                local ra = tonumber(r.GPR.n.ra) or 0
                -- For a Read probe, the value being read is whatever
                -- is sitting at the watched address right now.
                local val = read_u32_safe(watched_addr) or 0
                return { pc = pc, ra = ra, val = val }
            end)
            if not ok then return end
            if csv_fh then
                csv_fh:write(string.format(
                    "%d,0x%08X,0x%08X,4,0x%08X,0x%08X\n",
                    idx - 1, watched_addr, info.pc, info.val, info.ra))
                csv_fh:flush()
            end
            if hits[idx] <= 3 then
                PCSX.log(string.format(
                    "[s4r] probe %d (0x%08X) hit %d: pc=0x%08X val=0x%08X ra=0x%08X",
                    idx - 1, watched_addr, hits[idx], info.pc, info.val, info.ra))
            end
            if hits[idx] == MAX_HITS_PER_PROBE then
                PCSX.log(string.format(
                    "[s4r] probe %d cap reached at %d hits; further hits silently counted",
                    idx - 1, MAX_HITS_PER_PROBE))
            end
        end
        local bp = PCSX.addBreakpoint(
            addr, "Read", 4, "s4r:" .. string.format("0x%08X", addr), cb)
        bps[#bps + 1] = bp
    end
    PCSX.log(string.format("[s4r] %d Read probes armed across slot 4", #PROBE_ADDRS))
end

local function disarm_probes()
    for _, bp in ipairs(bps) do bp:remove() end
    bps = {}
end

------------------------------------------------------------------
-- Output

local function dump_summary()
    PCSX.log("=== slot-4 readers hit counts ===")
    for i, addr in ipairs(PROBE_ADDRS) do
        PCSX.log(string.format(
            "  probe %2d  0x%08X  hits=%d%s",
            i - 1, addr, hits[i],
            hits[i] > MAX_HITS_PER_PROBE and " (capped)" or ""))
    end
    PCSX.log("=== end ===")
    if csv_fh then csv_fh:flush(); csv_fh:close(); csv_fh = nil end
end

------------------------------------------------------------------
-- State machine: WAIT_BOOT -> ARMED_LOADED -> DONE
--
-- Arm probes BEFORE loading the save state so that build-once reads
-- fired in the first few frames after a state-load also get captured.

local STATE_WAIT_BOOT    = 1
local STATE_ARMED_LOADED = 2
local STATE_DONE         = 3

local state         = STATE_WAIT_BOOT
local vsync_count   = 0
local capture_start = nil

local BOOT_DELAY_VSYNCS = 60
local CAPTURE_VSYNCS    = FRAMES

local function try_load_save_state()
    local fh = Support.File.open(SSTATE_PATH, "READ")
    if fh == nil or fh:failed() then
        PCSX.log("[s4r] FATAL: cannot open save state " .. SSTATE_PATH)
        PCSX.quit(2)
        return false
    end
    local zfh = Support.File.zReader(fh)
    PCSX.loadSaveState(zfh)
    zfh:close()
    fh:close()
    PCSX.log("[s4r] save state loaded")
    return true
end

-- Pad helpers.
local pad_held = false
local function pad_force(button)
    pcall(function() PCSX.SIO0.slots[1].pads[1].setOverride(button) end)
end
local function pad_release(button)
    pcall(function() PCSX.SIO0.slots[1].pads[1].clearOverride(button) end)
end

-- Live snapshot every 60 vsync.
local HITS_PATH = OUT_PATH:gsub("%.csv$", ".hits.txt")
local function write_live_hits(label)
    local f = io.open(HITS_PATH, "w")
    if not f then return end
    f:write(string.format("# %s  vsync=%d  capture_start=%s\n",
        label, vsync_count, tostring(capture_start)))
    for i, addr in ipairs(PROBE_ADDRS) do
        f:write(string.format("  probe %2d  0x%08X  hits=%d%s\n",
            i - 1, addr, hits[i],
            hits[i] > MAX_HITS_PER_PROBE and " (capped)" or ""))
    end
    f:close()
end
local SNAPSHOT_EVERY = 60

local function on_vsync()
    vsync_count = vsync_count + 1
    if vsync_count % SNAPSHOT_EVERY == 0 then
        write_live_hits("live")
    end

    if state == STATE_WAIT_BOOT then
        if vsync_count >= BOOT_DELAY_VSYNCS then
            arm_probes()
            if try_load_save_state() then
                state         = STATE_ARMED_LOADED
                capture_start = vsync_count
                PCSX.log("[s4r] probes armed before load; capture started")
                if HOLD_BUTTON ~= 0 and HOLD_FRAMES > 0 then
                    pad_force(HOLD_BUTTON)
                    pad_held = true
                    PCSX.log(string.format(
                        "[s4r] forcing button 0x%X held for %d vsyncs",
                        HOLD_BUTTON, HOLD_FRAMES))
                end
            end
        end
    elseif state == STATE_ARMED_LOADED then
        if pad_held and vsync_count - capture_start >= HOLD_FRAMES then
            pad_release(HOLD_BUTTON)
            pad_held = false
            PCSX.log(string.format(
                "[s4r] released button 0x%X at vsync %d",
                HOLD_BUTTON, vsync_count - capture_start))
        end
        if vsync_count - capture_start >= CAPTURE_VSYNCS then
            if pad_held then pad_release(HOLD_BUTTON); pad_held = false end
            disarm_probes()
            write_live_hits("final")
            dump_summary()
            state = STATE_DONE
            PCSX.log("[s4r] capture done; quitting in 30 vsyncs")
        end
    elseif state == STATE_DONE then
        if vsync_count - capture_start >= CAPTURE_VSYNCS + 30 then
            PCSX.quit(0)
        end
    end
end

local vsync_listener = PCSX.Events.createEventListener("GPU::Vsync", on_vsync)

PCSX.log("[s4r] vsync listener installed; waiting for boot")
