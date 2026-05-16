-- autorun_boot_walk_snapshots.lua
--
-- Multi-snapshot RAM-and-register probe for boot-sequence walks. One
-- emulator launch loads a save state, then takes a RAM dump + CPU
-- register snapshot at each emulator vsync in TARGETS. Drops one
-- ".bin" (2 MiB main RAM) and one ".regs" sidecar per snapshot.
--
-- Critical implementation detail (learned the hard way): doing a single
-- 2 MiB `PCSX.getMemoryAsFile():readAt(RAM_SIZE, 0)` permanently breaks
-- subsequent GPU::Vsync event delivery in PCSX-Redux's Lua harness
-- (probably triggers heavyweight Lua GC that disrupts the event loop).
-- 64 KiB reads are safe individually, but stacking 32 of them in one
-- callback also degrades vsync delivery. The fix: **one 64 KiB chunk
-- per vsync callback**, spread the 2 MiB read across 32 callbacks. The
-- snapshot state machine handles this transparently.
--
-- Env vars:
--   LEGAIA_SSTATE        path to .sstate (default: $HOME/Tools/pcsx-redux/SCUS94254.sstate7)
--   LEGAIA_OUT_DIR       output dir (default: captures/boot_walk)
--   LEGAIA_OUT_PREFIX    per-snapshot filename prefix (default: snap_vsync_)
--   LEGAIA_TARGETS       comma-separated vsync targets after save-state load
--                        (default: "60,300,900,1500,1800")
--   LEGAIA_BOOT_DELAY    vsyncs to wait before loading save state (default: 60)
--
-- Output paths:
--   <out_dir>/<prefix><NNNN>.bin    - 2 MiB main RAM blob
--   <out_dir>/<prefix><NNNN>.regs   - text register snapshot

local function getenv(name, fallback)
    local v = os.getenv(name)
    if v == nil or v == "" then return fallback end
    return v
end

local SSTATE_PATH = getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate7")
local OUT_DIR     = getenv("LEGAIA_OUT_DIR", "captures/boot_walk")
local OUT_PREFIX  = getenv("LEGAIA_OUT_PREFIX", "snap_vsync_")
local BOOT_DELAY  = tonumber(getenv("LEGAIA_BOOT_DELAY", "60"))

