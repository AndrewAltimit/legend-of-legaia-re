-- autorun_countdown_trigger.lua
--
-- Memory-watchpoint-driven RAM + screenshot snapshot. Arms a WRITE
-- breakpoint on _DAT_801EF16C (the title-attract countdown register).
--
-- The countdown is written *at least* twice during boot:
--   1. SCUS-side bulk init at FUN_8005DA40 stamps the 0x8000 sentinel
--      into the title-overlay BSS region (along with siblings sharing
--      a `...116C` low-half offset). This fires very early - before
--      the title overlay's tick function has ever run.
--   2. The title overlay's per-frame tick decrements it once the title
--      screen has become live (`*p = *p - 1`). This is the moment we
--      want to capture - PC + RA pin the title-overlay tick function
--      that hasn't been statically traced.
--
-- LEGAIA_HIT_SKIP controls how many BP hits to ignore before snapshotting.
--   * 1 (default for a save state taken pre-boot): skip the bulk init,
--     snapshot on the first decrement.
--   * 0 (for a save state taken *at* the title screen): bulk init has
--     already fired before save; the FIRST BP hit is a decrement.
--
-- Snapshot layout:
--   <OUT_PATH>             2 MiB main RAM dump (PSX virtual 0x80000000+)
--   <OUT_PATH>.regs        text register snapshot (pc/ra/gp/sp/a0..a3/s8)
--   <OUT_PATH>.screen      raw framebuffer pixels (BGR555 or BGR888)
--   <OUT_PATH>.screen.meta width=N\nheight=N\nbpp={16,24}\n
--
-- Implementation note: the BP callback ONLY records regs and flips the
-- state machine into S_DUMPING. The RAM dump itself spreads one 64 KiB
-- chunk per GPU::Vsync callback (a single 32x stacked read inside the
-- BP callback hangs the Lua event loop - this is documented in
-- autorun_boot_walk_snapshots.lua).
--
-- Env vars:
--   LEGAIA_SSTATE        path to .sstate (default: slot 7)
--   LEGAIA_OUT           output bin path (default: countdown_trigger.bin)
--   LEGAIA_BOOT_DELAY    vsyncs to wait before loading save state (default: 60)
--   LEGAIA_MAX_WAIT      max vsyncs to wait for the BP after load (default: 600)
--   LEGAIA_WATCH_ADDR    hex address to watch (default: 0x801EF16C)
--   LEGAIA_HIT_SKIP      number of BP hits to ignore (default: 1)

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
local HIT_SKIP = tonumber(getenv("LEGAIA_HIT_SKIP", "1")) or 1

-- Dump only the overlay window by default: 0x801C0000-0x80200000 (256
-- KiB), which holds the title/town/battle/menu/world-map overlays. This
-- avoids PCSX-Redux's interpreter+debugger segfault that fires after
-- ~1.5 MiB of cumulative readAt() bytes (reproducible across chunk sizes
-- 64 KiB and 256 KiB). Engines that want the full 2 MiB main RAM can
-- override via LEGAIA_DUMP_BASE / LEGAIA_DUMP_LEN.
--
-- Defaults match the documented overlay window in
-- docs/tooling/overlay-capture.md.
local function getenv_hex(name, fallback)
    local v = os.getenv(name)
    if v == nil or v == "" then return fallback end
    if v:match("^0[xX]") then return tonumber(v, 16) end
    return tonumber(v) or fallback
end
local DUMP_BASE = getenv_hex("LEGAIA_DUMP_BASE", 0x801C0000)
local DUMP_LEN  = getenv_hex("LEGAIA_DUMP_LEN",  0x40000)
local CHUNK     = 0x40000  -- one shot per vsync; matches DUMP_LEN by default

PCSX.log(string.format(
    "[trigger] dump base=0x%08X len=0x%X chunk=0x%X",
    DUMP_BASE, DUMP_LEN, CHUNK))

PCSX.log(string.format(
    "[trigger] sstate=%s out=%s watch=0x%08X boot_delay=%d max_wait=%d hit_skip=%d",
    SSTATE_PATH, OUT_PATH, WATCH_ADDR, BOOT_DELAY, MAX_WAIT, HIT_SKIP))

-- ------------------------------------------------------------------
-- State machine
--   S_WAIT_BOOT : BIOS settle window before loading save state
--   S_ARMED     : watchpoint armed, waiting for the (HIT_SKIP + 1)-th hit
--   S_DUMPING   : BP fired, regs written, spreading RAM read across vsyncs
--   S_DONE      : RAM dump complete + screenshot saved + about to quit

