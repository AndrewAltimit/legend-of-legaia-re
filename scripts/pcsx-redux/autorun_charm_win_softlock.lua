-- autorun_charm_win_softlock.lua
--
-- Confirm (and optionally un-stick) the enemy-ally "charm" softlock that fires
-- when a charmed monster wins a MULTI-enemy fight by KO'ing the last hostile.
--
-- ROOT CAUSE (static, from overlay_battle_action_801e7320.txt): the 0x380
-- retarget helper FUN_801E7320 has an UNBOUNDED reroll loop at 0x801E7370:
--
--     801e7370  jal rand ; a0 = 3 + rand % monster_count   (pick a monster slot)
--     801e73d0  lhu v0,0x14c(monster[a0])                    (target HP)
--     801e73d8  beq v0,zero,0x801e7370                       (reroll WHILE HP==0)
--
-- A charmed monster's default target is a party slot (<3), so it enters this
-- loop, which flips it to a MONSTER target and rerolls while the pick is dead.
-- Once the charmed ally is the last living monster, every other monster slot is
-- dead -> the reroll never terminates -> hard softlock. Vanilla never hits it
-- (a confused actor's opposite side is the party, which can't be fully dead
-- mid-battle); charm makes the opposite side the OTHER monsters, which can be.
--
-- This probe arms an Exec breakpoint on the loop head (0x801E7370) and counts
-- hits per frame. Normal retargeting rerolls only a few times between vsyncs; a
-- SPIN hammers the loop head thousands of times with NO vsync in between (the
-- CPU never returns to render). On spin it dumps the live monster slots (HP +
-- flags), which will show the charmed ally (flags & 0x380) alive and every other
-- monster dead (HP==0) - the confirmation.
--
-- LEGAIA_CHARM_UNSTICK=1: on spin, poke the battle-end signal DAT_8007BD71=0xFE
-- so the fight ends and you can keep playing/capturing. This also proves the fix
-- direction: if forcing battle-end resolves cleanly, the victory logic is fine
-- and the ONLY bug is the unbounded loop.
--
-- Exec BPs need the interpreter + debugger, so DO NOT run this with --fast:
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_charm_win_softlock.lua \
--     --sstate /path/to/your/slot1/near/the/softlock.sstate
-- (Load the save inside the emulator and walk into the charmed multi-enemy
-- fight; or point --sstate at a state already in that battle.)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local bp    = require("probe.bp")
local bit   = require("bit") -- PCSX-Redux is LuaJIT: use bit.band, not `&`

local RETARGET_LOOP_HEAD = 0x801E7370 -- FUN_801E7320 loop-1 reroll head
local ACTOR_TABLE = 0x801C9370        -- slots 0..2 party, 3..6 monsters
local CTX_PTR     = 0x8007BD24        -- -> ctx struct (byte[0]=party, [1]=monster)
local VICT_SIGNAL = 0x8007BD71        -- u8:  0xFE = battle-end signal
local WIPE_SIDE   = 0x8007BD2C        -- u32: 0 = monster wipe (WIN), 5 = party wipe (LOSE)
local BATTLE_FLAG = 0x8007BD60        -- u8:  &= 0x7F on battle end
local A_HP        = 0x14C
local A_FLAGS     = 0x16E
local AI_DELEGATE = 0x380

-- A frame with more loop-head hits than this, with no vsync in between, is a
-- spin (real retargeting rerolls at most a handful of times per frame).
local SPIN_THRESHOLD = 4000

local UNSTICK = probe.getenv("LEGAIA_CHARM_UNSTICK", "") == "1"

local function u8(a)  return probe.read_u8(a)  or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function ok_ptr(p) return p >= 0x80000000 and p < 0x80200000 end

local frame_hits   = 0
local total_hits   = 0
local spin_logged  = false

local function log(s) PCSX.log(s) end

