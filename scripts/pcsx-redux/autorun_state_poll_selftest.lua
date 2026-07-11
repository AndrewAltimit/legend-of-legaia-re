-- autorun_state_poll_selftest.lua
--
-- REGRESSION SELF-TEST for the community poll probe. Loads the REAL
-- autorun_state_poll.lua (dofile, unmodified), then drives a scripted
-- poke sequence that exercises every diff stream + autosnap trigger the
-- probe advertises - no human at the pad, a couple of minutes end to end.
--
-- WHY: the poll probe is the community-handoff capture rig; an edit that
-- silently breaks one stream costs a volunteer's whole playthrough. This
-- wrapper is the committed form of the scratch poke-test that validated
-- the original battle/xp/equip/counter streams - the pattern is: Lua
-- pokes go straight to RAM (bypassing the CPU), so the probe's per-frame
-- diff sees a change while the game itself mostly doesn't react.
--
-- TWO PHASES:
--   A (field state, e.g. s3_rimelm_freeroam): flags (single set/clear,
--     bulk frame, target-flag autosnap), gold, item, party ids, level,
--     spell, xp, equip, counters, scene-name (never-seen-scene autosnap),
--     bgm, fmv, battle-id staging (+ autosnap), organic pos rows via a
--     pad-override walk, input edges, picker-choice row via a fabricated
--     picker struct in the verified-dead SCUS arena 0x8007AE00.
--   B (battle state, e.g. party_battle_gobu_gobu, loaded MID-RUN via
--     sstate.load): the load itself exercises the battle-entry latch
--     (`battle` row) + bulk-load flag tagging; then per-actor HP, status
--     (incl. the 0x400 first-set autosnap) and the party action-queue
--     window (dirs-only append -> artsin0 autosnap).
--   The phase-B full-state load wipes every phase-A poke, so the test
--   cannot leave corrupted RAM behind; nothing is written to a memory
--   card. Snapshots written by the run are test artifacts in the run dir.
--
-- LAUNCH (one command; resolves states, runs, checks):
--   bash scripts/pcsx-redux/run_state_poll_selftest.sh
-- or by hand:
--   LEGAIA_SNAP_FLAGS=4001 \
--   LEGAIA_SELFTEST_BATTLE_SSTATE=<battle .sstate> \
--   timeout --kill-after=15s 300s \
--   bash scripts/pcsx-redux/run_probe.sh --fast \
--     --scenario s3_rimelm_freeroam \
--     --lua scripts/pcsx-redux/autorun_state_poll_selftest.lua
-- then: python3 scripts/pcsx-redux/check_state_poll_selftest.py <run dir>
--
-- LEGAIA_SNAP_FLAGS=4001 keeps the target-flag autosnap test on a flag
-- retail never uses (idx 4001, byte 500 of the bank - inside the pure
-- story-flag window, above every observed retail flag) instead of poking
-- a real spine gate like 0x482.
--
-- The wrapper never self-quits (the emulator stays up); it logs
-- "STATE_POLL_SELFTEST COMPLETE" repeatedly once done so the runner can
-- kill the session, and keeps a >=520-frame tail after the last poke so
-- a probe heartbeat (every 480) has flushed the CSV first.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe  = require("probe")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bit    = require("bit")

-- Load the REAL probe (installs its own vsync listener first, so each
-- selftest poke is observed by the probe on the following vsync).
dofile("scripts/pcsx-redux/autorun_state_poll.lua")

