//! A party **Attack** has to queue strikes - the wiring between the command
//! menu's confirm and the attack band's strike loop.
//!
//! The attack band (`FUN_801E295C` state `0x1E`) is a walk over the acting
//! actor's action-parameter stream `actor[+0x1DF..]`, terminating on `0x00`.
//! An Attack that arms the band without seeding that stream is therefore a
//! **no-op turn**: the loop reads its terminator on byte 0 and drops straight
//! to recovery, so no swing byte is staged, no equipment swing clip is
//! committed, no effect script is installed, and the move-power record the
//! weapon-trail streak projects from never resolves (its key is this stream's
//! first byte). Damage still landed, because the live loop applied it through
//! its own edge-triggered path - which is what made the gap invisible.
//!
//! These tests pin all four halves:
//!
//!  1. confirming Attack writes a non-empty swing stream
//!     ([`legaia_engine_vm::battle_action::basic_attack_queue`], the port of
//!     `FUN_801EED1C`'s no-directional-input arm);
//!  2. the chain walks it - every queued byte is staged as the next anim and
//!     the strike cursor advances, instead of exiting on byte 0;
//!  3. a weapon swing clip actually commits per queued byte;
//!  4. damage is applied **exactly once per queued swing** - the chain owns
//!     it, and the loop's edge-triggered path stands down.
//!
//! Disc-free. The disc-gated sibling
//! `battle_attack_queue_move_power_disc.rs` pins the move-power half against
//! the real table.

use legaia_asset::monster_archive::{MonsterAnimation, PartPose};
use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_vm::battle_action::{ActionState, SWING_LEFT, SWING_RIGHT, is_swing_command};

/// A player-driven battle parked on its opening command menu, with the
/// monster band buffed so it survives the measurement window.
///
/// Entry goes through the ordinary encounter path (the same shape
/// `battle_player_driven.rs` uses): battle entry is what opens the opening
/// command session, so the menu is already up when this returns.
fn battle_awaiting_command() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 30_000;
        w.actors[i].battle.max_hp = 30_000;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 120);
    }
    w.load_party(legaia_save::Party::zeroed(3));
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());

    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.field_camera_azimuth = 0;

    use legaia_engine_core::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("attack_queue_wiring_test");
    table.set_trigger_rate(0xFF);
    table.push(EncounterEntry::new(1, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));

    w.mode = SceneMode::Field;
    w.live_gameplay_loop = true;
    w.battle_player_driven = true;

    let up = InputState::mask_of([PadButton::Up]);
    for _ in 0..8000 {
        w.set_pad(up);
        w.tick();
        if w.mode == SceneMode::Battle {
            break;
        }
    }
    assert_eq!(w.mode, SceneMode::Battle, "walking should trigger a battle");
    assert!(
        w.battle_command.is_some(),
        "battle entry opens the opening command session"
    );
    // Make every seated monster tanky enough that the measured Attack can't
    // wipe the formation mid-action.
    for i in w.party_count as usize..w.actors.len() {
        let a = &mut w.actors[i].battle;
        if a.max_hp > 0 {
            a.hp = 30_000;
            a.max_hp = 30_000;
        }
    }
    w
}

/// The party slot the open command session belongs to.
fn acting_slot(w: &World) -> usize {
    w.battle_ctx.active_actor as usize
}

/// Press Cross on alternate frames (the picker is edge-triggered) until the
/// command session resolves: first press picks Attack, second confirms the
/// lone monster target.
fn confirm_attack(w: &mut World) {
    let cross = InputState::mask_of([PadButton::Cross]);
    let mut pressed = false;
    for _ in 0..64 {
        w.set_pad(if pressed { 0 } else { cross });
        pressed = !pressed;
        w.tick();
        if w.battle_command.is_none() {
            return;
        }
    }
    panic!("Attack was never confirmed");
}

/// The swing bytes sitting in `actor[+0x1DF..]`, up to the `0x00` terminator.
fn queued_swings(w: &World, slot: usize) -> Vec<u8> {
    w.actors[slot]
        .battle
        .params
        .iter()
        .copied()
        .take_while(|&b| b != 0)
        .collect()
}

