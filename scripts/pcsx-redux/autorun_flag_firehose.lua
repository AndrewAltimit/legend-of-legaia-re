-- autorun_flag_firehose.lua
--
-- Whole-playthrough story-progression provenance capture. Unlike the
-- narrow spine probe (autorun_spine_flag_writers.lua), this logs EVERY
-- story-flag write with its writer, so one long play session answers all
-- current AND future "who sets flag X" questions for the segment played:
--
--   1. Exec-bp FUN_8003CE08 (0x8003CE08) - flag SET,   a0 = flag index.
--   2. Exec-bp FUN_8003CE34 (0x8003CE34) - flag CLEAR, a0 = flag index.
--      (TEST 0x8003CE64 is deliberately NOT armed: gate tests run every
--       walk-on/frame and reads are already census-known statically.)
--   3. Write-watch 0x8007B7FC - the scripted battle-id staging byte
--      (the Zeto-class trigger path).
--   4. Vsync poll: scene-name (0x8007050C) and game-mode (0x8007B83C)
--      transitions, so every flag row has a story-context timeline.
--
-- Every CSV row carries the writer pc/ra plus the current mode + scene.
-- Repeat suppression keeps pathological per-tick callers (e.g. the
-- chitei2 timed-flag scheduler, which pokes FUN_8003CE08 every frame
-- below its threshold) from flooding the CSV: per (kind,value,ra) key the
-- first DETAIL_FULL occurrences log fully, then only every SUPPRESS_EVERYth,
-- with the running count in the last column - totals stay reconstructible.
--
-- HUMAN-NAVIGATED session, NO self-quit: wrap the launch in
-- `timeout --kill-after`. Data volume is small (hundreds of KB for hours
-- of play); play as far as you like - deeper is strictly better.
--
-- Launch (MUST be -interpreter -debugger; Lua BPs are dead under --fast):
--   LEGAIA_NO_SSTATE=1 \
--   timeout --kill-after=15s 14400s \
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_flag_firehose.lua
--
-- Output:
--   flag_firehose.csv        tick,kind,value,pc,ra,mode,scene,count
--     kind = set | clear | battleid | scene | mode
--     value = flag index (set/clear), staged byte (battleid),
--             new mode byte (mode); scene rows carry the name in `scene`.
--   flag_firehose.detail.txt call-context for the first hit of each
--                            unique (kind, ra) writer site.
--
-- Analysis later: any `set`/`clear` row's ra attributes by containment
-- (attribute_overlay_hits.py) exactly like the spine runbook describes.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe  = require("probe")
local mem    = require("probe.mem")
local bp     = require("probe.bp")
local bit    = require("bit")
local sstate = require("probe.sstate")

-- +-- addresses -------------------------------------------------------------
local GAME_MODE     = 0x8007B83C  -- u8; field mode = 0x03
local SCENE_NAME    = 0x8007050C  -- 8-byte CDNAME label (pcsxr SCENE_NAME_VA)
local BATTLE_ID     = 0x8007B7FC  -- DAT_8007b7fc: battle-id staging byte
local FLAG_SET_PC   = 0x8003CE08  -- FUN_8003CE08: set bit;   a0 = flag index
local FLAG_CLEAR_PC = 0x8003CE34  -- FUN_8003CE34: clear bit; a0 = flag index

-- +-- config ----------------------------------------------------------------
local SSTATE     = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local NO_SSTATE  = probe.getenv("LEGAIA_NO_SSTATE", "") == "1"
local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
local DETAIL_MAX = probe.getenv_num("LEGAIA_MAX_DETAIL", 60)  -- unique (kind,ra) context dumps
local DETAIL_FULL    = 8    -- per-key: log this many rows unsuppressed...
local SUPPRESS_EVERY = 64   -- ...then only every Nth, count column running
local ARM_STABLE = 6        -- consecutive field-mode frames before arming

local CSV = probe.csv_open(probe.out_path("flag_firehose.csv"),
    "tick,kind,value,pc,ra,mode,scene,count")
local DETAIL = probe.out_path("flag_firehose.detail.txt")

