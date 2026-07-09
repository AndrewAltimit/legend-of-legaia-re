-- autorun_flag_reader_watch.lua
--
-- Story-flag PROVENANCE capture: name the engine-side READERS and WRITERS
-- of story flags, for the whole segment played - not just one target.
--
-- WHY "everything, deduped" instead of one filtered flag: the interpreter
-- tier is the expensive resource (a human trekking at ~10 fps). A run that
-- only answers "who reads flag X" wastes the trek if you later need flag Y
-- from the same segment. The static census can't backstop that: the
-- bytecode walker desyncs in dialogue-heavy MANs (the 0x528 case - census
-- said zero TEST sites, the live capture found 1951 reads at ra
-- 0x801E35E8), so runtime reads are NOT always census-known. This probe
-- therefore arms all three flag helpers UNFILTERED and dedups by
-- (kind, flag, ra) - one session banks reader+writer provenance for every
-- flag the segment touches, answering current AND future questions.
--
-- HOW STORY FLAGS ARE ACCESSED (static, from ghidra/scripts/funcs):
--   Bank base DAT_80085758. Flag `n` lives at byte 0x80085758 + (n>>3),
--   bit mask 0x80 >> (n&7).
--     FUN_8003CE08(n)  SET   bit
--     FUN_8003CE34(n)  CLEAR bit
--     FUN_8003CE64(n)  TEST  bit -> 0xFF if set else 0   <- the getter
--
-- WATCHES:
--   1. Exec-bp FUN_8003CE64 - EVERY test: (a0=flag, ra=reader). Target
--      flags additionally get call-context detail + a first-hit snapshot.
--   2. Exec-bp FUN_8003CE08 / FUN_8003CE34 - EVERY set/clear with writer
--      ra (the firehose's writer capture, merged in; LEGAIA_WRITERS=0 if
--      you want the quieter read-only probe).
--   3. Read-watch on each TARGET flag's byte (width 1) - catches a DIRECT
--      (inlined) reader that bypasses the helper. The byte holds 8 flags
--      and bulk save/copy scans also touch it, so post-filter by checking
--      the code at `pc` masks the target bit (the analyzer marks these).
--      Accesses from inside the three helpers (0x8003CE08..0x8003CE8F)
--      are suppressed - watches 1/2 already cover them.
--
-- TARGETS: LEGAIA_FLAG accepts a COMMA LIST ("0x1E8,0x5A0,0x5A1,0x6C3") -
-- one trek answers the whole worklist. Targets get byteread watches,
-- prioritized detail capture, and a first-hit auto-snapshot.
--
-- CONTEXT: every row carries mode + scene + (in field mode) the player
-- tile in the note column ("t<x>;<z>") - door/trigger attribution without
-- a second pass. New-scene auto-snapshots (LEGAIA_AUTOSNAP, capped) bank a
-- save state at the mouth of every area reached, so a future run resumes
-- adjacent to any beat instead of replaying the trek. manifest.txt records
-- the run's config + source sstate (the resume/provenance chain).
--
-- WHAT TO DO to make a target's reader fire: load a state where the flag
-- is already SET, then exercise the paths that would consult a progress
-- marker: open the field menu, SAVE the game, and cross scene transitions
-- (re-enter the flag's scene if you can). Deeper + more varied navigation
-- = more reader sites - and with the unfiltered capture, everything else
-- you walk past is banked too.
--
-- VERSION GUARD: refuses to arm unless the loaded game fingerprints as the
-- USA SCUS_942.54 build. HUMAN-NAVIGATED, NO self-quit: wrap in
-- `timeout --kill-after`. Lua BPs are DEAD under --fast; run -interpreter
-- -debugger (the default, i.e. do NOT pass --fast).
--
-- Launch:
--   LEGAIA_SSTATE=captures/state_poll/<ts>/autosave_a.sstate \
--   LEGAIA_FLAG=0x1E8,0x5A0,0x5A1,0x6C3 \
--   timeout --kill-after=15s 3600s \
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_flag_reader_watch.lua
--
-- Output (summarize with scripts/pcsx-redux/analyze_reader_watch.py):
--   flag_reader_watch.csv    tick,kind,flag,pc,ra,mode,scene,count,note
--     kind = test | set | clear   (helper hits; flag = a0, ra = caller)
--          | byteread             (direct read of a target byte; post-filter)
--          | scene | mode | snap  (context timeline / snapshot record)
--     note = "tgt" marks a target flag; "t<x>;<z>" player tile (field mode)
--   flag_reader_watch.detail.txt  call context (targets prioritized)
--   manifest.txt                  run config + source sstate
--   snap_*.sstate                 new-scene + first-target-hit snapshots

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
local FLAG_SET_PC   = 0x8003CE08  -- FUN_8003CE08: set bit;   a0 = flag index
local FLAG_CLEAR_PC = 0x8003CE34  -- FUN_8003CE34: clear bit; a0 = flag index
local FLAG_GET_PC   = 0x8003CE64  -- FUN_8003CE64: test bit;  a0 = flag index
-- The three helpers' own loads/stores span this range; suppress them in the
-- byteread watch (the exec-bps above already attribute those paths).
local HELPER_LO   = 0x8003CE08
local HELPER_HI   = 0x8003CE90
-- Player actor pointer (field mode only); world X/Z s16 at +0x14/+0x18;
-- tile = (pos-0x40)>>7. Same derivation as autorun_state_poll.lua P1.
local PLAYER_PTR  = 0x8007C364
local POS_X_OFF   = 0x14
local POS_Z_OFF   = 0x18
local FIELD_MODE  = 0x03

-- +-- config ----------------------------------------------------------------
-- Target flags: comma list, 0x.. or decimal. Targets get byteread watches +
-- detail priority + first-hit snapshots; everything else is still captured
-- (deduped) unless LEGAIA_ALL_TESTS=0.
local function parse_int(s)
    if s == nil or s == "" then return nil end
    if s:lower():sub(1, 2) == "0x" then return tonumber(s:sub(3), 16) end
    return tonumber(s)
end
local TARGETS = {}       -- flag -> true
local TARGET_LIST = {}   -- ordered, for logging
do
    local spec = probe.getenv("LEGAIA_FLAG", "0x1BE")
    for tok in spec:gmatch("[^,%s]+") do
        local n = parse_int(tok)
        if n ~= nil and not TARGETS[n] then
            TARGETS[n] = true
            TARGET_LIST[#TARGET_LIST + 1] = n
        end
    end
end

local SSTATE     = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local NO_SSTATE  = probe.getenv("LEGAIA_NO_SSTATE", "") == "1"
local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
-- Unfiltered TEST capture (default on). 0 = only target flags logged.
local ALL_TESTS  = probe.getenv("LEGAIA_ALL_TESTS", "1") ~= "0"
-- Writer capture via SET/CLEAR exec-bps (default on). 0 = readers only.
local WRITERS    = probe.getenv("LEGAIA_WRITERS", "1") ~= "0"
-- Byteread watch on each target byte (default on). 0 = helper path only.
local DIRECT_READ = probe.getenv("LEGAIA_DIRECT_READ", "1") == "1"
-- Detail budgets: targets get their own (per unique kind|flag|ra), and are
-- never starved by background churn (per unique kind|ra, shared cap).
local DETAIL_MAX     = probe.getenv_num("LEGAIA_MAX_DETAIL", 60)
local TGT_DETAIL_MAX = probe.getenv_num("LEGAIA_MAX_TGT_DETAIL", 48)
-- Row suppression per dedup key: targets log 8 then every 64th; background
-- 4 then every 256th (count column keeps totals exact either way).
local TGT_FULL, TGT_EVERY = 8, 64
local BG_FULL,  BG_EVERY  = 4, 256
local ARM_STABLE  = 6
-- New-scene / first-target-hit snapshots (P2, ported from the poll tier).
local AUTOSNAP  = probe.getenv("LEGAIA_AUTOSNAP", "1") ~= "0"
local SNAP_MAX  = probe.getenv_num("LEGAIA_SNAP_MAX", 20)

local CSV = probe.csv_open(probe.out_path("flag_reader_watch.csv"),
    "tick,kind,flag,pc,ra,mode,scene,count,note")
local DETAIL = probe.out_path("flag_reader_watch.detail.txt")

local AUTOSAVE_EVERY = probe.getenv_num("LEGAIA_AUTOSAVE_EVERY", 1800)
local AUTOSAVE_PATHS = { probe.out_path("autosave_a.sstate"),
                         probe.out_path("autosave_b.sstate") }
local autosave_flip  = 0

-- +-- helpers ----------------------------------------------------------------
local function u8(addr)  return mem.read_u8(addr)  or 0 end
local function u16(addr) return mem.read_u16(addr) or 0 end
local function regs()    return PCSX.getRegisters() end
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
local function s16(v) return (v >= 0x8000) and (v - 0x10000) or v end
-- Player tile as a note fragment ("t<x>;<z>"), or "" outside field mode /
-- with no live actor. Semicolon separator keeps the CSV column count sane.
local function tile_note()
    if u8(GAME_MODE) ~= FIELD_MODE then return "" end
    local ptr = mem.read_u32(PLAYER_PTR) or 0
    local off = mem.ram_offset(ptr)
    if off == nil or off < 0x10000 then return "" end
    if not mem.in_ram(ptr + POS_Z_OFF, 2) then return "" end
    local x = s16(u16(ptr + POS_X_OFF))
    local z = s16(u16(ptr + POS_Z_OFF))
    return string.format("t%d;%d",
        math.floor((x - 0x40) / 128), math.floor((z - 0x40) / 128))
end

-- +-- state ------------------------------------------------------------------
local vsync       = 0
local loaded_at   = nil
local armed       = false
local field_frames = 0
local version_pass = false
local capture_disabled = false
local key_counts  = {}   -- "kind|flag|ra" -> occurrences
local ra_detailed = {}   -- background: "kind|ra" -> true once detailed
local tgt_detailed = {}  -- targets:    "kind|flag|ra" -> true once detailed
local detail_used = 0
local tgt_detail_used = 0
local totals      = { test = 0, set = 0, clear = 0, byteread = 0,
                      scene = 0, mode = 0, snap = 0 }
local last_scene  = nil
local last_mode   = nil
local seen_scenes = {}   -- new-scene snapshot trigger
local snap_flags  = {}   -- first-target-hit snapshot, once per flag
local snap_count  = 0
local pending_snaps = {} -- snapshot requests queued by bp callbacks

local function log(s)
    CSV.fh:flush()
    PCSX.log("[reader] " .. s)
end

-- Called from INSIDE a bp callback (emulation thread). Read regs/RAM here;
-- queue all file/GUI/sstate I/O for the vsync drain.
local pending = {}
local function record(kind, flag, pc, ra)
    local tgt = TARGETS[flag] or (kind == "byteread")
    totals[kind] = (totals[kind] or 0) + 1
    local key = string.format("%s|%d|%08X", kind, flag, ra)
    local n = (key_counts[key] or 0) + 1
    key_counts[key] = n
    local full, every
    if tgt then full, every = TGT_FULL, TGT_EVERY
    else        full, every = BG_FULL,  BG_EVERY end
    local ev = nil
    if n <= full or (n % every) == 0 then
        local note = tgt and "tgt" or ""
        local tn = tile_note()
        if tn ~= "" then note = (note == "") and tn or (note .. " " .. tn) end
        ev = {
            csv = string.format("%d,%s,%d,0x%08X,0x%08X,0x%02X,%s,%d,%s",
                vsync, kind, flag, pc, ra, u8(GAME_MODE), scene_name(), n, note),
        }
        if n == 1 and (tgt or kind ~= "test") then
            ev.log = string.format(
                "[reader] %-8s flag=0x%-4X pc=0x%08X ra=0x%08X scene=%s%s",
                kind, flag, pc, ra, scene_name(), tgt and " [TGT]" or "")
        end
    end
    -- Call-context detail: targets have their own budget (per kind|flag|ra)
    -- so background churn can never starve the flags this run is FOR.
    local want_detail = false
    if tgt then
        if not tgt_detailed[key] and tgt_detail_used < TGT_DETAIL_MAX then
            tgt_detailed[key] = true
            tgt_detail_used = tgt_detail_used + 1
            want_detail = true
        end
    else
        local dkey = string.format("%s|%08X", kind, ra)
        if not ra_detailed[dkey] and detail_used < DETAIL_MAX then
            ra_detailed[dkey] = true
            detail_used = detail_used + 1
            want_detail = true
        end
    end
    if want_detail then
        ev = ev or {}
        ev.detail = probe.capture_call_context(
            string.format("%s flag=0x%X pc=0x%08X ra=0x%08X tick=%d scene=%s",
                kind, flag, pc, ra, vsync, scene_name()))
    end
    -- First helper hit on a target flag: queue a snapshot (a mid-beat
    -- bracket exactly at the moment the flag mattered). Drained at vsync -
    -- sstate.save is I/O and must NOT run on the emulation thread.
    if TARGETS[flag] and kind ~= "byteread" and not snap_flags[flag] then
        snap_flags[flag] = true
        pending_snaps[#pending_snaps + 1] = string.format("hit_f%X", flag)
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

-- P2: fingerprinted snapshot on a rare event (new scene / first target hit).
-- Vsync-thread only. A `snap` CSV row records the reason + filename.
local function autosnap(reason)
    if not AUTOSNAP or snap_count >= SNAP_MAX then return end
    local sc = scene_name()
    local fname = string.format("snap_%07d_%s_%s.sstate", vsync, reason, sc)
    if sstate.save(probe.out_path(fname)) then
        snap_count = snap_count + 1
        totals.snap = snap_count
        CSV:row("%d,snap,%d,0x0,0x0,0x%02X,%s,%d,%s -> %s",
            vsync, snap_count, u8(GAME_MODE), sc, snap_count, reason, fname)
        log(string.format("AUTOSNAP #%d/%d: %s (tick %d scene %s)",
            snap_count, SNAP_MAX, reason, vsync, sc))
    end
end

-- +-- arm --------------------------------------------------------------------
local function arm_all()
    -- 1. TEST helper: every read (or targets only, LEGAIA_ALL_TESTS=0).
    bp.arm(FLAG_GET_PC, "Exec", 4, "flag_get", function()
        local r  = regs()
        local a0 = (tonumber(r.GPR.n.a0) or 0) % 0x10000
        if not ALL_TESTS and not TARGETS[a0] then return end
        record("test", a0, FLAG_GET_PC, u32(r.GPR.n.ra))
    end)
    -- 2. SET/CLEAR helpers: every write with writer ra (firehose merge).
    if WRITERS then
        bp.arm(FLAG_SET_PC, "Exec", 4, "flag_set", function()
            local r  = regs()
            local a0 = (tonumber(r.GPR.n.a0) or 0) % 0x10000
            record("set", a0, FLAG_SET_PC, u32(r.GPR.n.ra))
        end)
        bp.arm(FLAG_CLEAR_PC, "Exec", 4, "flag_clear", function()
            local r  = regs()
            local a0 = (tonumber(r.GPR.n.a0) or 0) % 0x10000
            record("clear", a0, FLAG_CLEAR_PC, u32(r.GPR.n.ra))
        end)
    end
    -- 3. Direct/inlined readers: one Read-watch per distinct TARGET byte.
    --    Suppress the helpers' own accesses (watches 1/2 cover those). The
    --    flag column carries the byte's representative target (lowest);
    --    the byte holds 8 flags, so post-filter by the mask at `pc`.
    if DIRECT_READ then
        local byte_rep = {}  -- byte addr -> lowest target flag on it
        for _, f in ipairs(TARGET_LIST) do
            local addr = FLAG_BASE + bit.rshift(f, 3)
            if byte_rep[addr] == nil or f < byte_rep[addr] then
                byte_rep[addr] = f
            end
        end
        for addr, f in pairs(byte_rep) do
            bp.arm(addr, "Read", 1, string.format("flag_byte_%X", f), function()
                local r  = regs()
                local pc = u32(r.pc)
                if pc >= HELPER_LO and pc < HELPER_HI then return end
                record("byteread", f, pc, u32(r.GPR.n.ra))
            end)
        end
    end
    armed = true
    log(string.format("armed at tick %d (mode=0x%02X scene=%s)",
        vsync, u8(GAME_MODE), scene_name()))
    local tl = {}
    for _, f in ipairs(TARGET_LIST) do
        local byte = FLAG_BASE + bit.rshift(f, 3)
        local mask = bit.rshift(0x80, bit.band(f, 7))
        local set  = bit.band(u8(byte), mask) ~= 0
        tl[#tl + 1] = string.format("0x%X(%s)", f, set and "SET" or "clear")
        if not set then
            log(string.format("  NOTE: target 0x%X is CLEAR in this state - a"
                .. " reader that short-circuits on 'clear' may hide; a state"
                .. " with it SET gives the strongest signal.", f))
        end
    end
    log("  targets: " .. table.concat(tl, " "))
    log(string.format("  test : Exec-bp 0x%08X %s", FLAG_GET_PC,
        ALL_TESTS and "UNFILTERED (all flags, deduped)" or "targets only"))
    if WRITERS then
        log(string.format("  set/clear: Exec-bp 0x%08X / 0x%08X (all writers)",
            FLAG_SET_PC, FLAG_CLEAR_PC))
    end
    if DIRECT_READ then
        log("  byteread: Read-watch per target byte (direct readers; dedup by pc,ra)")
    end
    log("  now: open the menu, SAVE, cross scene transitions to trigger reads")

    probe.write_manifest("autorun_flag_reader_watch.lua", {
        targets        = table.concat(tl, " "),
        sstate         = NO_SSTATE and "(hand-loaded card save)" or SSTATE,
        all_tests      = tostring(ALL_TESTS),
        writers        = tostring(WRITERS),
        direct_read    = tostring(DIRECT_READ),
        autosnap       = string.format("%s (max %d)", tostring(AUTOSNAP), SNAP_MAX),
        autosave_every = tostring(AUTOSAVE_EVERY),
        armed_tick     = tostring(vsync),
        armed_scene    = scene_name(),
        core           = "interpreter+debugger (required; BPs dead under --fast)",
    })
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
    if #pending_snaps > 0 then
        for i = 1, #pending_snaps do autosnap(pending_snaps[i]) end
        pending_snaps = {}
    end

    local sc = scene_name()
    if sc ~= last_scene then
        last_scene = sc
        totals.scene = totals.scene + 1
        CSV:row("%d,scene,0,0x0,0x0,0x%02X,%s,%d,",
            vsync, u8(GAME_MODE), sc, totals.scene)
        log(string.format("scene -> %s (tick %d)", sc, vsync))
        -- New-scene snapshot: bank a state at the mouth of every area this
        -- trek reaches, so a future run starts adjacent, not from scratch.
        if armed and sc ~= "?" and not seen_scenes[sc] then
            seen_scenes[sc] = true
            autosnap("scene_" .. sc)
        end
    end
    local md = u8(GAME_MODE)
    if md ~= last_mode then
        last_mode = md
        totals.mode = totals.mode + 1
        CSV:row("%d,mode,%d,0x0,0x0,0x%02X,%s,%d,", vsync, md, md, sc, totals.mode)
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
        log(string.format(
            "alive tick=%d mode=0x%02X scene=%s test=%d set=%d clear=%d byteread=%d snap=%d",
            vsync, md, sc, totals.test, totals.set, totals.clear,
            totals.byteread, snap_count))
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
log("=== autorun_flag_reader_watch (flag provenance) ===")
log(string.format("targets: %d flag(s); unfiltered test=%s writers=%s",
    #TARGET_LIST, tostring(ALL_TESTS), tostring(WRITERS)))
log("every flag tested/set/cleared this session is recorded with its ra (deduped)")
log("this session never self-quits -- wrap the launch in timeout --kill-after")

PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
log("vsync listener installed; waiting for field mode to arm")