local function parse_targets(s)
    local t = {}
    for n in string.gmatch(s, "([^,%s]+)") do
        local x = tonumber(n)
        if x ~= nil then t[#t+1] = x end
    end
    table.sort(t)
    return t
end
local TARGETS = parse_targets(getenv("LEGAIA_TARGETS", "60,300,900,1500,1800"))

local RAM_SIZE  = 2 * 1024 * 1024
local CHUNK     = 0x10000  -- 64 KiB - safe per-callback read size

PCSX.log(string.format(
    "[boot_walk] sstate=%s out_dir=%s targets=[%s] boot_delay=%d chunk=0x%X",
    SSTATE_PATH, OUT_DIR, table.concat(TARGETS, ","), BOOT_DELAY, CHUNK))

-- State machine:
--   IDLE        : waiting for save-state load completion
--   WAITING     : waiting for the next snapshot target vsync to arrive
--   SNAPSHOTTING: a snapshot is in progress, one chunk per vsync until 2 MiB done
--   DONE        : all targets captured; quit on next vsync

local S_WAIT_BOOT = 0
local S_WAITING   = 1
local S_SNAPPING  = 2
local S_DONE      = 3

local state            = S_WAIT_BOOT
local vsync_count      = 0
local load_complete_at = nil
local cursor           = 1   -- index into TARGETS for next target

-- Active-snapshot state (when state == S_SNAPPING)
local snap_target  = nil
local snap_off     = 0
local snap_fh      = nil
local snap_started_pc = nil
local snap_started_gp = nil

local function try_load_save_state()
    local fh, err = Support.File.open(SSTATE_PATH, "READ")
    if fh == nil or fh:failed() then
        PCSX.log(string.format("[boot_walk] FATAL: cannot open %s (%s)", SSTATE_PATH, tostring(err)))
        PCSX.quit(2)
        return false
    end
    local zfh = Support.File.zReader(fh)
    PCSX.loadSaveState(zfh)
    zfh:close()
    fh:close()
    PCSX.log("[boot_walk] save state loaded")
    return true
end

local function start_snapshot(target_vsync)
    -- Capture registers + open output file. The 2 MiB read happens
    -- across subsequent vsync callbacks.
    local r = PCSX.getRegisters()
    local function n(v) return bit.band(v, 0xFFFFFFFF) end
    snap_started_pc = n(tonumber(r.pc))
    snap_started_gp = n(tonumber(r.GPR.n.gp))

    local regs_path = string.format("%s/%s%04d.regs", OUT_DIR, OUT_PREFIX, target_vsync)
    local fh = io.open(regs_path, "w")
    if fh ~= nil then
        fh:write(string.format("# boot_walk snapshot at vsync_after_load=%d\n", target_vsync))
        fh:write(string.format("pc  0x%08X\n", snap_started_pc))
        fh:write(string.format("gp  0x%08X\n", snap_started_gp))
        fh:write(string.format("sp  0x%08X\n", n(tonumber(r.GPR.n.sp))))
        fh:write(string.format("ra  0x%08X\n", n(tonumber(r.GPR.n.ra))))
        fh:write(string.format("a0  0x%08X\n", n(tonumber(r.GPR.n.a0))))
        fh:write(string.format("a1  0x%08X\n", n(tonumber(r.GPR.n.a1))))
        fh:write(string.format("a2  0x%08X\n", n(tonumber(r.GPR.n.a2))))
        fh:write(string.format("a3  0x%08X\n", n(tonumber(r.GPR.n.a3))))
        fh:write(string.format("s8  0x%08X\n", n(tonumber(r.GPR.n.s8))))
        fh:close()
    end

    local bin_path = string.format("%s/%s%04d.bin", OUT_DIR, OUT_PREFIX, target_vsync)
    snap_fh = io.open(bin_path, "wb")
    if snap_fh == nil then
        PCSX.log(string.format("[boot_walk] FATAL: cannot open %s", bin_path))
        return false
    end
    snap_target = target_vsync
    snap_off    = 0
    PCSX.log(string.format("[boot_walk] snap %d: starting (pc=0x%08X, gp=0x%08X)",
        target_vsync, snap_started_pc, snap_started_gp))
    return true
end

local function advance_snapshot()
    -- Pull one 64 KiB chunk from emulator RAM, write to disk. Returns
    -- true when the snapshot is fully written.
    if snap_fh == nil then return true end
    local n = math.min(CHUNK, RAM_SIZE - snap_off)
    local mem_file = PCSX.getMemoryAsFile()
    local buf = mem_file:readAt(n, snap_off)
    if buf == nil then
        PCSX.log(string.format("[boot_walk] FATAL: read failed at off=0x%X for snap %d",
            snap_off, snap_target))
        snap_fh:close(); snap_fh = nil
        return true
    end
    snap_fh:write(tostring(buf))
    snap_off = snap_off + n
    if snap_off >= RAM_SIZE then
        snap_fh:close()
        snap_fh = nil
        PCSX.log(string.format("[boot_walk] snap %d: complete (%d bytes)", snap_target, RAM_SIZE))
        return true
    end
    return false
end

local function on_vsync()
    vsync_count = vsync_count + 1

    -- Liveness trace.
    if load_complete_at ~= nil then
        local rel = vsync_count - load_complete_at
        if rel > 0 and rel % 60 == 0 then
            PCSX.log(string.format("[boot_walk] heartbeat rel=%d  state=%d  cursor=%d  snap_off=0x%X",
                rel, state, cursor, snap_off or 0))
        end
    end

    if state == S_WAIT_BOOT then
        if vsync_count >= BOOT_DELAY then
            if try_load_save_state() then
                state = S_WAITING
                load_complete_at = vsync_count
            end
        end
        return
    end

    if state == S_WAITING then
        local rel = vsync_count - load_complete_at
        if cursor > #TARGETS then
            state = S_DONE
            return
        end
        if rel >= TARGETS[cursor] then
            if start_snapshot(TARGETS[cursor]) then
                state = S_SNAPPING
            else
                -- Open failed - skip this target
                cursor = cursor + 1
            end
        end
        return
    end

    if state == S_SNAPPING then
        if advance_snapshot() then
            cursor = cursor + 1
            state = S_WAITING
        end
        return
    end

    if state == S_DONE then
        PCSX.log("[boot_walk] all snapshots written; quitting")
        PCSX.quit(0)
        state = -1  -- prevent re-firing
        return
    end
end

PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