local function dump_monster_slots(tag)
    local ctx = u32(CTX_PTR)
    local party_n = ok_ptr(ctx) and u8(ctx + 0) or -1
    local mon_n   = ok_ptr(ctx) and u8(ctx + 1) or -1
    log(string.format("[charm-softlock] %s  party_n=%d monster_n=%d",
        tag, party_n, mon_n))
    for slot = 3, 6 do
        local p = u32(ACTOR_TABLE + slot * 4)
        if ok_ptr(p) then
            local hp = u16(p + A_HP)
            local fl = u16(p + A_FLAGS)
            log(string.format(
                "  slot %d @%08X  HP=%-5d flags=0x%04X%s%s",
                slot, p, hp, fl,
                bit.band(fl, AI_DELEGATE) == AI_DELEGATE and "  [CHARMED 0x380]" or "",
                hp == 0 and "  [DEAD]" or ""))
        else
            log(string.format("  slot %d  (empty)", slot))
        end
    end
end

local function on_loop_head()
    frame_hits = frame_hits + 1
    total_hits = total_hits + 1
    if frame_hits == SPIN_THRESHOLD and not spin_logged then
        spin_logged = true
        log(string.format(
            "[charm-softlock] SPIN DETECTED: FUN_801E7320 loop-1 head 0x%08X hit "
            .. "%d times in one frame with no vsync -> unbounded reroll (no live "
            .. "monster target).", RETARGET_LOOP_HEAD, frame_hits))
        dump_monster_slots("state at spin")
        if UNSTICK then
            -- Force a MONSTER WIPE (win): set the win side FIRST, then the end
            -- signal. Poking only BD71 leaves BD2C stale and the teardown reads
            -- a party wipe -> "team annihilated" -> title (0=win, 5=lose;
            -- reader FUN_801D5854 0x801D69D4 `beq BD2C,zero,<victory>`).
            probe.write_u16(WIPE_SIDE, 0)
            probe.write_u16(WIPE_SIDE + 2, 0)
            probe.write_u8(BATTLE_FLAG, bit.band(u8(BATTLE_FLAG), 0x7F))
            probe.write_u8(VICT_SIGNAL, 0xFE)
            log("[charm-softlock] LEGAIA_CHARM_UNSTICK=1: forced MONSTER WIPE "
                .. "(BD2C=0, BD71=0xFE). Fight should resolve as a WIN; if it "
                .. "does, the victory path is fine and the loop is the only bug.")
        else
            log("[charm-softlock] (re-run with LEGAIA_CHARM_UNSTICK=1 to "
                .. "force the battle to end and recover.)")
        end
    end
end

-- vsync means a frame completed, so we were NOT spinning: reset the per-frame
-- counter and the one-shot log latch. Anchor the listener in the GLOBAL anchor
-- table - a `local` handle is GC'd once this chunk returns, and PCSX's __gc
-- proxy then deletes the C++ listener (and GC mid-dispatch can segfault).
local function on_vsync()
    if frame_hits > 0 and frame_hits < SPIN_THRESHOLD then
        -- occasional heartbeat so a healthy run shows the loop is bounded
        if (total_hits % 997) < frame_hits then
            log(string.format("[charm-softlock] alive: retarget loop bounded "
                .. "(%d hits last frame, %d total)", frame_hits, total_hits))
        end
    end
    frame_hits  = 0
    spin_logged = false
end
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)

bp.arm(RETARGET_LOOP_HEAD, "Exec", 4, "charm_retarget_loop", on_loop_head)

log(string.format(
    "[charm-softlock] armed Exec BP on FUN_801E7320 loop head 0x%08X "
    .. "(spin threshold %d hits/frame). unstick=%s", RETARGET_LOOP_HEAD,
    SPIN_THRESHOLD, tostring(UNSTICK)))
log("[charm-softlock] load your save, walk into a charmed multi-enemy "
    .. "fight, and let the charmed ally KO the last hostile.")