#[test]
fn confirming_attack_seeds_a_non_empty_swing_stream() {
    let mut w = battle_awaiting_command();
    let slot = acting_slot(&w);

    // Before the confirm the stream is empty - `clear_action_stream` runs at
    // every arm, and that is exactly the state the band used to execute in.
    assert!(
        queued_swings(&w, slot).is_empty(),
        "a parked turn starts from a cleared stream"
    );

    confirm_attack(&mut w);

    let swings = queued_swings(&w, slot);
    assert!(
        !swings.is_empty(),
        "confirming Attack must queue at least one strike, got an empty stream"
    );
    // Retail's no-input arm writes two independently rolled Left/Right arm
    // swings for an ordinary target.
    assert_eq!(swings.len(), 2, "two arm swings: {swings:02X?}");
    for b in &swings {
        assert!(
            *b == SWING_LEFT || *b == SWING_RIGHT,
            "each swing is a rolled arm command: {b:#04X}"
        );
        assert!(is_swing_command(*b));
    }
    assert_eq!(
        w.actors[slot].battle.strike_index, 0,
        "the strike cursor rewinds with the seed"
    );
    assert_eq!(w.actors[slot].battle.action_category, 3, "Attack category");
}

#[test]
fn the_attack_chain_walks_every_queued_swing() {
    let mut w = battle_awaiting_command();
    let slot = acting_slot(&w);
    confirm_attack(&mut w);
    let queued = queued_swings(&w, slot);
    assert!(!queued.is_empty());

    // Drive the SM to the end of the action, recording every byte the chain
    // stages and how far its cursor gets.
    let mut staged: Vec<u8> = Vec::new();
    let mut max_cursor = 0u8;
    let mut reached_chain = false;
    let mut reached_recovery = false;
    for _ in 0..4000 {
        let before = w.battle_ctx.action_state;
        w.set_pad(0);
        w.tick();
        if before == ActionState::AttackChain.as_byte() {
            reached_chain = true;
            let a = &w.actors[slot].battle;
            max_cursor = max_cursor.max(a.strike_index);
            if is_swing_command(a.queued_anim) && staged.last() != Some(&a.queued_anim) {
                staged.push(a.queued_anim);
            }
        }
        if w.battle_ctx.action_state == ActionState::AttackRecovery.as_byte() {
            reached_recovery = true;
            break;
        }
    }

    assert!(reached_chain, "the action must reach the strike loop");
    assert!(reached_recovery, "and leave it through recovery");
    assert_eq!(
        max_cursor as usize,
        queued.len(),
        "the cursor must consume every queued byte, not exit on byte 0"
    );
    assert!(
        !staged.is_empty(),
        "the chain must stage the queued swing bytes as anim ids"
    );
    for b in &staged {
        assert!(is_swing_command(*b), "staged a non-swing byte {b:#04X}");
    }
}

/// A synthetic two-frame clip with a distinguishable pose, enough for the
/// anim commit to install a player (which is what `battle_staged_anim`
/// records).
fn stub_clip(action_id: u8) -> MonsterAnimation {
    let pose = |f: u16| PartPose {
        rx: 0,
        ry: f.wrapping_mul(16),
        rz: 0,
        tx: (f as i16).wrapping_mul(16),
        ty: 0,
        tz: 0,
    };
    MonsterAnimation {
        action_id,
        rate: 1,
        part_count: 1,
        frame_count: 8,
        frames: (0..8u16).map(|f| vec![pose(f)]).collect(),
        effect_script: Vec::new(),
    }
}

