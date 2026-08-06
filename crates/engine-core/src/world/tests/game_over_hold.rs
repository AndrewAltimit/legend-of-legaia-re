//! Party-wipe teardown: the game-over hold + the scripted-loss latch.
//!
//! Ground truth is `FUN_8003AEB0`'s back-from-battle arm (see
//! `docs/subsystems/battle.md` § party wipe + the game-over overlay):
//! a real wipe with story-flag 0 clear stores `game_mode = 0x16` +
//! `_DAT_8007BB00 = 1` and pauses the BGM (`jal 0x800266E0` at
//! `0x8003B5EC`); with story-flag 0 (the scripted-loss latch) set, the
//! wipe returns to the field like any battle end and MAIN INIT consumes
//! the latch.

use super::*;
use crate::field_events::FieldEvent;
use crate::monster_catalog::{FormationDef, FormationSlot};
use vm::battle_action::BattleEndCause;

/// A world one party member deep, mid-battle with the BGM swap active and
/// a field snapshot captured, ready for `finish_battle`.
fn wiped_battle_world() -> World {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.current_bgm = Some(0x0A); // field track playing
    world.set_battle_bgm(Some(0x40));
    let formation = FormationDef::new(7, vec![FormationSlot::new(1)]);
    world.field_return = Some(FieldReturnState {
        actors: world.actors.clone(),
        player_actor_slot: world.player_actor_slot,
        party_count: world.party_count,
    });
    world.battle_return_mode = SceneMode::Field;
    world.enter_battle_from_formation(&formation);
    let _ = world.drain_field_events(); // drop the battle-BGM start
    world.actors[0].battle.liveness = 0; // the wipe
    world.battle_end = Some(BattleEndCause::PartyWipe);
    world
}

#[test]
fn party_wipe_pauses_bgm_and_defers_the_field_restore() {
    let mut world = wiped_battle_world();
    world.finish_battle();

    assert!(world.game_over, "wipe raises game over");
    assert!(world.game_over_hold, "field restore is deferred");
    assert_eq!(
        world.mode,
        SceneMode::Battle,
        "the hold keeps the battle scene up (the frozen frame)"
    );
    assert!(
        world.field_return.is_some(),
        "field actor snapshot is NOT restored before the hand-off"
    );
    assert!(!world.battle_bgm_active, "swap bookkeeping dropped");
    let evs = world.drain_field_events();
    assert!(
        evs.iter()
            .any(|e| matches!(e, FieldEvent::Bgm { sub_op: 2, .. })),
        "the wipe routes the retail BGM pause (0x8003B5EC), got {evs:?}"
    );
    assert!(
        !evs.iter()
            .any(|e| matches!(e, FieldEvent::Bgm { sub_op: 1, .. })),
        "no field-BGM cross-fade on a wipe: {evs:?}"
    );

    // Host's GameOverSession resolves -> the deferred restore runs.
    world.resolve_game_over_hold();
    assert!(!world.game_over_hold);
    assert_eq!(world.mode, SceneMode::Field);
    assert!(world.field_return.is_none(), "snapshot consumed on resolve");
}

#[test]
fn party_wipe_under_scripted_loss_latch_returns_to_field() {
    let mut world = wiped_battle_world();
    // The scene script raised the scripted-loss latch (story-flag index 0,
    // `0x80085758` bit 0x80) at battle start - the Rim Elm ambush shape.
    world.system_flag_set(0);
    world.finish_battle();

    assert!(!world.game_over, "a scripted loss is not a game over");
    assert!(!world.game_over_hold, "no hold - ordinary field return");
    assert_eq!(world.mode, SceneMode::Field);
    assert!(
        !world.system_flag_test(0),
        "the latch is consumed (retail 0x8003B608 andi 0x7f)"
    );
    assert!(world.field_return.is_none(), "field snapshot restored");
    let evs = world.drain_field_events();
    assert!(
        evs.iter().any(|e| matches!(
            e,
            FieldEvent::Bgm {
                text_id: 0x0A,
                sub_op: 1
            }
        )),
        "field BGM restores like any battle end: {evs:?}"
    );
}

#[test]
fn repeated_wipe_folds_during_the_hold_are_inert() {
    let mut world = wiped_battle_world();
    world.finish_battle();
    let _ = world.drain_field_events();

    // With the scene parked in Battle mode, a host that keeps ticking the
    // world re-runs the action SM's wipe scan, which re-raises `battle_end`
    // every tick. The repeat fold must consume the cause and change nothing.
    world.battle_end = Some(BattleEndCause::PartyWipe);
    world.finish_battle();
    assert_eq!(world.battle_end, None, "repeat cause consumed");
    assert!(world.game_over_hold, "hold still frozen");
    assert_eq!(world.mode, SceneMode::Battle);
    assert!(world.field_return.is_some());
    assert!(
        world.drain_field_events().is_empty(),
        "no duplicate BGM events from the repeat fold"
    );
}

#[test]
fn non_wipe_teardown_keeps_the_latch_and_restores_the_field() {
    // A victory in a latch-armed battle must not consume the latch or take
    // any wipe arm: the non-wipe path is unchanged.
    let mut world = wiped_battle_world();
    world.system_flag_set(0);
    world.actors[0].battle.liveness = 1; // party survived after all
    world.battle_end = Some(BattleEndCause::MonsterWipe);
    world.finish_battle();

    assert!(!world.game_over);
    assert!(!world.game_over_hold);
    assert_eq!(world.mode, SceneMode::Field);
    assert!(world.field_return.is_none(), "field snapshot restored");
    assert!(
        world.system_flag_test(0),
        "the wipe gate reads the latch only on a wipe"
    );
    let evs = world.drain_field_events();
    assert!(
        evs.iter().any(|e| matches!(
            e,
            FieldEvent::Bgm {
                text_id: 0x0A,
                sub_op: 1
            }
        )),
        "victory restores the field BGM: {evs:?}"
    );
}

#[test]
fn resolve_game_over_hold_is_a_noop_without_a_hold() {
    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    world.resolve_game_over_hold();
    assert_eq!(world.mode, SceneMode::Field);
    assert!(!world.game_over_hold);
}
