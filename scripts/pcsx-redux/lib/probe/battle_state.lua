-- probe/battle_state.lua  -- typed battle-state extraction (addresses -> table).
--
-- The shared "read pinned battle addresses -> typed BattleState" layer.
-- Deliberately TRANSPORT-FREE: it only reads memory and returns a Lua
-- table. The diff/sweep emitter, the MIDI register encoder (VRChat diorama
-- PRD), and the UDP packet encoder (spectator-viewport PRD) all sit ON TOP
-- of this so neither delivery target forks the probe. See
--   the VRChat live battle-diorama design (PRD; secs 5.2, 11)
--
-- Address + field provenance: docs/subsystems/battle.md
--   - battle actor pointer table &DAT_801C9370 (8 slots: 0..2 party, 3..7 enemy)
--   - battle ctx pointer _DAT_8007BD24 -> ctx struct (action ids at +0x06/+0x07,
--     active slot at +0x13, turn/phase at +0x09)
--   - per-slot monster id array DAT_8007BD0C[ordinal] (FUN_801DA51C fills it
--     from the encounter record [3 reserved][count][ids])
--   - battle render/camera mode _DAT_8007B83C (0x15 = orbit) ; game-state
--     guard _DAT_800846C0 (battle SM requires != 2)
--   - actor fields: status word +0x00 (bit 0x2000 = confuse/charm), HP cur
--     +0x14C / max +0x172, MP cur +0x150 / max +0x174, AI-chosen action id
--     +0x1DF, world X/Z +0x34/+0x38, hit-radius/size +0x1F
--
-- Usage:
--   local bs = require("probe.battle_state")
--   local st = bs.read()              -- nil-safe; every field defaults to 0/false
--   if st.in_battle then ... end
--   local sig = bs.signature(st)      -- cheap change-detection key

local mem = require("probe.mem")

local M = {}

-- Pinned globals (KSEG0 virtual addresses; see provenance above).
M.ACTOR_ARRAY = 0x801C9370 -- &DAT_801C9370, 8 x u32 actor pointers
M.CTX_PTR     = 0x8007BD24 -- _DAT_8007BD24, pointer to the battle ctx struct
M.MONSTER_IDS = 0x8007BD0C -- DAT_8007BD0C, per-monster-ordinal id bytes
M.RENDER_MODE = 0x8007B83C -- _DAT_8007B83C, 0x15 during the battle orbit camera
M.GAME_STATE  = 0x800846C0 -- _DAT_800846C0, battle SM guards on != 2

M.SLOT_COUNT   = 8
M.PARTY_SLOTS  = 3 -- slots 0..2 are party members, 3..7 are enemies
M.MONSTER_SLOTS = 5

-- ctx struct byte offsets.
local CTX_MONSTER_ACTION = 0x06
local CTX_PARTY_ACTION   = 0x07
local CTX_TURN           = 0x09
local CTX_ACTIVE_SLOT    = 0x13

-- actor struct field offsets.
local A_STATUS  = 0x00
local A_SIZE    = 0x1F
local A_POS_X   = 0x34
local A_POS_Z   = 0x38
local A_HP      = 0x14C
local A_MP      = 0x150
local A_HP_MAX  = 0x172
local A_MP_MAX  = 0x174
local A_ACTION  = 0x1DF

local function s16(v)
    if v == nil then return 0 end
    if v >= 0x8000 then return v - 0x10000 end
    return v
end

local function u8(a)  return mem.read_u8(a) or 0 end
local function u16(a) return mem.read_u16(a) or 0 end
local function u32(a) return mem.read_u32(a) or 0 end

local function ok_ptr(p)
    return p ~= nil and p >= 0x80000000 and p < 0x80200000
end

-- A slot is "present" when in battle, its actor pointer is a real RAM address,
-- and its HP reads in the plausible combat range. The in-battle gate matters:
-- OUTSIDE battle the actor pointer table holds stale field-state pointers, so
-- HP alone yields bogus "enemies" (live-observed: a field-mode save reported
-- `316/30305`). `in_battle` is passed in so presence is never trusted when the
-- battle render mode is not active. (Scene-init also parks inactive actors at
-- the origin; the HP-range check filters those within a real battle.)
local function read_slot(slot, in_battle)
    local ptr = u32(M.ACTOR_ARRAY + slot * 4)
    local role = (slot < M.PARTY_SLOTS) and "party" or "enemy"
    local s = {
        slot = slot,
        role = role,
        ptr = ptr,
        present = false,
        hp = 0, hp_max = 0,
        mp = 0, mp_max = 0,
        action = 0,
        status = 0,
        x = 0, z = 0,
        size = 0,
        monster_id = 0,
    }
    if role == "enemy" then
        s.monster_id = u8(M.MONSTER_IDS + (slot - M.PARTY_SLOTS))
    end
    if not ok_ptr(ptr) then return s end
    local hp = s16(u16(ptr + A_HP))
    s.hp = hp
    s.hp_max = s16(u16(ptr + A_HP_MAX))
    s.mp = s16(u16(ptr + A_MP))
    s.mp_max = s16(u16(ptr + A_MP_MAX))
    s.action = u8(ptr + A_ACTION)
    s.status = u32(ptr + A_STATUS)
    s.x = s16(u16(ptr + A_POS_X))
    s.z = s16(u16(ptr + A_POS_Z))
    s.size = u8(ptr + A_SIZE)
    -- Present only inside a real battle, with HP in a sane combat range.
    s.present = in_battle and hp >= 0 and hp <= 9999 and (s.hp_max > 0 or hp > 0)
    return s
