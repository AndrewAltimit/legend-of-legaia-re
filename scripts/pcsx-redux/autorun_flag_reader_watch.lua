-- autorun_flag_reader_watch.lua
--
-- Find the engine-side READER of a story flag that is write-only in
-- field-VM script space. The census (`man-scripts --system-flag-census`)
-- proves some flags are SET by script bytes but TESTED by NO script - so
-- their consumer is engine/overlay code. This probe names that consumer.
--
-- Motivating case: flag 0x1BE (446, the Jeremi/`geremi` arrival one-shot).
-- It is SET at the head of geremi P2[0] and has zero Test sites disc-wide,
-- so whatever reacts to "the player has arrived at Jeremi" reads it in
-- compiled code, not bytecode.
--
-- HOW STORY FLAGS ARE READ (static, from ghidra/scripts/funcs):
--   The bank base is DAT_80085758. Flag `n` lives at byte
--   0x80085758 + (n>>3), bit mask 0x80 >> (n&7).
--     FUN_8003CE08(n)  SET   bit           (the firehose arms this)
--     FUN_8003CE34(n)  CLEAR bit
--     FUN_8003CE64(n)  TEST  bit -> 0xFF if set else 0   <- the getter
--   FUN_8003CE64 is the shared test-flag helper (150+ callers), so the
--   reader almost certainly funnels through it. We cannot resolve "which
--   caller passes n=446" statically (a0 is loaded dynamically in nearly
--   every caller), so we arm it at RUNTIME with an a0==target FILTER and
--   capture the caller `ra`. That is the deliverable.
--
-- TWO WATCHES (belt + braces):
--   1. Exec-bp FUN_8003CE64, filter a0==TARGET -> the helper-path reader.
--      `ra` = the routine that tested the flag; call-context captured.
--   2. Read-watch on the flag's byte (width 1) -> a DIRECT (inlined) reader
--      that bypasses the helper. This byte holds 8 flags, so it also fires
--      for the helper's own `lbu` (pc 0x8003CE74, suppressed here since
--      watch #1 covers it) and for save/load bulk scans; hits are deduped
--      by (pc,ra) and you post-filter by checking the code at `pc` masks
--      the target bit.
--
-- WHAT TO DO to make the reader fire: load a state where the target flag is
-- already SET (e.g. the post-geremi `ropeway2` autosave for 0x1BE), then
-- exercise the paths that would consult a progress marker: open the field
-- menu, SAVE the game (the save writer scans the bank), and cross a couple
-- of scene transitions (re-enter `geremi` if you can). Deeper + more varied
-- navigation = more reader sites.
--
-- VERSION GUARD: refuses to arm unless the loaded game fingerprints as the
-- USA SCUS_942.54 build. HUMAN-NAVIGATED, NO self-quit: wrap in
-- `timeout --kill-after`. Lua BPs are DEAD under --fast; run -interpreter
-- -debugger (the default, i.e. do NOT pass --fast).
--
-- Launch:
--   LEGAIA_SSTATE=captures/state_poll/<ts>/autosave_a.sstate \
--   LEGAIA_FLAG=0x1BE \
--   timeout --kill-after=15s 3600s \
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_flag_reader_watch.lua
--
-- Output:
--   flag_reader_watch.csv     tick,kind,flag,pc,ra,mode,scene,count
--     kind = test  (helper hit, ra = caller of FUN_8003CE64 with a0==flag)
--          | byteread (direct byte read; post-filter by the mask at pc)
--          | scene | mode  (context timeline)
--   flag_reader_watch.detail.txt  call context for the first hit per ra.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe   = require("probe")
local mem     = require("probe.mem")
local bp      = require("probe.bp")
local bit     = require("bit")
local sstate  = require("probe.sstate")
local version = require("probe.version")

-- +-- addresses -------------------------------------------------------------
local GAME_MODE   = 0x8007B83C  -- u8; field mode = 0x03
local SCENE_NAME  = 0x8007050C  -- 8-byte CDNAME label
local FLAG_BASE   = 0x80085758  -- story-flag bank base (DAT_80085758)
local FLAG_GET_PC = 0x8003CE64  -- FUN_8003CE64: test bit; a0 = flag index
local CE64_LBU_PC = 0x8003CE74  -- the getter's own lbu (suppress in byteread)

-- +-- config ----------------------------------------------------------------
-- Target flag index. Default 0x1BE (446, geremi arrival one-shot). Accepts
-- 0x.. or decimal.
local function parse_int(s, dflt)
    if s == nil or s == "" then return dflt end
    if s:lower():sub(1, 2) == "0x" then return tonumber(s:sub(3), 16) or dflt end
    return tonumber(s) or dflt
