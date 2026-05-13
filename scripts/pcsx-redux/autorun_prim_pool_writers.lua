-- autorun_prim_pool_writers.lua
--
-- Closed-loop probe for "what code writes the GPU prim pool".
--
-- The memory note (project_world_map_top_view_findings) puts the world-map
-- continent prim pool at ~0x800AD400 in main RAM, ~341 KB long, ~5000
-- POLY_FT4 packets. The autorun_world_map_probe data proves the draw VM
-- (FUN_801D362C) does NOT re-execute the continent-render ops during play
-- - so the pool must be populated by some other code path, either at scene
-- load or by a per-frame refresh outside the move-VM.
--
-- This script sets Write breakpoints at several offsets inside the pool
-- and logs the PC of every writer + the value being stored. The set of
-- writer PCs IS the geometry emitter we're trying to identify.
--
-- Capture strategy:
--   * Probe at pool_base + {0x08, 0x100, 0x1000, 0x10000, 0x40000}.
--     Offset 0x08 catches writes to the first prim's body (past its
--     8-byte chain header). The 4 deeper offsets sample prims across the
--     pool, so if the writer is a single loop we see one PC repeated,
--     and if there are multiple emitters (camera-relative vs static slab)
--     we see distinct PCs at different offsets.
--   * Cap hits per probe at MAX_HITS_PER_PROBE so per-frame OT cleanup
--     loops can't drown the log.
--   * Stream rows to CSV immediately so early exit preserves data.
--
-- Output CSV columns:
--   probe_idx  : which probe fired (0..N-1)
--   addr       : the watched address (pool_base + offset)
--   pc         : PC of the writing instruction
--   width      : write width (1/2/4 bytes)
--   ra         : ra register (return address - useful for callsite ID)
--
-- Run via the same wrapper, just override the script path:
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_prim_pool_writers.lua \
--     LEGAIA_OUT=prim_pool_writers.csv \
--     ./scripts/pcsx-redux/run_world_map_probe.sh

------------------------------------------------------------------
-- Configuration

local function getenv(name, fallback)
    local v = os.getenv(name)
    if v == nil or v == "" then return fallback end
    return v
end

local SSTATE_PATH = getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = tonumber(getenv("LEGAIA_FRAMES", "120"))
local OUT_PATH    = getenv("LEGAIA_OUT",  "prim_pool_writers.csv")
-- LEGAIA_HOLD_UP: same semantics as autorun_world_map_probe.lua. When
-- nonzero, force D-pad UP held for that many vsyncs after state-load
-- so a town-state character walks into a world-map exit and triggers
-- a scene transition while the probes are armed.
local HOLD_UP_FRAMES = tonumber(getenv("LEGAIA_HOLD_UP", "0"))
local BTN_UP = 4

-- 0x800AD400 is the GPU prim-pool base pinned by
-- `legaia_mednafen::prim_pool::decode()` (memory:
-- project_world_map_top_view_findings). LuaJIT's tonumber parses "0x"
-- hex literals fine.
local POOL_BASE   = tonumber(getenv("LEGAIA_POOL_BASE", "0x800AD400"))
-- During a town->world-map transition the terrain prims (~150 KB of
-- POLY_FT4 packets) get written into Buffer A and/or Buffer B inside
-- the 0x800AD400..0x800EFFFF pool. Buffer A starts at +0x2C00 (offset
-- 0x800B0000 in absolute), Buffer B at +0x22C00 (0x800D0000). Probe
-- both buffers densely so wherever the terrain lands we catch it.
local PROBE_OFFSETS = {
    0x00008,    -- OT head
    0x00100,    -- OT head
    0x01000,    -- OT head deep
    0x02D00,    -- Buffer A + 0x100
    0x05000,    -- Buffer A early
    0x0A000,    -- Buffer A mid
    0x10000,    -- Buffer A late
    0x18000,    -- Buffer A near end
    0x22D00,    -- Buffer B + 0x100
    0x28000,    -- Buffer B mid
    0x30000,    -- Buffer B mid
    0x38000,    -- Buffer B late
    0x40000,    -- Buffer B very late
}
local MAX_HITS_PER_PROBE = 50

PCSX.log(string.format("[ppw] sstate=%s frames=%d out=%s pool_base=0x%08X",
    SSTATE_PATH, FRAMES, OUT_PATH, POOL_BASE))

------------------------------------------------------------------
-- CSV setup

local csv_fh, csv_err = io.open(OUT_PATH, "w")
if csv_fh then
    csv_fh:write("probe_idx,addr,pc,width,value,ra\n")
    csv_fh:flush()
else
    PCSX.log("[ppw] FATAL: cannot open " .. OUT_PATH .. ": " ..
        tostring(csv_err))
end

------------------------------------------------------------------
-- Probe state