end

-- Read the full typed battle state. Never throws; unmapped reads default to 0.
function M.read()
    local ctx = u32(M.CTX_PTR)
    local st = {
        render_mode = u8(M.RENDER_MODE),
        game_state  = u8(M.GAME_STATE),
        ctx = ctx,
        active_slot = 0,
        party_action_id = 0xFF,
        monster_action_id = 0xFF,
        turn = 0,
        slots = {},
        in_battle = false,
    }
    if ok_ptr(ctx) then
        st.active_slot = u8(ctx + CTX_ACTIVE_SLOT)
        st.party_action_id = u8(ctx + CTX_PARTY_ACTION)
        st.monster_action_id = u8(ctx + CTX_MONSTER_ACTION)
        st.turn = u8(ctx + CTX_TURN)
    end
    -- In battle when the ctx pointer is live and the orbit camera (mode 0x15,
    -- BattleMode) is active. Determined BEFORE the slot read so slot presence
    -- can be gated on it (avoids reporting stale field-state actor pointers as
    -- enemies; live-confirmed against a field-mode save resuming at mode 0x03).
    st.in_battle = ok_ptr(ctx) and st.render_mode == 0x15
    for slot = 0, M.SLOT_COUNT - 1 do
        st.slots[slot] = read_slot(slot, st.in_battle)
    end
    return st
end

-- Cheap change-detection key. Includes only fields whose change should
-- trigger a delta emit (HP, action, status, presence, monster id, the meta
-- action ids + turn). Position is intentionally excluded -- it changes every
-- frame and is not part of the diorama/spectator id-and-scalar vocabulary.
function M.signature(st)
    local parts = {}
    parts[#parts + 1] = string.format("b%d,m%02X,p%02X,e%02X,a%d,t%d",
        st.in_battle and 1 or 0, st.render_mode,
        st.party_action_id, st.monster_action_id, st.active_slot, st.turn)
    for slot = 0, M.SLOT_COUNT - 1 do
        local s = st.slots[slot]
        parts[#parts + 1] = string.format("%d:%d:%d/%d:%d:%02X:%02X:%d",
            slot, s.present and 1 or 0, s.hp, s.hp_max, s.mp,
            s.action, st.slots[slot].status % 0x10000, s.monster_id)
    end
    return table.concat(parts, "|")
end

-- Serialise one slot as a compact JSON object (manual; no json lib in the
-- PCSX Lua sandbox). Only the id-and-scalar vocabulary the transports carry.
local function slot_json(s)
    return string.format(
        '{"slot":%d,"role":"%s","present":%s,"monster_id":%d,'
        .. '"hp":%d,"hp_max":%d,"mp":%d,"mp_max":%d,'
        .. '"action":%d,"status":%d,"x":%d,"z":%d,"size":%d}',
        s.slot, s.role, tostring(s.present), s.monster_id,
        s.hp, s.hp_max, s.mp, s.mp_max,
        s.action, s.status, s.x, s.z, s.size)
end

-- Serialise the whole state as a single JSON line (newline-delimited JSON
-- stream friendly). `kind` tags the record ("full" sweep vs "delta").
function M.to_json(st, kind, vsync)
    local slots = {}
    for slot = 0, M.SLOT_COUNT - 1 do
        slots[#slots + 1] = slot_json(st.slots[slot])
    end
    return string.format(
        '{"kind":"%s","vsync":%d,"in_battle":%s,"render_mode":%d,'
        .. '"active_slot":%d,"party_action":%d,"monster_action":%d,'
        .. '"turn":%d,"slots":[%s]}',
        kind, vsync, tostring(st.in_battle), st.render_mode,
        st.active_slot, st.party_action_id, st.monster_action_id,
        st.turn, table.concat(slots, ","))
end

return M