local S_WAIT_BOOT = 1
local S_ARMED     = 2
local S_DUMPING   = 3
local S_DONE      = 4

local state            = S_WAIT_BOOT
local vsync_count      = 0
local load_complete_at = nil
local hit_count        = 0

-- Active dump state (when state == S_DUMPING)
local dump_fh      = nil
local dump_off     = 0
local dump_reason  = nil

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

-- Take registers + screenshot + write regs sidecar + open the bin file.
-- Returns true on success. Called inside the BP callback exactly once.
--
-- Screenshot is taken HERE (in the BP callback, before transitioning to
-- the deferred RAM dump) because PCSX-Redux has been observed to
-- segfault during long-running RAM reads spread across vsync callbacks
-- in interpreter+debugger mode. Taking the screenshot first guarantees
-- we get the visible framebuffer even if the RAM dump aborts.
local function begin_dump(reason)
    local r = PCSX.getRegisters()
    local function n(v) return bit.band(v, 0xFFFFFFFF) end
    local pc = n(tonumber(r.pc))
    local ra = n(tonumber(r.GPR.n.ra))

    local regs_path = OUT_PATH .. ".regs"
    local rfh = io.open(regs_path, "w")
    if rfh ~= nil then
        rfh:write(string.format("# countdown_trigger snapshot (%s)\n", reason))
        rfh:write(string.format("watch_addr  0x%08X\n", WATCH_ADDR))
        rfh:write(string.format("hit_count   %d\n", hit_count))
        rfh:write(string.format("hit_skip    %d\n", HIT_SKIP))
        rfh:write(string.format("pc  0x%08X\n", pc))
        rfh:write(string.format("gp  0x%08X\n", n(tonumber(r.GPR.n.gp))))
        rfh:write(string.format("sp  0x%08X\n", n(tonumber(r.GPR.n.sp))))
        rfh:write(string.format("ra  0x%08X\n", ra))
        rfh:write(string.format("a0  0x%08X\n", n(tonumber(r.GPR.n.a0))))
        rfh:write(string.format("a1  0x%08X\n", n(tonumber(r.GPR.n.a1))))
        rfh:write(string.format("a2  0x%08X\n", n(tonumber(r.GPR.n.a2))))
        rfh:write(string.format("a3  0x%08X\n", n(tonumber(r.GPR.n.a3))))
        rfh:write(string.format("s8  0x%08X\n", n(tonumber(r.GPR.n.s8))))
        rfh:close()
    end
    PCSX.log(string.format("[trigger] capture (%s): pc=0x%08X ra=0x%08X",
        reason, pc, ra))

    -- Take screenshot first (cheap + survives any later crash).
    local ss_ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if ss_ok and ss ~= nil and ss.data ~= nil then
        local ss_path = OUT_PATH .. ".screen"
        local sfh = io.open(ss_path, "wb")
        if sfh ~= nil then
            sfh:write(tostring(ss.data))
            sfh:close()
            local bpp_str = (ss.bpp == 0) and "16" or "24"
            PCSX.log(string.format(
                "[trigger] wrote screenshot %dx%d bpp=%s to %s",
                tonumber(ss.width), tonumber(ss.height), bpp_str, ss_path))
            local mfh = io.open(ss_path .. ".meta", "w")
            if mfh ~= nil then
                mfh:write(string.format("width=%d\n", tonumber(ss.width)))
                mfh:write(string.format("height=%d\n", tonumber(ss.height)))
                mfh:write(string.format("bpp=%s\n", bpp_str))
                mfh:close()
            end
        end
    else
        PCSX.log(string.format("[trigger] WARN: takeScreenShot failed (%s)",
            tostring(ss)))
    end

    dump_fh = io.open(OUT_PATH, "wb")
    if dump_fh == nil then
        PCSX.log(string.format("[trigger] FATAL: cannot open %s", OUT_PATH))
        return false
    end
    dump_off = 0
    dump_reason = reason
    return true
end