local hits = {}            -- probe_idx -> count
local bps  = {}            -- list of bp objects
local PROBE_ADDRS = {}     -- probe_idx -> watched addr
for i, off in ipairs(PROBE_OFFSETS) do
    PROBE_ADDRS[i] = POOL_BASE + off
    hits[i] = 0
end

------------------------------------------------------------------
-- Memory + register helpers

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
                -- The value being written is in one of the GPR slots
                -- depending on the store instruction. We can't tell which
                -- one without disassembling the store - so dump the post-
                -- write memory at the watched address, which IS the value
                -- that just landed there.
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
                    "[ppw] probe %d (0x%08X) hit %d: pc=0x%08X val=0x%08X ra=0x%08X",
                    idx - 1, watched_addr, hits[idx], info.pc, info.val, info.ra))
            end
            if hits[idx] == MAX_HITS_PER_PROBE then
                PCSX.log(string.format(
                    "[ppw] probe %d cap reached at %d hits; further hits silently counted",
                    idx - 1, MAX_HITS_PER_PROBE))
            end
        end
        local bp = PCSX.addBreakpoint(
            addr, "Write", 4, "ppw:" .. string.format("0x%08X", addr), cb)
        bps[#bps + 1] = bp
    end
    PCSX.log(string.format("[ppw] %d Write probes armed across pool", #PROBE_ADDRS))
end

local function disarm_probes()
    for _, bp in ipairs(bps) do bp:remove() end
    bps = {}
end

------------------------------------------------------------------
-- Output

local function dump_summary()
    PCSX.log("=== prim-pool writer hit counts ===")
    for i, addr in ipairs(PROBE_ADDRS) do
        PCSX.log(string.format(
            "  probe %d  0x%08X  hits=%d%s",
            i - 1, addr, hits[i],
            hits[i] > MAX_HITS_PER_PROBE and " (capped)" or ""))
    end
    PCSX.log("=== end ===")
    if csv_fh then csv_fh:flush(); csv_fh:close(); csv_fh = nil end
end

------------------------------------------------------------------
-- State machine: WAIT_BOOT -> ARM_THEN_LOAD -> CAPTURE -> DONE
--
-- Arm probes BEFORE loading the save state so we don't miss build-once
-- writes that fire in the first few frames after a transition-frame load.

local STATE_WAIT_BOOT     = 1
local STATE_ARMED_LOADED  = 2
local STATE_DONE          = 3

local state           = STATE_WAIT_BOOT
local vsync_count     = 0
local capture_start   = nil

local BOOT_DELAY_VSYNCS    = 60
local CAPTURE_VSYNCS       = FRAMES

local function try_load_save_state()
    local fh = Support.File.open(SSTATE_PATH, "READ")
    if fh == nil or fh:failed() then
        PCSX.log("[ppw] FATAL: cannot open save state " .. SSTATE_PATH)
        PCSX.quit(2)
        return false
    end
    local zfh = Support.File.zReader(fh)
    PCSX.loadSaveState(zfh)
    zfh:close()
    fh:close()
    PCSX.log("[ppw] save state loaded")
    return true
end

-- Pad helpers (same as autorun_world_map_probe.lua).
local pad_held = false
local function pad_force(button)
    pcall(function() PCSX.SIO0.slots[1].pads[1].setOverride(button) end)
end
local function pad_release(button)
    pcall(function() PCSX.SIO0.slots[1].pads[1].clearOverride(button) end)
end

-- Live snapshot, same idea as the world_map probe: rewritten every 60
-- vsync so we have hit counts even if pcsx-redux exits early.
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
                PCSX.log("[ppw] probes armed before load; capture started")
                if HOLD_UP_FRAMES > 0 then
                    pad_force(BTN_UP)
                    pad_held = true
                    PCSX.log(string.format(
                        "[ppw] forcing D-pad UP held for %d vsyncs",
                        HOLD_UP_FRAMES))
                end
            end
        end
    elseif state == STATE_ARMED_LOADED then
        if pad_held and vsync_count - capture_start >= HOLD_UP_FRAMES then
            pad_release(BTN_UP)
            pad_held = false
            PCSX.log(string.format(
                "[ppw] released D-pad UP at vsync %d",
                vsync_count - capture_start))
        end
        if vsync_count - capture_start >= CAPTURE_VSYNCS then
            if pad_held then pad_release(BTN_UP); pad_held = false end
            disarm_probes()
            write_live_hits("final")
            dump_summary()
            state = STATE_DONE
            PCSX.log("[ppw] capture done; quitting in 30 vsyncs")
        end
    elseif state == STATE_DONE then
        if vsync_count - capture_start >= CAPTURE_VSYNCS + 30 then
            PCSX.quit(0)
        end
    end
end

local vsync_listener = PCSX.Events.createEventListener("GPU::Vsync", on_vsync)

PCSX.log("[ppw] vsync listener installed; waiting for boot")
