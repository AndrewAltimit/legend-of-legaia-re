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
--   battle           ONE row per battle, emitted on the field->battle mode
--                    edge: the formation table 0x8007BD0C[4] (first-monster
--                    ids, sampled once the battle scene is active so they are
--                    the NEW battle's) identifies boss vs random, plus a
--                    best-effort read of the 0x8007B7FC staging id (idx col;
--                    usually 0 because it is written+consumed sub-vsync - that
--                    is exactly why battleid diffs come up empty, and why the
--                    writer needs the exec-bp firehose, not this poll)
--   gold             0x8008459C party gold (with delta)
--   item             0x80085958 inventory: id/count changes (with delta) -
--                    consumables AND the start of the key-item page, so
--                    quest-item grants land too
--   party            0x80084594 count + 0x80084598 member-id list
--   level            per-roster-slot displayed-level byte (char record
--                    0x80084708 + slot*0x414, +0x130) - level-up beats
--   spell            per-roster-slot Seru-magic list (record +0x13C count +
--                    +0x13D ids + +0x161 levels) - a count bump = a Seru
--                    capture grant; a level byte bump = a spell level-up.
--                    Offsets are the capture-pinned ones in
--                    crates/engine-core/src/capture_observations/seru_capture.rs
--   scene / mode     0x8007050C name + 0x8007B83C mode transitions
--   pos    (P1)      player tile (col idx=tileX, value=tileZ) emitted on a tile
--                    crossing while in field mode; turns "flag X flipped in
--                    scene Y" into "...at tile T" for door/trigger attribution
--                    with no second pass. Raw world XZ in the note.
--   bgm    (P5)      global BGM id (0x8007BAC8) on change - finishes the
--                    music_labels sound-test census join on any run.
--   input  (P4)      pad press/release edges (0x8007B850); idx=button bit,
--                    value=1 press / 0 release, note=button name.
--   pick   (P4)      dialogue picker cursor index at a confirm press
--                    (*(0x801C6EA4)+0x0C) - the branch/answer chosen.
--   snap   (P2)      an auto-snapshot save state was written (rare-event
--                    harvest: never-seen scene / lone-boss formation / a
--                    first-time target-set flag). note = reason + filename.
--
-- P3 (bulk-load tagging): a frame that flips >= LEGAIA_BULK_FLAGS story flags
--   (a save-load / scene-init dump, not a beat) tags every one of its flag rows
--   note=bulkload, so analyze_state_poll.py auto-filters the noise that
--   otherwise buries the organic in-scene sets carrying the play-order signal.
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
local FORMATION  = 0x8007BD0C  -- DAT_8007bd0c[4] live battle formation (first-monster ids)
local FLAG_BASE  = 0x80085758  -- fourth flag bank; idx 0 == firehose value 0
local GOLD       = 0x8008459C  -- u32 party gold
local PARTY_CNT  = 0x80084594  -- u8 party member count
local PARTY_IDS  = 0x80084598  -- u8[4] member ids
local INV_BASE   = 0x80085958  -- inventory (id,count) 2-byte stride
-- Character records (roster slots 0..3; slot-3 tail ends exactly at FLAG_BASE).
local CHAR_BASE   = 0x80084708
local CHAR_STRIDE = 0x414
local CHAR_SLOTS  = 4
local LEVEL_OFF   = 0x130      -- displayed-level byte (rank counter)
-- Spell window: +0x13C count u8, +0x13D..0x160 id array, +0x161..0x184 levels.
local SPELL_OFF   = 0x13C
local SPELL_LEN   = 0x49       -- count + 36 ids + 36 levels
-- P1: player-position / tile. Player actor pointer global; the live struct
-- stores world X at +0x14 and world Z at +0x18 (both s16). tile = (pos-0x40)>>7
-- (the camera cluster FUN_801dbec4 derives the player tile the same way). Only
-- valid in field mode (0x03); the pointer is stale/garbage otherwise.
local PLAYER_PTR  = 0x8007C364
local POS_X_OFF   = 0x14
local POS_Z_OFF   = 0x18
local FIELD_MODE  = 0x03
-- P5: global BGM id (field-VM op 0x35 sub-1 target; <2000 scene-local, >=2000
-- global pool). Emitted on change for the music_labels census join.
local BGM_ID      = 0x8007BAC8  -- u16
-- P4: per-frame held-pad mask (game-decoded; bit layout == probe.pad.BTN) and
-- the dialogue picker struct pointer (cursor index at +0x0C).
local HELD_PAD    = 0x8007B850  -- u16
local PICKER_PTR  = 0x801C6EA4  -- *(PICKER_PTR)+0x0C = picker cursor index
local PICKER_CUR  = 0x0C
local BTN_NAME = { [0]="SELECT", [1]="L3", [2]="R3", [3]="START",
                   [4]="UP", [5]="RIGHT", [6]="DOWN", [7]="LEFT",
                   [8]="L2", [9]="R2", [10]="L1", [11]="R1",
                   [12]="TRIANGLE", [13]="CIRCLE", [14]="CROSS", [15]="SQUARE" }
-- Confirm-button bits sampled for the picker-choice column.
local CONFIRM_BITS = { [13]=true, [14]=true }  -- CIRCLE / CROSS

-- Game-mode brackets for the per-battle identity row. BATTLE_MODES is the broad
-- "a battle is loading/active" set that latches the in-battle state (so we emit
-- exactly one row per fight); BATTLE_ACTIVE is the subset where the battle scene
-- is fully up, so the formation table 0x8007BD0C holds THIS battle's ids (not the
-- previous one's, which persists across the 0x08/0x09 load shims).
local BATTLE_MODES  = { [0x08]=true, [0x09]=true, [0x14]=true, [0x15]=true,
                        [0x16]=true, [0x17]=true }
local BATTLE_ACTIVE = { [0x14]=true, [0x15]=true, [0x16]=true, [0x17]=true }

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

-- +-- enhancement toggles (P1..P4) -------------------------------------------
-- All default ON so a single volunteer playthrough harvests the most; a
-- volunteer who wants the leanest CSV can switch any off.
local TRACE_POS   = probe.getenv("LEGAIA_TRACE_POS", "1")   ~= "0"  -- P1
local TRACE_BGM   = probe.getenv("LEGAIA_TRACE_BGM", "1")   ~= "0"  -- P5
local TRACE_INPUT = probe.getenv("LEGAIA_TRACE_INPUT", "1") ~= "0"  -- P4
-- P3: a frame flipping >= this many flags is a bulk load/init dump, not a
-- story beat. Matches analyze_state_poll.py's DEFAULT_BULK_THRESHOLD.
local BULK_FLAGS  = probe.getenv_num("LEGAIA_BULK_FLAGS", 20)
-- P2: auto-snapshot on rare events. Capped so disk stays bounded; the CSV is
-- always the prize, snapshots are a free bonus harvest. AUTOSNAP off => no
-- state files written (a volunteer short on disk/upload can disable).
local AUTOSNAP     = probe.getenv("LEGAIA_AUTOSNAP", "1") ~= "0"
local SNAP_MAX     = probe.getenv_num("LEGAIA_SNAP_MAX", 40)
-- Target flag set for the first-time-flag snapshot trigger: the known
-- spine/story gates (mid-beat brackets around these are what future sessions
-- keep wishing for). Comma list of decimal idxs overrides the default.
local SNAP_FLAGS = {}
do
    local spec = probe.getenv("LEGAIA_SNAP_FLAGS", "")
    if spec == "" then
        -- 0x142=322 (Caruban gate), 0x225=549 (Rim Elm one-shot),
        -- 0x482=1154 (other7 mist walls), 0x1BE=446 (geremi arrival).
        for _, f in ipairs({ 322, 549, 1154, 446 }) do SNAP_FLAGS[f] = true end
    else
        for tok in spec:gmatch("[^,]+") do
            local n = tonumber(tok)
            if n then SNAP_FLAGS[n] = true end
        end
    end
end

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
local prev_level = {}        -- per roster slot: level byte
local prev_spell = {}        -- per roster slot: SPELL_LEN-byte window string
local prev_tilex = nil       -- P1: last emitted player tile X
local prev_tilez = nil       -- P1: last emitted player tile Z
local prev_bgm   = nil       -- P5: last global BGM id
local prev_pad   = 0         -- P4: last held-pad mask
-- P2 rare-event bookkeeping.
local seen_scenes = {}       -- scenes visited this run (never-seen -> snap)
local snap_flags  = {}       -- target flags already snapped this run
local snap_count  = 0        -- snapshots written (capped at SNAP_MAX)
-- Per-battle identity latch (see BATTLE_MODES above).
local in_battle       = false  -- latched while mode is in a battle bracket
local batt_pending    = false  -- a `battle` row is still owed for this fight
local batt_batid      = 0      -- staging id captured this fight (best-effort)
local batt_enter_mode = 0      -- the mode that started the fight
local totals     = { flagset = 0, flagclr = 0, battleid = 0, battle = 0,
                     gold = 0, item = 0, party = 0, scene = 0, mode = 0,
                     level = 0, spell = 0, pos = 0, bgm = 0, input = 0,
                     pick = 0, snap = 0 }

local function row(kind, idx, value, delta, note)
    totals[kind] = (totals[kind] or 0) + 1
    CSV:row("%d,%s,%d,%d,%d,0x%02X,%s,%s",
        vsync, kind, idx, value, delta, u8(GAME_MODE), scene_name(),
        note or "")
end

-- P2: write a fingerprinted full save state on a rare event so future sessions
-- can harvest a mid-beat bracket for free. Capped at SNAP_MAX; a `snap` CSV row
-- records the reason + filename. Follows the autosave out_path convention.
local function autosnap(reason)
    if not AUTOSNAP or snap_count >= SNAP_MAX then return end
    local sc = scene_name()
    local fname = string.format("snap_%07d_%s_%s.sstate", vsync, reason, sc)
    local path = probe.out_path(fname)
    if sstate.save(path) then
        snap_count = snap_count + 1
        row("snap", snap_count, 0, 0, reason .. " -> " .. fname)
        log(string.format("AUTOSNAP #%d/%d: %s (tick %d scene %s) -> %s",
            snap_count, SNAP_MAX, reason, vsync, sc, fname))
    end
end

-- Emit the one-per-battle identity row: idx = best-effort staging id, value =
-- formation[0] (the first-monster / lone-boss id), note = the full 4-id
-- formation + the mode the fight started in. Cleared once emitted.
local function emit_battle()
    local f0 = u8(FORMATION)
    local f1 = u8(FORMATION + 1)
    local f2 = u8(FORMATION + 2)
    local f3 = u8(FORMATION + 3)
    row("battle", batt_batid, f0, 0,
        string.format("form=%02X%02X%02X%02X enter=0x%02X",
            f0, f1, f2, f3, batt_enter_mode))
    batt_pending = false
    -- Lone non-zero formation slot = a solo enemy = almost always a scripted
    -- boss (or a solo-strong random) - snapshot it.
    if f0 ~= 0 and f1 == 0 and f2 == 0 and f3 == 0 then
        autosnap(string.format("boss%02X", f0))
    end
end

-- +-- diffs ------------------------------------------------------------------

-- Flag bank: XOR each changed byte; each flipped bit -> one flag row.
-- Bit convention MATCHES FUN_8003CE08: byte = base + (idx>>3),
-- mask = 0x80 >> (idx & 7). So within a byte, bit position p (0=LSB..7=MSB)
-- maps to idx&7 = 7 - p, and idx = byte_index*8 + (7 - p).
-- Collect this frame's flag flips into a list {idx, set} WITHOUT emitting, so
-- the caller can count them first (P3: a >= BULK_FLAGS frame is a save-load /
-- init dump and every row is tagged note=bulkload; and P2: first-time target
-- flags only snapshot on non-bulk frames).
local function collect_flag_flips(cur)
    local events = {}
    if prev_flags == nil then return events end
    for i = 1, #cur do
        local a = prev_flags:byte(i)
        local b = cur:byte(i)
        if a ~= b then
            local x = bit.bxor(a, b)
            for p = 0, 7 do
                if bit.band(x, bit.lshift(1, p)) ~= 0 then
                    local idx = (i - 1) * 8 + (7 - p)
                    local nowset = bit.band(b, bit.lshift(1, p)) ~= 0
                    events[#events + 1] = { idx = idx, set = nowset }
                end
            end
        end
    end
    return events
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

-- Per-roster-slot level + spell-list diff. Level-ups and spell/Seru grants
-- are rare, so one row per slot per changed frame stays quiet in the CSV.
local function diff_chars()
    for s = 0, CHAR_SLOTS - 1 do
        local rec = CHAR_BASE + s * CHAR_STRIDE

        local lvl = u8(rec + LEVEL_OFF)
        if baselined and prev_level[s] ~= nil and lvl ~= prev_level[s] then
            row("level", s, lvl, lvl - prev_level[s])
        end
        prev_level[s] = lvl

        local win = mem.read_bytes(rec + SPELL_OFF, SPELL_LEN)
        if win ~= nil then
            win = tostring(win)
            if baselined and prev_spell[s] ~= nil and win ~= prev_spell[s] then
                local cnt  = win:byte(1)
                local pcnt = prev_spell[s]:byte(1)
                -- note = the first `cnt` spell ids + their levels (both small)
                local n = math.min(cnt, 36)
                local ids, lvs = "", ""
                for i = 1, n do
                    ids = ids .. string.format("%02X", win:byte(1 + i))
                    lvs = lvs .. string.format("%02X", win:byte(1 + 36 + i))
                end
                row("spell", s, cnt, cnt - pcnt,
                    string.format("ids=%s lv=%s", ids, lvs))
            end
            prev_spell[s] = win
        end
    end
end

-- P1: player tile. Only meaningful in field mode (the pointer is stale/garbage
-- in battle/menu/world modes); emit a `pos` row on a tile crossing so the CSV
-- stays small (idle standing writes nothing). idx=tileX value=tileZ; note holds
-- the raw signed world XZ. tile = (pos-0x40)>>7 (arithmetic shift; s16 pos).
local function s16(v) return (v >= 0x8000) and (v - 0x10000) or v end
local function tile_of(pos) return math.floor((pos - 0x40) / 128) end
local function diff_pos()
    if not TRACE_POS then return end
    if u8(GAME_MODE) ~= FIELD_MODE then return end
    local ptr = u32(PLAYER_PTR)
    -- Reject a null/low pointer: it would still pass in_ram (offset 0x18) and
    -- read garbage out of low RAM. A live actor sits well above the first 64K.
    local off = mem.ram_offset(ptr)
    if off == nil or off < 0x10000 then return end
    if not mem.in_ram(ptr + POS_Z_OFF, 2) then return end
    local x = s16(u16(ptr + POS_X_OFF))
    local z = s16(u16(ptr + POS_Z_OFF))
    local tx, tz = tile_of(x), tile_of(z)
    if tx ~= prev_tilex or tz ~= prev_tilez then
        row("pos", tx, tz, 0, string.format("x=%d z=%d", x, z))
        prev_tilex, prev_tilez = tx, tz
    end
end

-- P5: global BGM id on change.
local function diff_bgm()
    if not TRACE_BGM then return end
    local id = u16(BGM_ID)
    if baselined and id ~= prev_bgm then
        row("bgm", 0, id, id - (prev_bgm or 0))
    end
    prev_bgm = id
end

-- P4: pad press/release edges + picker-choice at a confirm press. The held mask
-- is the game-decoded per-frame button word (bit layout == probe.pad.BTN).
local function read_picker_cursor()
    local ptr = u32(PICKER_PTR)
    local off = mem.ram_offset(ptr)
    if off == nil or off < 0x10000 then return nil end
    if not mem.in_ram(ptr + PICKER_CUR, 2) then return nil end
    local cur = u16(ptr + PICKER_CUR)
    -- Reject stale/garbage: a real menu cursor is small. Filters the common
    -- case where confirm is pressed in the field with no picker open.
    if cur >= 64 then return nil end
    return cur
end
local function diff_input()
    if not TRACE_INPUT then return end
    local pad = u16(HELD_PAD)
    if baselined and pad ~= prev_pad then
        local changed = bit.bxor(pad, prev_pad)
        for b = 0, 15 do
            local mask = bit.lshift(1, b)
            if bit.band(changed, mask) ~= 0 then
                local nowdown = bit.band(pad, mask) ~= 0
                row("input", b, nowdown and 1 or 0, 0, BTN_NAME[b] or "?")
                -- On a confirm press, sample the dialogue picker cursor.
                if nowdown and CONFIRM_BITS[b] then
                    local cur = read_picker_cursor()
                    if cur ~= nil then
                        row("pick", cur, cur, 0, BTN_NAME[b])
                    end
                end
            end
        end
    end
    prev_pad = pad
end

local function snapshot_and_diff()
    -- Flag bank. Collect flips first so the frame can be classified bulk vs
    -- beat (P3) before any row is written, and target-flag snapshots (P2) are
    -- suppressed on bulk frames.
    local flags = mem.read_bytes(FLAG_BASE, FLAG_BYTES)
    if flags ~= nil then
        flags = tostring(flags)
        if baselined then
            local events = collect_flag_flips(flags)
            local bulk = #events >= BULK_FLAGS
            local note = bulk and "bulkload" or ""
            for _, e in ipairs(events) do
                if e.set then
                    row("flagset", e.idx, 1, 1, note)
                else
                    row("flagclr", e.idx, 0, -1, note)
                end
            end
            -- P2: first-time target-set flag (real beats only, not load dumps).
            if not bulk then
                for _, e in ipairs(events) do
                    if e.set and SNAP_FLAGS[e.idx] and not snap_flags[e.idx] then
                        snap_flags[e.idx] = true
                        autosnap(string.format("flag%d", e.idx))
                    end
                end
            end
        end
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

    -- Per-character level + spell/Seru list
    diff_chars()

    -- P1/P5/P4: player tile, BGM id, input edges + picker choice.
    diff_pos()
    diff_bgm()
    diff_input()

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
        -- Run manifest: config + source sstate, so this run dir stays
        -- interpretable later and chains to the state it resumed from.
        probe.write_manifest("autorun_state_poll.lua", {
            sstate         = NO_SSTATE and "(hand-loaded card save)" or SSTATE,
            flag_window    = string.format("0x%X", FLAG_BYTES),
            inv_slots      = tostring(INV_SLOTS),
            trace_pos      = tostring(TRACE_POS),
            trace_bgm      = tostring(TRACE_BGM),
            trace_input    = tostring(TRACE_INPUT),
            bulk_flags     = tostring(BULK_FLAGS),
            autosnap       = string.format("%s (max %d)", tostring(AUTOSNAP), SNAP_MAX),
            point_card_max = tostring(POINT_CARD_MAX),
            autosave_every = tostring(AUTOSAVE_EVERY),
            baseline_tick  = tostring(vsync),
            baseline_scene = scene_name(),
            core           = probe.getenv("LEGAIA_CORE",
                "unknown (no LEGAIA_CORE; hand launch)"),
        })
    end

    -- Scene + mode transition rows (context timeline).
    local sc = scene_name()
    if sc ~= last_scene then
        last_scene = sc
        totals.scene = totals.scene + 1
        CSV:row("%d,scene,0,0,0,0x%02X,%s,%d",
            vsync, u8(GAME_MODE), sc, totals.scene)
        log(string.format("scene -> %s (tick %d)", sc, vsync))
        -- P1: a scene change is a warp, not a walk - drop the tile baseline so
        -- the first field frame in the new scene doesn't emit a phantom
        -- crossing between the two scenes' coordinate frames.
        prev_tilex, prev_tilez = nil, nil
        -- P2: snapshot the first time we ever enter a scene this run (a fresh
        -- bracket at the mouth of every new area - the highest-value harvest).
        if sc ~= "?" and not seen_scenes[sc] then
            seen_scenes[sc] = true
            autosnap("scene_" .. sc)
        end
    end
    local md = u8(GAME_MODE)
    if md ~= last_mode then
        last_mode = md
        totals.mode = totals.mode + 1
        CSV:row("%d,mode,%d,%d,0,0x%02X,%s,%d", vsync, md, md, md, sc, totals.mode)
    end

    -- Per-battle identity: latch on the field->battle edge, emit one `battle`
    -- row once the scene is active (formation is this fight's). Fixes "which of
    -- N battles was the boss" (formation) + best-effort staging id.
    local inb = BATTLE_MODES[md] ~= nil
    if inb and not in_battle then
        in_battle       = true
        batt_pending    = true
        batt_enter_mode = md
        batt_batid      = u8(BATTLE_ID)   -- earliest shot at the staging byte
    elseif (not inb) and in_battle then
        in_battle = false
        if batt_pending then emit_battle() end  -- ended before an active mode
    end
    if in_battle then
        if batt_batid == 0 then
            local b = u8(BATTLE_ID)          -- keep watching in case it flickers up
            if b ~= 0 then batt_batid = b end
        end
        if batt_pending and BATTLE_ACTIVE[md] ~= nil then
            emit_battle()                     -- formation is current here
        end
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
            "alive tick=%d mode=0x%02X scene=%s flags(set=%d clr=%d) item=%d gold=%d party=%d pos=%d bgm=%d input=%d snap=%d",
            vsync, md, sc, totals.flagset, totals.flagclr,
            totals.item, totals.gold, totals.party,
            totals.pos, totals.bgm, totals.input, snap_count))
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
log("poll-diff: flags/battleid/gold/item/party/level/spell/scene/mode/pos/bgm/input - NO breakpoints")
log(string.format("enhancements: pos=%s bgm=%s input=%s autosnap=%s(max %d) bulk>=%d",
    TRACE_POS and "on" or "off", TRACE_BGM and "on" or "off",
    TRACE_INPUT and "on" or "off", AUTOSNAP and "on" or "off", SNAP_MAX, BULK_FLAGS))
log("run with run_probe.sh --fast; this session never self-quits (use timeout)")
if probe.getenv("LEGAIA_CORE", "") == "interpreter" then
    log("NOTE: launched on the interpreter core (no --fast). The poll works")
    log("  fine but runs ~10x slow; this probe arms NO breakpoints, so you")
    log("  almost certainly wanted --fast.")
end

-- Anchor the listener handle: a GC'd listener object silently deletes the
-- C++ listener (and GC mid-dispatch can segfault the emulator).
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
log("vsync listener installed; waiting for save load + version guard")