-- Pull one CHUNK from emulator RAM, write to disk. Returns true
-- when the dump is complete. Called once per vsync while S_DUMPING.
-- `dump_off` is the offset within the dump window (DUMP_BASE-relative);
-- the file is the slice of main RAM starting at DUMP_BASE.
local function advance_dump()
    if dump_fh == nil then return true end
    local remaining = DUMP_LEN - dump_off
    if remaining <= 0 then
        dump_fh:close()
        dump_fh = nil
        return true
    end
    local n = math.min(CHUNK, remaining)
    local mem_file = PCSX.getMemoryAsFile()
    -- PCSX-Redux memory file is indexed by KSEG0/USEG offset (low 29
    -- bits of the virtual address).
    local ram_off = bit.band(DUMP_BASE + dump_off, 0x1FFFFFFF)
    local buf = mem_file:readAt(n, ram_off)
    if buf == nil then
        PCSX.log(string.format(
            "[trigger] FATAL: read failed at off=0x%X (ram=0x%X)",
            dump_off, ram_off))
        dump_fh:close()
        dump_fh = nil
        return true
    end
    dump_fh:write(tostring(buf))
    dump_off = dump_off + n
    if dump_off >= DUMP_LEN then
        dump_fh:close()
        dump_fh = nil
        PCSX.log(string.format(
            "[trigger] wrote %d bytes (base=0x%08X) to %s",
            DUMP_LEN, DUMP_BASE, OUT_PATH))
        return true
    end
    return false
end

-- (Screenshot is taken inside begin_dump now; this helper is unused but
-- kept in case a future variant wants to grab a fresh screenshot after
-- the RAM dump rather than at the BP fire.)

local function arm_watchpoint()
    -- 2-byte write breakpoint at WATCH_ADDR. The countdown is read/written
    -- as u16 (lh/sh) per the cutscene.md doc ("underflows"). A width-2
    -- BP catches sh / sb operations whose address matches.
    local bp = PCSX.addBreakpoint(WATCH_ADDR, "Write", 2, "trigger:countdown",
        function()
            if state ~= S_ARMED then return end
            hit_count = hit_count + 1
            if hit_count <= HIT_SKIP then
                local r = PCSX.getRegisters()
                local function n(v) return bit.band(v, 0xFFFFFFFF) end
                PCSX.log(string.format(
                    "[trigger] skipping hit %d/%d (pc=0x%08X ra=0x%08X)",
                    hit_count, HIT_SKIP,
                    n(tonumber(r.pc)), n(tonumber(r.GPR.n.ra))))
                return
            end
            -- This is the targeted fire. Snapshot regs now (inside the
            -- BP callback, where PC/RA reflect the exact write
            -- instruction), then defer the heavy work to vsync.
            if begin_dump(string.format("watchpoint:hit#%d", hit_count)) then
                state = S_DUMPING
            else
                -- Open failed - abort
                state = S_DONE
            end
        end)
    if bp == nil then
        PCSX.log("[trigger] FATAL: addBreakpoint returned nil")
        PCSX.quit(3)
        return false
    end
    PCSX.log(string.format(
        "[trigger] watchpoint armed at 0x%08X (width=2, kind=Write, skip=%d)",
        WATCH_ADDR, HIT_SKIP))
    return true
end

local function on_vsync()
    vsync_count = vsync_count + 1

    if state == S_WAIT_BOOT then
        if vsync_count >= BOOT_DELAY then
            if try_load_save_state() then
                if arm_watchpoint() then
                    state = S_ARMED
                    load_complete_at = vsync_count
                end
            end
        end
        return
    end

    if state == S_ARMED then
        local rel = vsync_count - load_complete_at
        if rel > 0 and rel % 60 == 0 then
            -- Read the live countdown value so we can see if it's drifting
            -- (e.g. another write the BP missed because the write width
            -- straddled the BP's 2-byte window). The BP catches sh/sb
            -- whose effective address matches; a sw at 0x801EF16A
            -- covering 0x801EF16C..16E wouldn't.
            local cur = "?"
            local mf = PCSX.getMemoryAsFile()
            local off = bit.band(WATCH_ADDR, 0x1FFFFFFF)
            local ok, v = pcall(function() return mf:readU16At(off) end)
            if ok and v then cur = string.format("0x%04X", tonumber(v)) end
            PCSX.log(string.format(
                "[trigger] heartbeat rel=%d  hits=%d  cur=%s  state=ARMED",
                rel, hit_count, cur))
        end
        if rel >= MAX_WAIT then
            PCSX.log(string.format(
                "[trigger] MAX_WAIT %d reached without enough hits (%d/%d+); aborting without snapshot",
                MAX_WAIT, hit_count, HIT_SKIP + 1))
            state = S_DONE
        end
        return
    end

    if state == S_DUMPING then
        if advance_dump() then
            state = S_DONE
        end
        return
    end

    if state == S_DONE then
        PCSX.log("[trigger] done; quitting")
        PCSX.quit(0)
        state = -1  -- prevent re-firing
        return
    end
end

PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
