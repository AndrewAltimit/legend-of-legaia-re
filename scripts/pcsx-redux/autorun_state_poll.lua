-- autorun_state_poll.lua
--
-- FAST-CORE, whole-playthrough progression capture - the community-
-- handoff sibling of autorun_flag_firehose.lua.
--
-- WHY THIS EXISTS
--   The firehose gets writer provenance (the `ra` that set each flag) by
--   arming exec-breakpoints on FUN_8003CE08 / _CE34. Lua breakpoints only
--   fire under `-interpreter -debugger` (see run_probe.sh), and the
--   interpreter is the ~10x tax that makes the firehose run at ~10 fps -
--   miserable to play, and it rides the known live-display
--   scene-transition segfault race.
--
--   This probe arms NO breakpoints. It only POLLS RAM every vsync and
--   diffs against the previous frame, so it runs under the recompiler
--   (`--fast`) at ~full speed and never touches the debugger crash
--   surface. Trade-off: you get "flag X flipped in scene Y at tick T",
--   NOT the writer `ra`. For a community-scale MAP of what-changes-where
--   that is the 90% dataset; for the handful where you need the `ra`, run
--   the exec-bp firehose yourself in a targeted burst against the exact
--   scene this probe fingered.
--
-- WHAT IT CAPTURES (all by per-frame diff; intra-frame churn is naturally
-- filtered - a flag set-then-cleared inside one frame shows no change):
--   flagset/flagclr  story-flag bank 0x80085758 (idx space == firehose's)
--   battleid         0x8007B7FC staged battle id (the Zeto-class trigger)
--   gold             0x8008459C party gold (with delta)
--   item             0x80085958 inventory: id/count changes (with delta) -
--                    consumables AND the start of the key-item page, so
--                    quest-item grants land too
--   party            0x80084594 count + 0x80084598 member-id list
--   scene / mode     0x8007050C name + 0x8007B83C mode transitions
--
-- VERSION GUARD: refuses to run unless the loaded game fingerprints as the
-- USA SCUS_942.54 build (probe/version.lua). Lock the fingerprint before
-- handoff so a volunteer on a JP/EU/PAL disc gets a hard refusal, not
-- silent garbage. See COMMUNITY-CAPTURE.md.
--
-- HUMAN-NAVIGATED, NO self-quit: wrap the launch in `timeout --kill-after`.
-- Data volume is small (hundreds of KB for hours of play); play as far as
-- you like - deeper is strictly better.
--
-- Launch (note: --fast; NO -interpreter needed since no BPs are armed):
--   LEGAIA_NO_SSTATE=1 \
--   timeout --kill-after=15s 14400s \
--   bash scripts/pcsx-redux/run_probe.sh --fast \
--     --lua scripts/pcsx-redux/autorun_state_poll.lua
--
-- Output:
--   state_poll.csv   tick,kind,idx,value,delta,mode,scene,note
--   Resume a crashed session with LEGAIA_SSTATE=<run dir>/autosave_a.sstate
--   (whichever is newest).

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe   = require("probe")
local mem     = require("probe.mem")
local sstate  = require("probe.sstate")
local version = require("probe.version")
local bit     = require("bit")

-- +-- addresses -------------------------------------------------------------
local GAME_MODE  = 0x8007B83C  -- u8; field mode = 0x03
local SCENE_NAME = 0x8007050C  -- 8-byte CDNAME label
local BATTLE_ID  = 0x8007B7FC  -- DAT_8007b7fc battle-id staging byte
local FLAG_BASE  = 0x80085758  -- fourth flag bank; idx 0 == firehose value 0
local GOLD       = 0x8008459C  -- u32 party gold
local PARTY_CNT  = 0x80084594  -- u8 party member count
local PARTY_IDS  = 0x80084598  -- u8[4] member ids
local INV_BASE   = 0x80085958  -- inventory (id,count) 2-byte stride

-- +-- config ----------------------------------------------------------------
local SSTATE    = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local NO_SSTATE = probe.getenv("LEGAIA_NO_SSTATE", "") == "1"
local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
-- Flag window in BYTES from FLAG_BASE. Default 0x200 = flag idx 0..4095.
-- This is bounded DELIBERATELY: the char-record slot-3 tail ends exactly at
-- FLAG_BASE (0x80085758) and the item inventory begins exactly at
-- FLAG_BASE+0x200 (0x80085958), so 0x200 is the largest window that is pure
-- story-flag bytes with NO overlap onto volatile char-record or inventory
-- cells (inventory is diffed separately below). idx space matches the
-- firehose's a0. Widening past 0x200 re-introduces inventory double-counting
-- - only do it (knowingly) to chase a flag above idx 4095.
local FLAG_BYTES = probe.getenv_num("LEGAIA_FLAG_WINDOW", 0x200)
-- Inventory window in slots (2 bytes each). 128 covers the 72 consumables
-- plus the start of the key-item page (quest items).
local INV_SLOTS  = probe.getenv_num("LEGAIA_INV_SLOTS", 128)

local AUTOSAVE_EVERY = probe.getenv_num("LEGAIA_AUTOSAVE_EVERY", 1800) -- ~30s
local AUTOSAVE_PATHS = { probe.out_path("autosave_a.sstate"),
                         probe.out_path("autosave_b.sstate") }
local autosave_flip  = 0

-- Optional cruise booster: LEGAIA_POINT_CARD_MAX=1 pins the Point Card counter
-- at its retail cap every vsync, so a Point Card (item 0xFE) strike nukes any
-- boss for max damage - the easiest way to blow through fights while capturing
-- progression. Ported verbatim from autorun_flag_firehose.lua. The counter is
-- _DAT_800845B4 (u32, cap 9,999,999): the shop buy commit FUN_801db7f4 accrues
-- `price/20 * qty` into it when the Point Card is held (see
-- ghidra/scripts/funcs/overlay_shop_save_801db7f4.txt). It writes ONLY this
-- counter - none of the CSV progression cells (flags/battle-id/gold/items/
-- party/scene/mode) - so the capture stays intact. Off by default: a normal
-- run never writes memory. You still need the Point Card in inventory and must
-- USE it in battle; this just keeps its damage pinned at max.
local POINT_CARD_MAX  = probe.getenv("LEGAIA_POINT_CARD_MAX", "") == "1"
local POINT_CARD_ADDR = 0x800845B4
local POINT_CARD_CAP  = 9999999  -- 0x0098967F

local CSV = probe.csv_open(probe.out_path("state_poll.csv"),
    "tick,kind,idx,value,delta,mode,scene,note")

-- +-- helpers ----------------------------------------------------------------
local function u8(addr)  return mem.read_u8(addr)  or 0 end
local function u16(addr) return mem.read_u16(addr) or 0 end
local function u32(addr) return mem.read_u32(addr) or 0 end

local function scene_name()
    local s = ""
    for i = 0, 7 do
        local b = u8(SCENE_NAME + i)
        if b < 0x20 or b >= 0x7F then break end
        s = s .. string.char(b)
    end
    return (s == "") and "?" or s
end

local function log(s)
    CSV.fh:flush()
    PCSX.log("[state_poll] " .. s)
end

-- +-- state ------------------------------------------------------------------
local vsync      = 0
local loaded_at  = nil
local baselined  = false     -- true once the first snapshot is taken
local last_scene = nil
local last_mode  = nil
-- previous-frame snapshots
local prev_flags = nil       -- string of FLAG_BYTES bytes
local prev_batid = nil
local prev_gold  = nil
local prev_pcnt  = nil
local prev_pids  = nil
local prev_inv   = nil       -- string of INV_SLOTS*2 bytes
local totals     = { flagset = 0, flagclr = 0, battleid = 0, gold = 0,
                     item = 0, party = 0, scene = 0, mode = 0 }

local function row(kind, idx, value, delta, note)
    totals[kind] = (totals[kind] or 0) + 1
    CSV:row("%d,%s,%d,%d,%d,0x%02X,%s,%s",
        vsync, kind, idx, value, delta, u8(GAME_MODE), scene_name(),
        note or "")
end

-- +-- diffs ------------------------------------------------------------------

-- Flag bank: XOR each changed byte; each flipped bit -> one flag row.
-- Bit convention MATCHES FUN_8003CE08: byte = base + (idx>>3),
-- mask = 0x80 >> (idx & 7). So within a byte, bit position p (0=LSB..7=MSB)
-- maps to idx&7 = 7 - p, and idx = byte_index*8 + (7 - p).
local function diff_flags(cur)
    if prev_flags == nil then return end
    for i = 1, #cur do
        local a = prev_flags:byte(i)
        local b = cur:byte(i)
        if a ~= b then
            local x = bit.bxor(a, b)
            for p = 0, 7 do
                if bit.band(x, bit.lshift(1, p)) ~= 0 then
                    local idx = (i - 1) * 8 + (7 - p)
                    local nowset = bit.band(b, bit.lshift(1, p)) ~= 0
                    if nowset then
                        row("flagset", idx, 1, 1)
                    else
                        row("flagclr", idx, 0, -1)
                    end
                end
            end
        end
    end
end

-- Inventory: diff (id,count) pairs slot by slot; log net change per slot.
local function diff_inv(cur)
    if prev_inv == nil then return end
    for s = 0, INV_SLOTS - 1 do
        local o = s * 2 + 1
        local pid, pct = prev_inv:byte(o), prev_inv:byte(o + 1)
        local cid, cct = cur:byte(o), cur:byte(o + 1)
        if pid ~= cid or pct ~= cct then
            local delta = cct - pct
            -- note carries slot + old->new id when the id itself changed
            local note = string.format("slot%d", s)
            if pid ~= cid then
                note = string.format("slot%d id%02X->%02X", s, pid, cid)
            end
            row("item", cid, cct, delta, note)
        end
    end
end

local function snapshot_and_diff()
    -- Flag bank
    local flags = mem.read_bytes(FLAG_BASE, FLAG_BYTES)
    if flags ~= nil then
        flags = tostring(flags)
        if baselined then diff_flags(flags) end
        prev_flags = flags
    end

    -- Battle-id staging byte
    local batid = u8(BATTLE_ID)
    if baselined and batid ~= prev_batid and batid ~= 0 then
        row("battleid", 0, batid, batid - (prev_batid or 0))
    end
    prev_batid = batid

    -- Gold
    local gold = u32(GOLD)
    if baselined and gold ~= prev_gold then
        row("gold", 0, gold, gold - (prev_gold or 0))
    end
    prev_gold = gold

    -- Party count + ids
    local pcnt = u8(PARTY_CNT)
    local pids = { u8(PARTY_IDS), u8(PARTY_IDS + 1),
                   u8(PARTY_IDS + 2), u8(PARTY_IDS + 3) }
    local pidstr = string.format("%02X%02X%02X%02X",
        pids[1], pids[2], pids[3], pids[4])
    if baselined and (pcnt ~= prev_pcnt or pidstr ~= prev_pids) then
        row("party", pcnt, pcnt, pcnt - (prev_pcnt or 0), "ids=" .. pidstr)
    end
    prev_pcnt = pcnt
    prev_pids = pidstr

    -- Inventory
    local inv = mem.read_bytes(INV_BASE, INV_SLOTS * 2)
    if inv ~= nil then
        inv = tostring(inv)
        if baselined then diff_inv(inv) end
        prev_inv = inv
    end

    baselined = true
end

-- +-- vsync loop -------------------------------------------------------------
local function on_vsync()
    vsync = vsync + 1

    -- Load the save state (or wait for a hand-loaded card save).
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
            log(string.format("state loaded at tick %d", vsync))
        end
        return
    end
    if loaded_at < 0 then return end

    -- Version guard: gate ALL capture on a confirmed USA build. Re-checked
    -- until it passes (RAM may not be resident the first frame post-load).
    if not baselined then
        if version.record_mode() then
            local sig = version.record_fingerprint()  -- nil until SCUS resident
            if sig then
                log("fingerprint = " .. sig)
                log("RECORD MODE: paste into version.USA_FINGERPRINT (or export "
                    .. "LEGAIA_FP_EXPECTED), relaunch WITHOUT LEGAIA_FP_RECORD to "
                    .. "capture. Not arming.")
                loaded_at = -1  -- stop; record-only
            end
            return
        end
        local ok, msg, terminal = version.check(version.USA_FINGERPRINT)
        if not ok then
            -- terminal = genuine wrong revision. Non-terminal = booting;
            -- keep polling.
            if terminal then
                log("FATAL version guard: " .. msg)
                log("Refusing to capture - not the expected USA SCUS_942.54 build.")
                loaded_at = -1
            elseif (vsync % 60) == 0 then
                log("waiting for SCUS: " .. msg)
            end
            return
        end
        log("version guard: " .. msg)
        log(string.format("baseline snapshot: flag window 0x%X bytes, %d inv slots",
            FLAG_BYTES, INV_SLOTS))
        if POINT_CARD_MAX then
            log(string.format("cruise booster ON: Point Card counter 0x%08X "
                .. "pinned at %d every vsync (use item 0xFE to nuke bosses)",
                POINT_CARD_ADDR, POINT_CARD_CAP))
        end
        log("polling under fast core - play as far as you like")
    end

    -- Scene + mode transition rows (context timeline).
    local sc = scene_name()
    if sc ~= last_scene then
        last_scene = sc
        totals.scene = totals.scene + 1
        CSV:row("%d,scene,0,0,0,0x%02X,%s,%d",
            vsync, u8(GAME_MODE), sc, totals.scene)
        log(string.format("scene -> %s (tick %d)", sc, vsync))
    end
    local md = u8(GAME_MODE)
    if md ~= last_mode then
        last_mode = md
        totals.mode = totals.mode + 1
        CSV:row("%d,mode,%d,%d,0,0x%02X,%s,%d", vsync, md, md, md, sc, totals.mode)
    end

    -- The whole point: diff every progression cell against last frame.
    snapshot_and_diff()

    -- Cruise booster: re-top the Point Card counter every vsync while active.
    -- Lua pokes bypass the CPU, so this touches no CSV cell.
    if POINT_CARD_MAX then
        mem.write_u16(POINT_CARD_ADDR,     POINT_CARD_CAP % 0x10000)
        mem.write_u16(POINT_CARD_ADDR + 2, math.floor(POINT_CARD_CAP / 0x10000))
    end

    -- Heartbeat every ~8s.
    if (vsync % 480) == 0 then
        log(string.format(
            "alive tick=%d mode=0x%02X scene=%s flags(set=%d clr=%d) item=%d gold=%d party=%d",
            vsync, md, sc, totals.flagset, totals.flagclr,
            totals.item, totals.gold, totals.party))
    end

    -- Rotating autosave (crash insurance).
    if AUTOSAVE_EVERY > 0 and (vsync % AUTOSAVE_EVERY) == 0 then
        autosave_flip = 1 - autosave_flip
        local path = AUTOSAVE_PATHS[autosave_flip + 1]
        if sstate.save(path) then
            log(string.format("autosaved -> %s (tick %d, scene=%s)", path, vsync, sc))
        end
    end
end

-- +-- startup ----------------------------------------------------------------
log("=== autorun_state_poll (fast-core progression capture) ===")
log("poll-diff: flags/battleid/gold/item/party/scene/mode - NO breakpoints")
log("run with run_probe.sh --fast; this session never self-quits (use timeout)")

-- Anchor the listener handle: a GC'd listener object silently deletes the
-- C++ listener (and GC mid-dispatch can segfault the emulator).
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
log("vsync listener installed; waiting for save load + version guard")
