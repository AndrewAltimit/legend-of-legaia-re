//! Disc-free regression: "no party seated" is not "party dead".
//!
//! A world that enters battle WITHOUT a roster (no `load_party` /
//! `set_active_party` / `seed_starting_party`) stamps hollow party actors -
//! `max_hp == 0`, no record behind the ordinal. Retail cannot represent that
//! state: its wipe scan (`0x801E6510..0x801E664C`) walks the actor table for
//! the seated-count byte's worth of slots, and battle load always seats the
//! present party. In the port the first damage fold on a hollow actor pairs
//! `hp == 0` with `liveness = 0`, and before `BattleActionHost::slot_seated`
//! existed the wipe scan then reported a PARTY WIPE before anything was hit -
//! which, through the game-over hold, parked every unseeded ladder battle in
//! `SceneMode::Battle` forever (four disc-gated ladders were red on exactly
//! this).
//!
//! Pins both halves of the chosen shape:
//! - an unseeded battle is NOT reported as a party wipe, and
//! - it still terminates: a monster wipe resolves and tears down.

use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::battle_action::{ActionState, BattleEndCause, StepOutcome};

/// A battle world with NO roster: the party slots are stamped by
/// `enter_battle` but nothing projects records onto them - the unseeded
/// state every `SceneHost::open_extracted` ladder used to fight in.
fn unseeded_battle_world() -> World {
    let mut world = World::new();
    assert!(
        world.roster.members.is_empty(),
        "premise: a fresh world has no roster"
    );
    world.enter_battle(3, 2);
    assert!(
        world.actors[..3]
            .iter()
            .all(|a| a.battle.max_hp == 0 && a.battle.hp == 0),
        "premise: unseeded party slots are hollow"
    );
    world
}

#[test]
fn unseeded_party_reading_dead_is_not_a_party_wipe() {
    let mut world = unseeded_battle_world();
    // The ladder mechanism, condensed: something folds damage over the
    // hollow party slots and their liveness drops to 0 without ever having
    // had HP.
    for i in 0..3 {
        world.actors[i].battle.liveness = 0;
    }
    world.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
    let out = world.step_battle();
    assert_ne!(
        out,
        StepOutcome::BattleComplete,
        "an unseeded battle must not end on the party-wipe arm"
    );
    assert_ne!(
        world.battle_end,
        Some(BattleEndCause::PartyWipe),
        "no PartyWipe for a party that was never seated"
    );
    assert!(!world.game_over, "no game over either");
    assert!(
        !world.game_over_hold,
        "and no game-over hold parking Battle"
    );
}

/// The unseeded state must still be able to terminate - a monster wipe
/// resolves as the victory teardown, so a harness that reaches this
/// port-only state does not spin in `SceneMode::Battle` forever.
#[test]
fn unseeded_battle_still_tears_down_on_a_monster_wipe() {
    let mut world = unseeded_battle_world();
    world.battle_return_mode = SceneMode::Field;
    for i in 0..3 {
        world.actors[i].battle.liveness = 0;
    }
    for a in world.actors.iter_mut().skip(3) {
        a.battle.liveness = 0;
        a.battle.hp = 0;
    }
    world.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
    let out = world.step_battle();
    assert_eq!(out, StepOutcome::BattleComplete);
    assert_eq!(world.battle_end, Some(BattleEndCause::MonsterWipe));
    assert!(!world.game_over);
}

/// Contrast pass (non-vacuous): the SAME world with a seated party - records
/// projected via `load_party`, then killed - IS a party wipe. The guard
/// distinguishes "never seated" from "seated and dead"; it does not soften
/// real defeats.
#[test]
fn seated_party_killed_is_still_a_party_wipe() {
    let mut world = World::new();
    world.load_party(legaia_save::Party::zeroed(3));
    world.enter_battle(3, 2);
    for i in 0..3 {
        world.actors[i].battle.liveness = 0;
    }
    world.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
    let out = world.step_battle();
    assert_eq!(out, StepOutcome::BattleComplete);
    assert_eq!(world.battle_end, Some(BattleEndCause::PartyWipe));
}