end
local TARGET = parse_int(probe.getenv("LEGAIA_FLAG", ""), 0x1BE)
local FLAG_BYTE = FLAG_BASE + bit.rshift(TARGET, 3)
local FLAG_MASK = bit.rshift(0x80, bit.band(TARGET, 7))

local SSTATE     = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local NO_SSTATE  = probe.getenv("LEGAIA_NO_SSTATE", "") == "1"
local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
local DETAIL_MAX = probe.getenv_num("LEGAIA_MAX_DETAIL", 60)
-- Also watch the raw byte for a direct/inlined reader (default on). Set 0 to
-- track only the helper path (quieter).
local DIRECT_READ = probe.getenv("LEGAIA_DIRECT_READ", "1") == "1"
local ARM_STABLE  = 6

local CSV = probe.csv_open(probe.out_path("flag_reader_watch.csv"),
    "tick,kind,flag,pc,ra,mode,scene,count")
local DETAIL = probe.out_path("flag_reader_watch.detail.txt")

local AUTOSAVE_EVERY = probe.getenv_num("LEGAIA_AUTOSAVE_EVERY", 1800)
local AUTOSAVE_PATHS = { probe.out_path("autosave_a.sstate"),
                         probe.out_path("autosave_b.sstate") }
local autosave_flip  = 0

-- +-- helpers ----------------------------------------------------------------
local function u8(addr) return mem.read_u8(addr) or 0 end
local function regs()   return PCSX.getRegisters() end
local function u32(v)
    v = bit.band(tonumber(v) or 0, 0xFFFFFFFF)
    if v < 0 then v = v + 4294967296 end
    return v
end
local function scene_name()
    local s = ""
    for i = 0, 7 do
        local b = u8(SCENE_NAME + i)
        if b < 0x20 or b >= 0x7F then break end
        s = s .. string.char(b)
    end
    return (s == "") and "?" or s
end

-- +-- state ------------------------------------------------------------------
local vsync       = 0
local loaded_at   = nil
local armed       = false
local field_frames = 0
local version_pass = false
local capture_disabled = false
local key_counts  = {}
local ra_detailed = {}
local detail_used = 0
local totals      = { test = 0, byteread = 0, scene = 0, mode = 0 }
local last_scene  = nil
local last_mode   = nil

local function log(s)
    CSV.fh:flush()
    PCSX.log("[reader] " .. s)
end

