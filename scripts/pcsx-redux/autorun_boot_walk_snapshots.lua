-- autorun_boot_walk_snapshots.lua
--
-- Multi-snapshot RAM-and-register probe for boot-sequence walks. One
-- emulator launch loads a save state, then takes a RAM dump + CPU
-- register snapshot at each emulator vsync in TARGETS. Drops one
-- ".bin" (2 MiB main RAM) and one ".regs" sidecar per snapshot.
--
-- Critical implementation detail: a single 2 MiB readAt() permanently
-- breaks subsequent vsync delivery in PCSX-Redux's Lua harness.
-- Stacking 32x 64 KiB reads in one callback degrades it too. The fix:
-- one 64 KiB chunk per vsync callback. The lib gives us the
-- boot-delay/save-load/quit scaffolding; the chunk-spreader runs as a
-- custom on_capture state machine.
--
-- Env vars:
--   LEGAIA_SSTATE        save state path (default sstate7)
--   LEGAIA_OUT_DIR       output dir (default captures/boot_walk)
--   LEGAIA_OUT_PREFIX    per-snapshot filename prefix (default snap_vsync_)
--   LEGAIA_TARGETS       comma-separated post-load vsync targets
--                        (default 60,300,900,1500,1800)
--
-- Outputs (one per target):
--   <dir>/<prefix><NNNN>.bin    2 MiB main RAM blob
--   <dir>/<prefix><NNNN>.regs   text register snapshot (pc/gp/sp/ra/a0..a3/s8)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate7")
local OUT_DIR     = probe.getenv("LEGAIA_OUT_DIR", "captures/boot_walk")
local OUT_PREFIX  = probe.getenv("LEGAIA_OUT_PREFIX", "snap_vsync_")

local function parse_targets(s)
    local t = {}
    for n in string.gmatch(s, "([^,%s]+)") do
        local x = tonumber(n)
        if x ~= nil then t[#t + 1] = x end
    end
    table.sort(t)
    return t
end

local TARGETS = parse_targets(probe.getenv("LEGAIA_TARGETS",
    "60,300,900,1500,1800"))

local CHUNK = 0x10000  -- 64 KiB per vsync — safe per-callback read size

PCSX.log(string.format(
    "[boot_walk] sstate=%s out_dir=%s targets=[%s] chunk=0x%X",
    SSTATE_PATH, OUT_DIR, table.concat(TARGETS, ","), CHUNK))

os.execute(string.format("mkdir -p %q", OUT_DIR))

-- Snapshot sub-state machine (runs inside on_capture).
local S_WAITING  = 1
local S_SNAPPING = 2
local S_DONE     = 3

local sub_state    = S_WAITING
local cursor       = 1
local snap_target  = nil
local snap_off     = 0
local snap_fh      = nil
local last_capture_max = 0

local function n32(v) return bit.band(v, 0xFFFFFFFF) end

local function start_snapshot(target_vsync)
    local r = PCSX.getRegisters()
    local pc = n32(tonumber(r.pc) or 0)
    local gp = n32(tonumber(r.GPR.n.gp) or 0)

    local regs_path = string.format("%s/%s%04d.regs",
        OUT_DIR, OUT_PREFIX, target_vsync)
    local fh = io.open(regs_path, "w")
    if fh ~= nil then
        fh:write(string.format(
            "# boot_walk snapshot at vsync_after_load=%d\n", target_vsync))
        fh:write(string.format("pc  0x%08X\n", pc))
        fh:write(string.format("gp  0x%08X\n", gp))
        fh:write(string.format("sp  0x%08X\n", n32(tonumber(r.GPR.n.sp) or 0)))
        fh:write(string.format("ra  0x%08X\n", n32(tonumber(r.GPR.n.ra) or 0)))
        fh:write(string.format("a0  0x%08X\n", n32(tonumber(r.GPR.n.a0) or 0)))
        fh:write(string.format("a1  0x%08X\n", n32(tonumber(r.GPR.n.a1) or 0)))
        fh:write(string.format("a2  0x%08X\n", n32(tonumber(r.GPR.n.a2) or 0)))
        fh:write(string.format("a3  0x%08X\n", n32(tonumber(r.GPR.n.a3) or 0)))
        fh:write(string.format("s8  0x%08X\n", n32(tonumber(r.GPR.n.s8) or 0)))
        fh:close()
    end

    local bin_path = string.format("%s/%s%04d.bin",
        OUT_DIR, OUT_PREFIX, target_vsync)
    snap_fh = io.open(bin_path, "wb")
    if snap_fh == nil then
        PCSX.log(string.format("[boot_walk] FATAL: cannot open %s", bin_path))
        return false
    end
    snap_target = target_vsync
    snap_off    = 0
    PCSX.log(string.format(
        "[boot_walk] snap %d: starting (pc=0x%08X gp=0x%08X)",
        target_vsync, pc, gp))
    return true
end

local function advance_snapshot()
    if snap_fh == nil then return true end
    local n = math.min(CHUNK, probe.RAM_SIZE - snap_off)
    local buf = probe.read_bytes(0x80000000 + snap_off, n)
    if buf == nil then
        PCSX.log(string.format(
            "[boot_walk] FATAL: read failed at off=0x%X for snap %d",
            snap_off, snap_target))
        snap_fh:close(); snap_fh = nil
        return true
    end
    snap_fh:write(tostring(buf))
    snap_off = snap_off + n
    if snap_off >= probe.RAM_SIZE then
        snap_fh:close()
        snap_fh = nil
        PCSX.log(string.format(
            "[boot_walk] snap %d: complete (%d bytes)",
            snap_target, probe.RAM_SIZE))
        return true
    end
    return false
end

-- Worst-case capture_frames needs to cover the latest target plus the
-- 32 chunks (32 vsyncs) needed to spread the 2 MiB read across vsyncs,
-- plus a small tail margin.
local last_target  = TARGETS[#TARGETS] or 0
local capture_need = last_target + 64

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = capture_need,

    on_arm = function(_)
        PCSX.log(string.format(
            "[boot_walk] %d targets, capture_frames=%d",
            #TARGETS, capture_need))
        return {}
    end,

    on_capture = function(ctx, elapsed)
        last_capture_max = elapsed

        if sub_state == S_WAITING then
            if cursor > #TARGETS then
                sub_state = S_DONE
                ctx.request_quit = true
                return
            end
            if elapsed >= TARGETS[cursor] then
                if start_snapshot(TARGETS[cursor]) then
                    sub_state = S_SNAPPING
                else
                    cursor = cursor + 1
                end
            end
        elseif sub_state == S_SNAPPING then
            if advance_snapshot() then
                cursor = cursor + 1
                sub_state = S_WAITING
            end
        end
    end,

    on_done = function(_, _)
        PCSX.log(string.format(
            "[boot_walk] done; last_capture_vsync=%d", last_capture_max))
    end,
})
