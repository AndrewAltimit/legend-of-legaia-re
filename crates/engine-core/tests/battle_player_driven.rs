//! Player-driven battle integration test: the live loop pauses each party
//! turn for a command + target selection read from the pad, rather than
//! auto-resolving.
//!
//! Companion to `live_loop_tick.rs` (which exercises the auto-resolve spine).
//! Here [`World::battle_player_driven`] is set, so:
//!
//!   1. Walking still rolls a step-driven encounter and flips Field -> Battle.
//!   2. On entering battle the action SM PARKS - a command session opens and
//!      no strike lands until the player acts (asserted by holding no input
//!      and watching the monster's HP stay put).
//!   3. Pressing Cross selects Attack, then Cross again confirms the target;
//!      only then does the strike commit.
//!   4. The monster wipes, loot applies, and control returns to Field.
//!
//! Disc-free: synthetic party + the vanilla monster/formation tables. Runs in
//! CI unconditionally.

use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};

/// Build a field-ready world identical to the auto-resolve live-loop test
/// but with the player-driven battle flag set.
fn build_player_driven_world() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 60);
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
    let mut table = EncounterTable::new("player_driven_test");
    table.set_trigger_rate(0xFF);
    table.push(EncounterEntry::new(1, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));

    w.mode = SceneMode::Field;
    w.live_gameplay_loop = true;
    w.battle_player_driven = true;
    w
}

/// Sum the HP of all monster slots (party_count..) - the auto-resolve guard.
fn monster_hp_total(w: &World) -> u32 {
    (w.party_count as usize..w.actors.len())
        .map(|i| w.actors[i].battle.hp as u32)
        .sum()
}

#[test]
fn battle_waits_for_player_command_then_resolves() {
    let mut w = build_player_driven_world();
    let up = InputState::mask_of([PadButton::Up]);
    let cross = InputState::mask_of([PadButton::Cross]);

    // --- Phase 1: walk into a battle. ---
    let mut entered = false;
    for _ in 0..6000 {
        w.set_pad(up);
        w.tick();
        if w.mode == SceneMode::Battle {
            entered = true;
            break;
        }
    }
    assert!(
        entered,
        "walking should trigger a Field -> Battle transition"
    );
    assert!(
        w.battle_command.is_some(),
        "entering battle should open a command session, not auto-attack"
    );

    // --- Phase 2: hold no input - the SM must stay parked. ---
    let hp_before = monster_hp_total(&w);
    assert!(hp_before > 0, "monster should be alive on battle entry");
    for _ in 0..120 {
        w.set_pad(0);
        w.tick();
        assert_eq!(
            w.mode,
            SceneMode::Battle,
            "no input must not end the battle"
        );
        assert!(
            w.battle_command.is_some(),
            "command session stays open while the player gives no input"
        );
    }
    assert_eq!(
        monster_hp_total(&w),
        hp_before,
        "no strike may land before the player confirms a command"
    );

    // --- Phase 3: drive the command picker. Cross presses are edge-triggered,
    // so alternate press/release frames. First press selects Attack, second
    // confirms the (lone) monster target; then the strike commits and the
    // single-monster formation wipes. ---
    let mut returned = false;
    let mut pressed = false;
    for _ in 0..2000 {
        // Press Cross on alternate frames to generate clean just_pressed edges.
        w.set_pad(if pressed { 0 } else { cross });
        pressed = !pressed;
        w.tick();
        if w.mode == SceneMode::Field && w.last_battle_rewards.is_some() {
            returned = true;
            break;
        }
    }

    assert!(returned, "battle should resolve once commands are issued");
    assert_eq!(w.mode, SceneMode::Field, "return to field after the wipe");
    assert!(
        w.battle_command.is_none(),
        "command session cleared on exit"
    );
    let rewards = w
        .last_battle_rewards
        .as_ref()
        .expect("victory records rewards");
    assert!(rewards.xp > 0, "victory grants XP: {rewards:?}");
    assert!(rewards.gold > 0, "victory drops gold: {rewards:?}");
    assert!(!w.game_over, "a monster wipe is not a party wipe");
}

#[test]
fn player_driven_off_auto_resolves_like_the_spine() {
    // Same world but player-driven OFF: the live loop must auto-resolve with
    // no input at all (guards that the new flag is purely additive).
    let mut w = build_player_driven_world();
    w.battle_player_driven = false;
    let up = InputState::mask_of([PadButton::Up]);

    let mut entered = false;
    for _ in 0..8000 {
        w.set_pad(up);
        w.tick();
        if w.mode == SceneMode::Battle {
            entered = true;
        }
        assert!(
            w.battle_command.is_none(),
            "no command session when player-driven is off"
        );
        if entered && w.mode == SceneMode::Field && w.last_battle_rewards.is_some() {
            break;
        }
    }
    assert!(entered, "should still enter battle");
    assert_eq!(w.mode, SceneMode::Field, "auto-resolve returns to field");
    assert!(w.last_battle_rewards.is_some(), "auto-resolve applies loot");
}