-- Called from INSIDE a bp callback (emulation thread). Read regs/RAM here;
-- queue all file/GUI I/O for the vsync drain.
local pending = {}
local function record(kind, pc, ra)
    totals[kind] = (totals[kind] or 0) + 1
    local key = string.format("%s|%08X|%08X", kind, pc, ra)
    local n = (key_counts[key] or 0) + 1
    key_counts[key] = n
    local ev = nil
    if n <= 8 or (n % 64) == 0 then
        ev = {
            csv = string.format("%d,%s,%d,0x%08X,0x%08X,0x%02X,%s,%d",
                vsync, kind, TARGET, pc, ra, u8(GAME_MODE), scene_name(), n),
        }
        if n == 1 then
            ev.log = string.format(
                "[reader] %-8s flag=0x%X pc=0x%08X ra=0x%08X scene=%s",
                kind, TARGET, pc, ra, scene_name())
        end
    end
    local dkey = string.format("%s|%08X", kind, ra)
    if not ra_detailed[dkey] and detail_used < DETAIL_MAX then
        ra_detailed[dkey] = true
        detail_used = detail_used + 1
        ev = ev or {}
        ev.detail = probe.capture_call_context(
            string.format("%s flag=0x%X pc=0x%08X ra=0x%08X tick=%d scene=%s",
                kind, TARGET, pc, ra, vsync, scene_name()))
    end
    if ev then pending[#pending + 1] = ev end
end

local function drain_pending()
    if #pending == 0 then return end
    for i = 1, #pending do
        local ev = pending[i]
        if ev.csv then CSV:row("%s", ev.csv) end
        if ev.log then PCSX.log(ev.log) end
        if ev.detail then probe.append_call_context(DETAIL, ev.detail) end
    end
    pending = {}
end

-- +-- arm --------------------------------------------------------------------
local function arm_all()
    -- 1. The helper path: FUN_8003CE64(a0). Only record when a0==TARGET.
    bp.arm(FLAG_GET_PC, "Exec", 4, "flag_get", function()
        local r  = regs()
        local a0 = (tonumber(r.GPR.n.a0) or 0) % 0x10000
        if a0 ~= TARGET then return end
        record("test", FLAG_GET_PC, u32(r.GPR.n.ra))
    end)
    -- 2. Direct/inlined readers: watch the flag's byte. Suppress the getter's
    --    own lbu (pc CE64_LBU_PC) since watch #1 already covers that path.
    if DIRECT_READ then
        bp.arm(FLAG_BYTE, "Read", 1, "flag_byte", function()
            local r  = regs()
            local pc = u32(r.pc)
            if pc == CE64_LBU_PC then return end
            record("byteread", pc, u32(r.GPR.n.ra))
        end)
    end
    armed = true
    log(string.format("armed at tick %d (mode=0x%02X scene=%s)",
        vsync, u8(GAME_MODE), scene_name()))
    log(string.format("  target flag = 0x%X (%d)  byte=0x%08X mask=0x%02X",
        TARGET, TARGET, FLAG_BYTE, FLAG_MASK))
    log(string.format("  bank value now: byte=0x%02X -> flag is %s",
        u8(FLAG_BYTE),
        (bit.band(u8(FLAG_BYTE), FLAG_MASK) ~= 0) and "SET" or "CLEAR"))
    if bit.band(u8(FLAG_BYTE), FLAG_MASK) == 0 then
        log("  WARNING: target flag is CLEAR in this state - a reader that")
        log("  short-circuits on 'clear' may not reveal itself; load a state")
        log("  where the flag is already SET for the strongest signal.")
    end
    log("  test    : Exec-bp 0x8003CE64 filtered a0==target (helper readers)")
    if DIRECT_READ then
        log(string.format("  byteread: Read-watch 0x%08X w1 (direct readers; dedup by pc,ra)",
            FLAG_BYTE))
    end
    log("  now: open the menu, SAVE, cross scene transitions to trigger reads")
end

-- +-- version gate -----------------------------------------------------------
local function check_version_gate()
    if version_pass then return true end
    if version.record_mode() then
        local sig = version.record_fingerprint()
        if sig then
            log("fingerprint = " .. sig)
            log("RECORD MODE: paste into version.USA_FINGERPRINT, relaunch. Not arming.")
            capture_disabled = true
        end
        return false
    end
    local ok, msg, terminal = version.check(version.USA_FINGERPRINT)
    if ok then
        version_pass = true
        log("version guard: " .. msg)
        return true
    end
    if terminal then
        log("FATAL version guard: " .. msg)
        capture_disabled = true
        return false
    end
    if (vsync % 60) == 0 then log("waiting for SCUS: " .. msg) end
    return false
end

-- +-- vsync loop -------------------------------------------------------------
local function on_vsync()
    vsync = vsync + 1
    if capture_disabled then return end

    if loaded_at == nil then
        if NO_SSTATE then
            loaded_at = vsync
            log("LEGAIA_NO_SSTATE=1 -- load a card save by hand")
        elseif vsync >= BOOT_DELAY then
            if not probe.load_save_state(SSTATE) then
                log("FATAL: could not load save state; check LEGAIA_SSTATE (or set LEGAIA_NO_SSTATE=1)")
                loaded_at = -1
                return
            end
            loaded_at = vsync
            log(string.format("state loaded at tick %d; mode=0x%02X", vsync, u8(GAME_MODE)))
        end
        return
    end
    if loaded_at < 0 then return end

    if not version_pass then
        if not check_version_gate() then return end
    end

    drain_pending()

    local sc = scene_name()
    if sc ~= last_scene then
        last_scene = sc
        totals.scene = totals.scene + 1
        CSV:row("%d,scene,%d,0x0,0x0,0x%02X,%s,%d",
            vsync, TARGET, u8(GAME_MODE), sc, totals.scene)
        log(string.format("scene -> %s (tick %d)", sc, vsync))
    end
    local md = u8(GAME_MODE)
    if md ~= last_mode then
        last_mode = md
        totals.mode = totals.mode + 1
        CSV:row("%d,mode,%d,0x0,0x0,0x%02X,%s,%d", vsync, md, md, sc, totals.mode)
    end

    if not armed then
        if md == 0x03 then
            field_frames = field_frames + 1
            if field_frames >= ARM_STABLE then arm_all() end
        else
            field_frames = 0
        end
        return
    end

    if (vsync % 480) == 0 then
        log(string.format("alive tick=%d mode=0x%02X scene=%s test=%d byteread=%d",
            vsync, md, sc, totals.test, totals.byteread))
    end

    if AUTOSAVE_EVERY > 0 and (vsync % AUTOSAVE_EVERY) == 0 then
        autosave_flip = 1 - autosave_flip
        local path = AUTOSAVE_PATHS[autosave_flip + 1]
        if sstate.save(path) then
            log(string.format("autosaved -> %s (tick %d, scene=%s)", path, vsync, sc))
        end
    end
end

-- +-- startup ----------------------------------------------------------------
log("=== autorun_flag_reader_watch ===")
log(string.format("purpose: name the engine-side READER of flag 0x%X (a0==target on FUN_8003CE64 + byte read-watch)",
    TARGET))
log("this session never self-quits -- wrap the launch in timeout --kill-after")

PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
log("vsync listener installed; waiting for field mode to arm")