-- Optional booster: LEGAIA_POINT_CARD_MAX=1 pins the Point Card counter at
-- its retail cap every vsync, so card strikes hit for max damage all
-- session (kept re-topped even where battle use spends points). The
-- counter is _DAT_800845B4 (u32, cap 9,999,999): the shop buy commit
-- FUN_801db7f4 accrues `price/20 * qty` into it when item 0xFE (the Point
-- Card) is held - see ghidra/scripts/funcs/overlay_shop_save_801db7f4.txt.
-- Lua pokes bypass the CPU, so this never pollutes the exec-bp capture.
local POINT_CARD_MAX  = probe.getenv("LEGAIA_POINT_CARD_MAX", "") == "1"
local POINT_CARD_ADDR = 0x800845B4
local POINT_CARD_CAP  = 9999999  -- 0x0098967F

-- Crash insurance: once armed, autosave the emulator state every N ticks
-- into the run dir, alternating two files so a crash mid-write can never
-- destroy the only copy. Resume a crashed session with
-- LEGAIA_SSTATE=<run dir>/autosave_a.sstate (whichever is newest).
-- Autosaves are raw (no Lua zWriter); probe.sstate.load sniffs the format.
-- 0 disables.
local AUTOSAVE_EVERY = probe.getenv_num("LEGAIA_AUTOSAVE_EVERY", 1800) -- ~30s
local AUTOSAVE_PATHS = { probe.out_path("autosave_a.sstate"),
                         probe.out_path("autosave_b.sstate") }
local autosave_flip  = 0

-- +-- helpers ----------------------------------------------------------------
local function u8(addr) return mem.read_u8(addr) or 0 end
local function regs()   return PCSX.getRegisters() end

-- LuaJIT bit ops return SIGNED 32-bit ints; normalize to unsigned so ra/pc
-- print as clean 8-digit hex (0x801E07BC, not 0xFFFFFFFF801E07BC).
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
local key_counts  = {}  -- "kind|value|ra" -> occurrences
local ra_detailed = {}  -- "kind|ra" -> true once call context dumped
local detail_used = 0
local totals      = { set = 0, clear = 0, battleid = 0, scene = 0, mode = 0 }
local last_scene  = nil
local last_mode   = nil

local function log(s)
    CSV.fh:flush()
    PCSX.log("[firehose] " .. s)
end

-- One event. Applies per-(kind,value,ra) repeat suppression; every written
-- row carries the key's running count so suppressed repeats reconstruct.
local function record(kind, value, pc, ra)
    totals[kind] = (totals[kind] or 0) + 1
    local key = string.format("%s|%d|%08X", kind, value, ra)
    local n = (key_counts[key] or 0) + 1
    key_counts[key] = n
    if n <= DETAIL_FULL or (n % SUPPRESS_EVERY) == 0 then
        CSV:row("%d,%s,%d,0x%08X,0x%08X,0x%02X,%s,%d",
            vsync, kind, value, pc, ra, u8(GAME_MODE), scene_name(), n)
        if n == 1 then
            PCSX.log(string.format(
                "[firehose] %-8s value=%-5d pc=0x%08X ra=0x%08X scene=%s",
                kind, value, pc, ra, scene_name()))
        end
    end
    local dkey = string.format("%s|%08X", kind, ra)
    if not ra_detailed[dkey] and detail_used < DETAIL_MAX then
        ra_detailed[dkey] = true
        detail_used = detail_used + 1
        probe.append_call_context(DETAIL, probe.capture_call_context(
            string.format("%s value=%d ra=0x%08X tick=%d scene=%s",
                kind, value, ra, vsync, scene_name())))
    end
end