#[test]
fn each_queued_swing_commits_a_weapon_swing_clip() {
    let mut w = battle_awaiting_command();
    let slot = acting_slot(&w);
    // Action-table entries 0..=0x0F; `0x0C..=0x0F` are the equipment-spliced
    // weapon swings the queue's arm commands index.
    let clips: Vec<Option<MonsterAnimation>> = (0..0x10u8).map(|i| Some(stub_clip(i))).collect();
    w.set_actor_battle_action_clips(slot, std::sync::Arc::new(clips));

    confirm_attack(&mut w);
    let queued = queued_swings(&w, slot);

    let mut committed: Vec<u8> = Vec::new();
    for _ in 0..8000 {
        w.set_pad(0);
        w.tick();
        if let Some(id) = w.actors[slot].battle_staged_anim
            && is_swing_command(id)
            && committed.last() != Some(&id)
        {
            committed.push(id);
        }
        if w.battle_ctx.action_state == ActionState::AttackRecovery.as_byte() {
            break;
        }
    }

    assert!(
        !committed.is_empty(),
        "a weapon swing clip must commit for the queued strike"
    );
    assert!(
        committed.len() <= queued.len(),
        "no more swing clips than queued bytes: {committed:02X?} vs {queued:02X?}"
    );
    for id in &committed {
        assert!(
            (0x0C..=0x0F).contains(id),
            "committed clip {id:#04X} is not an equipment swing slot"
        );
    }
}

#[test]
fn damage_lands_exactly_once_per_queued_swing() {
    let mut w = battle_awaiting_command();
    let slot = acting_slot(&w);
    confirm_attack(&mut w);
    let queued = queued_swings(&w, slot);
    assert_eq!(queued.len(), 2);
    let target = w.actors[slot].battle.active_target as usize;
    assert!(target >= w.party_count as usize, "Attack targets a monster");

    let hp_before = w.actors[target].battle.hp;
    let mut hits: Vec<u16> = Vec::new();
    // Run the action out. Stop the moment the loop parks for the *next* party
    // command, so only this Attack's hits are counted.
    for _ in 0..8000 {
        w.set_pad(0);
        w.tick();
        for fx in w.drain_battle_hit_fx() {
            if fx.target_slot as usize == target && !fx.is_heal {
                hits.push(fx.amount);
            }
        }
        if w.battle_command.is_some() {
            break;
        }
    }

    assert_eq!(
        hits.len(),
        queued.len(),
        "one hit per queued swing - {} queued, {} landed ({hits:?})",
        queued.len(),
        hits.len()
    );
    assert!(hits.iter().all(|&d| d > 0), "each swing connects: {hits:?}");
    let total: u32 = hits.iter().map(|&d| d as u32).sum();
    assert_eq!(
        u32::from(hp_before) - u32::from(w.actors[target].battle.hp),
        total,
        "HP loss must equal the sum of the reported swings - a second \
         application path would show up as an unreported extra"
    );
}

#[test]
fn an_unseeded_actor_still_resolves_a_single_strike() {
    // The monster band has no queue (its swing count is the AGL budget, not a
    // byte stream), so the loop's edge-triggered application must stay live
    // for it. Guards the reconciliation against over-correcting.
    let mut w = World::new();
    w.enter_battle(1, 1);
    for slot in 0..2u8 {
        let a = &mut w.actors[slot as usize].battle;
        a.hp = 30_000;
        a.max_hp = 30_000;
        a.liveness = 1;
    }
    w.set_battle_attack(1, 120);
    w.set_battle_defense(0, 10);
    w.live_gameplay_loop = true;
    // Arm the monster's physical strike directly, leaving its stream empty.
    w.battle_ctx.active_actor = 1;
    w.battle_ctx.queued_action = 3;
    w.battle_ctx.action_state = ActionState::Begin.as_byte();
    w.actors[1].battle.active_target = 0;
    w.actors[1].battle.action_category = 3;
    assert!(queued_swings(&w, 1).is_empty());

    let hp_before = w.actors[0].battle.hp;
    for _ in 0..4000 {
        w.set_pad(0);
        w.tick();
        if w.battle_ctx.action_state == ActionState::AttackRecovery.as_byte() {
            break;
        }
    }
    assert!(
        w.actors[0].battle.hp < hp_before,
        "an unseeded attacker still lands its strike"
    );
}