-- +-- addresses (mirror of the poll probe's map; keep in sync) --------------
local GAME_MODE   = 0x8007B83C
local SCENE_NAME  = 0x8007050C
local BATTLE_ID   = 0x8007B7FC
local FLAG_BASE   = 0x80085758
local GOLD        = 0x8008459C
local PARTY_IDS   = 0x80084598
local INV_BASE    = 0x80085958
local CHAR_BASE   = 0x80084708
local CHAR_STRIDE = 0x414
local LEVEL_OFF   = 0x130
local SPELL_OFF   = 0x13C
local XP_OFF      = 0x0
local EQUIP_OFF   = 0x196
local FISH_PTS    = 0x8008444C
local BGM_ID      = 0x8007BAC8
local FMV_ID      = 0x8007BA78
local PLAYER_PTR  = 0x8007C364
local PICKER_PTR  = 0x801C6EA4
local ACTOR_TABLE = 0x801C9370
local HP_OFF      = 0x14C
local STATUS_OFF  = 0x16E
local Q_OFF       = 0x1DF
local Q_LEN       = 0x14
-- Verified-dead SCUS scratch (randomizer arena1: all-zero AND outside every
-- live indexed table across battle states) - hosts the fabricated picker
-- struct for the `pick` row test.
local ARENA       = 0x8007AE00

-- Single test flag idx 4000 and the runner-provided snap target 4001 both
-- live in bank byte 500 (0x1F4): inside the pure story-flag window, above
-- every retail-used flag.
local FLAG_TEST_BYTE = FLAG_BASE + 500
-- Bulk frame: 4 bytes = 32 flag flips >= the probe's default BULK_FLAGS=20.
local FLAG_BULK_ADDR = FLAG_BASE + 0x1E0
local FLAG_BULK_LEN  = 4

local BATTLE_SSTATE = probe.getenv("LEGAIA_SELFTEST_BATTLE_SSTATE", "")

-- +-- helpers ----------------------------------------------------------------
local function log(s) PCSX.log("[selftest] " .. s) end
local function u8(a)  return mem.read_u8(a)  or 0 end
local function u16(a) return mem.read_u16(a) or 0 end
local function u32(a) return mem.read_u32(a) or 0 end
local function w8(a, v)  mem.write_u8(a, v) end
local function w16(a, v) mem.write_u16(a, v) end
local function w32(a, v)
    mem.write_u16(a, v % 0x10000)
    mem.write_u16(a + 2, math.floor(v / 0x10000))
end

local saved = {}  -- per-step saved originals for restore closures

-- +-- step table --------------------------------------------------------------
-- Each step = { name, fn }; steps run STEP_GAP frames apart once the probe
-- has baselined (manifest.txt appears in the run dir). Pokes and their
-- restores are separate steps so the probe sees each edge in isolation.
local STEP_GAP = 12
local steps = {}
local function step(name, fn) steps[#steps + 1] = { name = name, fn = fn } end
local function wait(frames) steps[#steps + 1] = { name = "wait", gap = frames,
                                                  fn = function() end } end

-- ---- phase A: field-state streams ----
step("flagset_single", function()
    saved.flagbyte = u8(FLAG_TEST_BYTE)
    w8(FLAG_TEST_BYTE, bit.bor(saved.flagbyte, 0x80))     -- flag idx 4000 SET
end)
step("flagclr_single", function()
    w8(FLAG_TEST_BYTE, saved.flagbyte)                    -- flag idx 4000 CLEAR
end)
step("flag_bulk_set", function()
    saved.bulk = {}
    for i = 0, FLAG_BULK_LEN - 1 do
        saved.bulk[i] = u8(FLAG_BULK_ADDR + i)
        w8(FLAG_BULK_ADDR + i, 0xFF)
    end
end)
step("flag_bulk_restore", function()
    for i = 0, FLAG_BULK_LEN - 1 do w8(FLAG_BULK_ADDR + i, saved.bulk[i]) end
end)
step("flag_snap_target", function()
    -- idx 4001 (mask 0x40 in byte 500): the runner passes
    -- LEGAIA_SNAP_FLAGS=4001 so this SET must autosnap "flag4001".
    saved.flagbyte2 = u8(FLAG_TEST_BYTE)
    w8(FLAG_TEST_BYTE, bit.bor(saved.flagbyte2, 0x40))
end)
step("flag_snap_target_clear", function()
    w8(FLAG_TEST_BYTE, saved.flagbyte2)
end)
step("gold_poke", function()
    saved.gold = u32(GOLD)
    w32(GOLD, saved.gold + 100)
end)
step("gold_restore", function() w32(GOLD, saved.gold) end)
step("item_poke", function()
    saved.item_id, saved.item_ct = u8(INV_BASE), u8(INV_BASE + 1)
    if saved.item_id == 0 then
        w8(INV_BASE, 0x01); w8(INV_BASE + 1, 1)
    else
        w8(INV_BASE + 1, math.min(saved.item_ct + 1, 255))
    end
end)
step("item_restore", function()
    w8(INV_BASE, saved.item_id); w8(INV_BASE + 1, saved.item_ct)
end)
step("party_poke", function()
    -- 4th member-id byte: diffed via the ids string, unread while count < 4.
    saved.pid3 = u8(PARTY_IDS + 3)
    w8(PARTY_IDS + 3, 0x7F)
end)
step("party_restore", function() w8(PARTY_IDS + 3, saved.pid3) end)
step("level_poke", function()
    saved.level = u8(CHAR_BASE + LEVEL_OFF)
    w8(CHAR_BASE + LEVEL_OFF, math.min(saved.level + 1, 99))
end)
step("level_restore", function() w8(CHAR_BASE + LEVEL_OFF, saved.level) end)
step("spell_poke", function()
    local c = u8(CHAR_BASE + SPELL_OFF)
    saved.spell_c = c
    if c < 36 then
        saved.spell_id = u8(CHAR_BASE + SPELL_OFF + 1 + c)
        saved.spell_lv = u8(CHAR_BASE + SPELL_OFF + 0x25 + c)
        w8(CHAR_BASE + SPELL_OFF + 1 + c, 0x42)
        w8(CHAR_BASE + SPELL_OFF + 0x25 + c, 1)
        w8(CHAR_BASE + SPELL_OFF, c + 1)
    end
end)
step("spell_restore", function()
    if saved.spell_c ~= nil and saved.spell_c < 36 then
        w8(CHAR_BASE + SPELL_OFF, saved.spell_c)
        w8(CHAR_BASE + SPELL_OFF + 1 + saved.spell_c, saved.spell_id)
        w8(CHAR_BASE + SPELL_OFF + 0x25 + saved.spell_c, saved.spell_lv)
    end
end)
step("xp_poke", function()
    saved.xp = u32(CHAR_BASE + XP_OFF)
    w32(CHAR_BASE + XP_OFF, saved.xp + 10)
end)
step("xp_restore", function() w32(CHAR_BASE + XP_OFF, saved.xp) end)
step("equip_poke", function()
    saved.equip = u8(CHAR_BASE + EQUIP_OFF)
    w8(CHAR_BASE + EQUIP_OFF, bit.band(saved.equip + 1, 0xFF))
end)
step("equip_restore", function() w8(CHAR_BASE + EQUIP_OFF, saved.equip) end)
step("counter_poke", function()
    saved.fish = u32(FISH_PTS)
    w32(FISH_PTS, saved.fish + 5)
end)
step("counter_restore", function() w32(FISH_PTS, saved.fish) end)
step("bgm_poke", function()
    saved.bgm = u16(BGM_ID)
    w16(BGM_ID, saved.bgm + 1)
end)
step("bgm_restore", function() w16(BGM_ID, saved.bgm) end)
step("fmv_poke", function()
    saved.fmv = u16(FMV_ID)
    w16(FMV_ID, 3)
end)
step("fmv_restore", function() w16(FMV_ID, saved.fmv) end)

-- pos: organic walk via pad override (the locomotion controller re-derives
-- the position from real input, so a direct poke can be overwritten; a
-- forced walk crosses tiles the same way a volunteer does).
step("walk_up", function() pad.force(pad.BTN.UP) end)
wait(90)
step("walk_up_release", function() pad.release(pad.BTN.UP) end)
step("walk_down", function() pad.force(pad.BTN.DOWN) end)
wait(90)
step("walk_down_release", function() pad.release(pad.BTN.DOWN) end)

-- pick: fabricate a picker struct in the dead arena, point the picker
-- global at it, press a confirm button via pad override.
step("pick_setup", function()
    saved.picker = u32(PICKER_PTR)
    for i = 0, 0x0F do w8(ARENA + i, 0) end
    w16(ARENA + 0x0C, 2)                     -- cursor index = 2 (< 64)
    w32(PICKER_PTR, ARENA)
    pad.force(pad.BTN.CROSS)
end)
wait(10)
step("pick_teardown", function()
    pad.release(pad.BTN.CROSS)
    w32(PICKER_PTR, saved.picker)
    for i = 0, 0x0F do w8(ARENA + i, 0) end
end)

-- scene: a never-seen name must emit a scene row AND autosnap scene_zztest.
step("scene_poke", function()
    saved.scene = {}
    for i = 0, 7 do saved.scene[i] = u8(SCENE_NAME + i) end
    local name = "zztest"
    for i = 0, 7 do
        local c = name:byte(i + 1)
        w8(SCENE_NAME + i, c or 0)
    end
end)
wait(10)
step("scene_restore", function()
    for i = 0, 7 do w8(SCENE_NAME + i, saved.scene[i]) end
end)

-- battle-id staging byte: nonzero must row + autosnap batid01. 0x01 is far
-- outside the lone-monster consumer band (0x49..0x4D). Last phase-A step so
-- the phase-B state load wipes any residue.
step("battleid_poke", function() w8(BATTLE_ID, 0x01) end)
step("battleid_restore", function() w8(BATTLE_ID, 0x00) end)

-- ---- phase B: battle-state streams ----
step("load_battle_state", function()
    if BATTLE_SSTATE == "" then
        log("NO LEGAIA_SELFTEST_BATTLE_SSTATE - skipping phase B (battle streams UNTESTED)")
        return
    end
    if not sstate.load(BATTLE_SSTATE) then
        log("FAILED to load battle state: " .. BATTLE_SSTATE)
    else
        log("battle state loaded: " .. BATTLE_SSTATE)
    end
end)
wait(180)  -- let the probe latch the battle + settle its actor baselines

local function actor0()
    local ptr = u32(ACTOR_TABLE)
    local off = mem.ram_offset(ptr)
    if off == nil or off < 0x10000 then return nil end
    return ptr
end
step("hp_poke", function()
    local a = actor0(); if a == nil then return end
    saved.hp = u16(a + HP_OFF)
    w16(a + HP_OFF, math.max(saved.hp - 7, 1))
end)
step("hp_restore", function()
    local a = actor0(); if a == nil or saved.hp == nil then return end
    w16(a + HP_OFF, saved.hp)
end)
step("status_poke_venom", function()
    local a = actor0(); if a == nil then return end
    saved.status = u16(a + STATUS_OFF)
    w16(a + STATUS_OFF, bit.bor(saved.status, 0x0001))
end)
step("status_poke_400", function()
    -- first 0x400 raise of the run: must row AND autosnap status400.
    local a = actor0(); if a == nil then return end
    w16(a + STATUS_OFF, bit.bor(u16(a + STATUS_OFF), 0x0400))
end)
step("status_restore", function()
    local a = actor0(); if a == nil or saved.status == nil then return end
    w16(a + STATUS_OFF, saved.status)
end)
step("aq_poke", function()
    -- append two pure-direction bytes at the window tail: aq row with a
    -- dirs-only change -> the artsin0 autosnap bracket.
    local a = actor0(); if a == nil then return end
    saved.aq1 = u8(a + Q_OFF + Q_LEN - 2)
    saved.aq2 = u8(a + Q_OFF + Q_LEN - 1)
    w8(a + Q_OFF + Q_LEN - 2, 0x0C)
    w8(a + Q_OFF + Q_LEN - 1, 0x0D)
end)
step("aq_restore", function()
    local a = actor0(); if a == nil or saved.aq1 == nil then return end
    w8(a + Q_OFF + Q_LEN - 2, saved.aq1)
    w8(a + Q_OFF + Q_LEN - 1, saved.aq2)
end)

-- +-- driver -------------------------------------------------------------------
local manifest_path = probe.out_path("manifest.txt")
local tick        = 0
local baseline_at = nil   -- tick when manifest.txt appeared (+settle)
local step_i      = 1
local next_at     = nil
local done_at     = nil

local function manifest_exists()
    local fh = io.open(manifest_path, "r")
    if fh then fh:close(); return true end
    return false
end

local function on_vsync()
    tick = tick + 1
    if baseline_at == nil then
        -- Poll cheaply (every 30 frames) for the probe's baseline manifest.
        if (tick % 30) == 0 and manifest_exists() then
            baseline_at = tick
            next_at = tick + 60   -- settle margin after baseline
            log(string.format("probe baselined (manifest at tick %d); %d steps queued",
                tick, #steps))
        end
        return
    end
    if done_at ~= nil then
        -- Repeat the completion marker so the runner's log-poll can't miss it.
        if tick >= done_at and ((tick - done_at) % 60) == 0 then
            log("STATE_POLL_SELFTEST COMPLETE")
        end
        return
    end
    if step_i <= #steps and tick >= next_at then
        local s = steps[step_i]
        if s.name ~= "wait" then log(string.format("step %d/%d: %s", step_i, #steps, s.name)) end
        local ok, err = pcall(s.fn)
        if not ok then log("STEP ERROR " .. s.name .. ": " .. tostring(err)) end
        next_at = tick + (s.gap or STEP_GAP)
        step_i = step_i + 1
        if step_i > #steps then
            -- >=520-frame tail: a probe heartbeat (every 480) flushes the CSV
            -- before the runner sees the marker and kills the session.
            done_at = tick + 520
            log(string.format("all steps done at tick %d; flush tail until %d",
                tick, done_at))
        end
    end
end

log("=== state_poll SELF-TEST wrapper ===")
log(string.format("%d steps; battle sstate = %s", #steps,
    BATTLE_SSTATE ~= "" and BATTLE_SSTATE or "(none - phase B skipped)"))

PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