-- +-- arm --------------------------------------------------------------------
local function arm_all()
    bp.arm(FLAG_SET_PC, "Exec", 4, "flag_set", function()
        local r  = regs()
        local a0 = (tonumber(r.GPR.n.a0) or 0) % 0x10000
        local ra = u32(r.GPR.n.ra)
        record("set", a0, FLAG_SET_PC, ra)
    end)
    bp.arm(FLAG_CLEAR_PC, "Exec", 4, "flag_clear", function()
        local r  = regs()
        local a0 = (tonumber(r.GPR.n.a0) or 0) % 0x10000
        local ra = u32(r.GPR.n.ra)
        record("clear", a0, FLAG_CLEAR_PC, ra)
    end)
    bp.arm(BATTLE_ID, "Write", 1, "battle_id", function()
        local r  = regs()
        local pc = u32(r.pc)
        local ra = u32(r.GPR.n.ra)
        record("battleid", u8(BATTLE_ID), pc, ra)
    end)
    armed = true
    log(string.format("armed at tick %d (mode=0x%02X scene=%s)",
        vsync, u8(GAME_MODE), scene_name()))
    log(string.format("  set   : Exec-bp 0x%08X (every flag SET, any a0)", FLAG_SET_PC))
    log(string.format("  clear : Exec-bp 0x%08X (every flag CLEAR, any a0)", FLAG_CLEAR_PC))
    log(string.format("  battle: Write-watch 0x%08X width 1", BATTLE_ID))
    log("  scene + mode transitions polled per vsync")
    if POINT_CARD_MAX then
        log(string.format("  point-card booster ON: 0x%08X pinned at %d every vsync",
            POINT_CARD_ADDR, POINT_CARD_CAP))
    end
    log("play as far as you like - every flag write is being recorded")
end

-- +-- vsync loop -------------------------------------------------------------
local function on_vsync()
    vsync = vsync + 1

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

    -- Scene + mode transition rows (context timeline; cheap, unconditional).
    local sc = scene_name()
    if sc ~= last_scene then
        last_scene = sc
        totals.scene = totals.scene + 1
        CSV:row("%d,scene,0,0x0,0x0,0x%02X,%s,%d",
            vsync, u8(GAME_MODE), sc, totals.scene)
        log(string.format("scene -> %s (tick %d)", sc, vsync))
    end
    local md = u8(GAME_MODE)
    if md ~= last_mode then
        last_mode = md
        totals.mode = totals.mode + 1
        CSV:row("%d,mode,%d,0x0,0x0,0x%02X,%s,%d", vsync, md, md, sc, totals.mode)
    end

    -- Arm once the game settles into field mode (arming exec-bps during a
    -- menu/battle-load blip can crash PCSX-Redux).
    if not armed then
        if md == 0x03 then
            field_frames = field_frames + 1
            if field_frames >= ARM_STABLE then arm_all() end
        else
            field_frames = 0
        end
        return
    end

    -- Point-card booster: re-top the counter every vsync while active.
    if POINT_CARD_MAX then
        mem.write_u16(POINT_CARD_ADDR,     POINT_CARD_CAP % 0x10000)
        mem.write_u16(POINT_CARD_ADDR + 2, math.floor(POINT_CARD_CAP / 0x10000))
        if (vsync % 480) == 0 then
            local lo = mem.read_u16(POINT_CARD_ADDR) or 0
            local hi = mem.read_u16(POINT_CARD_ADDR + 2) or 0
            local v = hi * 0x10000 + lo
            if v ~= POINT_CARD_CAP then
                log(string.format("point-card poke MISMATCH: read back %d", v))
            end
        end
    end

    -- Heartbeat every ~8s.
    if (vsync % 480) == 0 then
        log(string.format(
            "alive tick=%d mode=0x%02X scene=%s set=%d clear=%d battleid=%d",
            vsync, md, sc, totals.set, totals.clear, totals.battleid))
    end

    -- Rotating autosave (crash insurance; see AUTOSAVE_EVERY above).
    if AUTOSAVE_EVERY > 0 and (vsync % AUTOSAVE_EVERY) == 0 then
        autosave_flip = 1 - autosave_flip
        local path = AUTOSAVE_PATHS[autosave_flip + 1]
        if sstate.save(path) then
            log(string.format("autosaved -> %s (tick %d, scene=%s)",
                path, vsync, sc))
        end
    end
end

-- +-- startup ----------------------------------------------------------------
log("=== autorun_flag_firehose ===")
log("purpose: record EVERY story-flag set/clear + battle-id write with writer ra")
log("kinds: set/clear (value=flag idx), battleid (value=staged byte), scene, mode")
log("repeat suppression: first " .. DETAIL_FULL .. " per (kind,value,ra), then every "
    .. SUPPRESS_EVERY .. "th (count column keeps totals exact)")
log("this session never self-quits -- wrap the launch in timeout --kill-after")

PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
log("vsync listener installed; waiting for field mode to arm")
