//! Disc-free: the battle-intro spin holds the retail mode word back until the
//! kernel's own hand-off frame.
//!
//! `_DAT_8007B83C = 0x14` lands at `0x801CF8F8` inside `FUN_801CF5BC`, and only
//! once the intro clock has passed its full duration *and* the entity's `ready`
//! reads exactly `3`. The port seats the battle **scene** at the encounter
//! trigger where retail seats it at the end of the spin, so `World::mode`
//! reaches `Battle` a whole transition early - and `ModeSeat::adopt_scene_mode`
//! moved the word with it. `ModeSeat::adopt_world_mode` is the form that asks
//! the world first.
//!
//! Non-vacuity: the same fixture is checked through `adopt_scene_mode`, which
//! *does* move on the held frame, so a hold that silently stopped working
//! fails here rather than passing by looking the same.

use legaia_engine_core::encounter::{
    EncounterEntry, EncounterRoll, EncounterSession, EncounterTable, EncounterTracker,
};
use legaia_engine_core::mode::{GameMode, ModeSeat};
use legaia_engine_core::world::{SceneMode, World};

fn transitioning_world() -> World {
    let mut world = World::new();
    let table = EncounterTable {
        scene_label: "w4d".to_string(),
        entries: vec![EncounterEntry {
            formation_id: 1,
            weight: 1,
            min_steps_since_last: 0,
        }],
        ..Default::default()
    };
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    assert!(
        session.trigger_with(EncounterRoll {
            formation_id: 1,
            row_index: 0,
            roll_q8: 0,
        }),
        "an Idle session takes an externally-rolled trigger"
    );
    world.encounter = Some(session);
    // What the port does at the trigger: the battle scene is already up.
    world.mode = SceneMode::Battle;
    world
}

#[test]
fn the_mode_word_waits_for_the_intro_hand_off() {
    let mut world = transitioning_world();
    assert!(
        world.battle_mode_word_held(),
        "the spin is running and the hand-off has not fired"
    );

    let mut held_seat = ModeSeat::new(GameMode::MainMode);
    assert_eq!(
        held_seat.adopt_world_mode(&world),
        None,
        "the world-aware form declines while the spin holds the word"
    );
    assert_eq!(held_seat.game_mode(), GameMode::MainMode);

    // Non-vacuity: the scene-only form moves on this very frame, which is the
    // behaviour the hold exists to correct.
    let mut naive_seat = ModeSeat::new(GameMode::MainMode);
    assert_eq!(
        naive_seat.adopt_scene_mode(world.mode),
        Some(GameMode::BattleMode),
        "the scene-only form takes the early edge"
    );

    // The kernel's hand-off frame releases it.
    world.battle_intro_mode_handoff = true;
    assert!(!world.battle_mode_word_held());
    assert_eq!(
        held_seat.adopt_world_mode(&world),
        Some(GameMode::BattleMode),
        "the word moves on the retail edge"
    );
    assert_eq!(held_seat.game_mode(), GameMode::BattleMode);
}

#[test]
fn nothing_outside_a_transition_is_gated() {
    // No encounter session at all: the hold must be false, or every mode
    // change in the engine would stall.
    let mut world = World::new();
    world.mode = SceneMode::Battle;
    assert!(!world.battle_mode_word_held());
    let mut seat = ModeSeat::new(GameMode::MainMode);
    assert_eq!(seat.adopt_world_mode(&world), Some(GameMode::BattleMode));

    // And a world sitting in the field is unaffected either way.
    let mut field = World::new();
    field.mode = SceneMode::Field;
    assert!(!field.battle_mode_word_held());
}
