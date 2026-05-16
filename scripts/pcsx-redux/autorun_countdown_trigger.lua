-- autorun_countdown_trigger.lua
--
-- Memory-watchpoint-driven RAM snapshot. Arms a WRITE breakpoint on
-- _DAT_801EF16C (the title-attract countdown register). The very first
-- write that changes its value from the post-init 0x8000 sentinel is
-- the moment the title overlay's tick function starts decrementing it
-- - i.e., the title screen has become live.
--
-- The breakpoint snapshots RAM and captures registers exactly at that
-- moment, then quits. No vsync-count guessing needed; the trigger is
-- the game's own state transition.
--
-- Env vars:
--   LEGAIA_SSTATE        path to .sstate (default: slot 7)
--   LEGAIA_OUT           output bin path (default: countdown_trigger.bin)
--   LEGAIA_BOOT_DELAY    vsyncs to wait before loading save state (default: 60)
--   LEGAIA_MAX_WAIT      max vsyncs to wait for the BP after load (default: 600)
--   LEGAIA_WATCH_ADDR    hex address to watch (default: 0x801EF16C)

local function getenv(name, fallback)
    local v = os.getenv(name)
    if v == nil or v == "" then return fallback end
    return v
end

local SSTATE_PATH = getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate7")
local OUT_PATH    = getenv("LEGAIA_OUT", "countdown_trigger.bin")
local BOOT_DELAY  = tonumber(getenv("LEGAIA_BOOT_DELAY", "60"))
local MAX_WAIT    = tonumber(getenv("LEGAIA_MAX_WAIT", "600"))
-- WATCH_ADDR: parse env in either hex (0x...) or decimal form, with a
-- safe hex literal default. Avoids the typoed-decimal-literal trap.
local WATCH_ADDR = 0x801EF16C  -- title-attract countdown
local watch_env = os.getenv("LEGAIA_WATCH_ADDR")
if watch_env and watch_env ~= "" then
    if watch_env:match("^0[xX]") then
        WATCH_ADDR = tonumber(watch_env, 16)
    else
        WATCH_ADDR = tonumber(watch_env)
    end
end
if WATCH_ADDR == nil then WATCH_ADDR = 0x801EF16C end

local RAM_SIZE = 2 * 1024 * 1024
local CHUNK    = 0x10000

PCSX.log(string.format(
    "[trigger] sstate=%s out=%s watch=0x%08X boot_delay=%d max_wait=%d",
    SSTATE_PATH, OUT_PATH, WATCH_ADDR, BOOT_DELAY, MAX_WAIT))

local STATE_WAIT_BOOT = 1
local STATE_ARMED     = 2
local STATE_DONE      = 3

local state            = STATE_WAIT_BOOT
local vsync_count      = 0
local load_complete_at = nil
local snapshot_taken   = false

local function try_load_save_state()
    local fh, err = Support.File.open(SSTATE_PATH, "READ")
    if fh == nil or fh:failed() then
        PCSX.log(string.format("[trigger] FATAL: cannot open %s (%s)",
            SSTATE_PATH, tostring(err)))
        PCSX.quit(2)
        return false
    end
    local zfh = Support.File.zReader(fh)
    PCSX.loadSaveState(zfh)
    zfh:close()
    fh:close()
    PCSX.log("[trigger] save state loaded")
    return true
end

local function capture_snapshot(reason)
    if snapshot_taken then return end
    snapshot_taken = true

    -- Register snapshot
    local r = PCSX.getRegisters()
    local function n(v) return bit.band(v, 0xFFFFFFFF) end
    local regs_path = OUT_PATH .. ".regs"
    local fh = io.open(regs_path, "w")
    if fh ~= nil then
        fh:write(string.format("# countdown_trigger snapshot (%s)\n", reason))
        fh:write(string.format("watch_addr  0x%08X\n", WATCH_ADDR))
        fh:write(string.format("pc  0x%08X\n", n(tonumber(r.pc))))
        fh:write(string.format("gp  0x%08X\n", n(tonumber(r.GPR.n.gp))))
        fh:write(string.format("sp  0x%08X\n", n(tonumber(r.GPR.n.sp))))
        fh:write(string.format("ra  0x%08X\n", n(tonumber(r.GPR.n.ra))))
        fh:write(string.format("a0  0x%08X\n", n(tonumber(r.GPR.n.a0))))
        fh:write(string.format("a1  0x%08X\n", n(tonumber(r.GPR.n.a1))))
        fh:write(string.format("a2  0x%08X\n", n(tonumber(r.GPR.n.a2))))
        fh:write(string.format("a3  0x%08X\n", n(tonumber(r.GPR.n.a3))))
        fh:write(string.format("s8  0x%08X\n", n(tonumber(r.GPR.n.s8))))
        fh:close()
    end
    PCSX.log(string.format("[trigger] capture (%s): pc=0x%08X ra=0x%08X",
        reason, n(tonumber(r.pc)), n(tonumber(r.GPR.n.ra))))

    -- RAM dump - chunked to avoid breaking subsequent events. We've
    -- already taken the registers we need; if vsync breaks after this
    -- we don't care.
    local mem_file = PCSX.getMemoryAsFile()
    local out_fh = io.open(OUT_PATH, "wb")
    if out_fh == nil then
        PCSX.log(string.format("[trigger] FATAL: cannot open %s", OUT_PATH))
        return
    end
    local off = 0
    while off < RAM_SIZE do
        local cn = math.min(CHUNK, RAM_SIZE - off)
        local buf = mem_file:readAt(cn, off)
        if buf == nil then break end
        out_fh:write(tostring(buf))
        off = off + cn
    end
    out_fh:close()
    PCSX.log(string.format("[trigger] wrote %d bytes to %s", off, OUT_PATH))
    PCSX.quit(0)
end

local function arm_watchpoint()
    -- 2-byte write breakpoint at WATCH_ADDR. The countdown is read/written
    -- as u16 (lh/sh) per the cutscene.md doc ("underflows"). A width-2
    -- BP catches sh / sb operations whose address matches.
    local bp = PCSX.addBreakpoint(WATCH_ADDR, "Write", 2, "trigger:countdown",
        function()
            capture_snapshot("watchpoint")
        end)
    if bp == nil then
        PCSX.log("[trigger] FATAL: addBreakpoint returned nil")
        PCSX.quit(3)
        return false
    end
    PCSX.log(string.format("[trigger] watchpoint armed at 0x%08X (width=2, kind=Write)", WATCH_ADDR))
    return true
end

local function on_vsync()
    vsync_count = vsync_count + 1
    if state == STATE_WAIT_BOOT then
        if vsync_count >= BOOT_DELAY then
            if try_load_save_state() then
                if arm_watchpoint() then
                    state = STATE_ARMED
                    load_complete_at = vsync_count
                end
            end
        end
    elseif state == STATE_ARMED then
        local rel = vsync_count - load_complete_at
        if rel > 0 and rel % 60 == 0 then
            PCSX.log(string.format("[trigger] heartbeat rel=%d  taken=%s", rel, tostring(snapshot_taken)))
        end
        if rel >= MAX_WAIT and not snapshot_taken then
            PCSX.log(string.format("[trigger] MAX_WAIT %d reached without watchpoint hit; taking fallback snapshot", MAX_WAIT))
            capture_snapshot("max_wait_fallback")
            state = STATE_DONE
        end
    end
end

PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
