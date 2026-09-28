//! The special-battle word's readers that sit in the battle flow rather than
//! in the reward arithmetic: the wipe rule, the run arm, the results-window
//! gate and the escape roll's forced flee (see
//! `docs/subsystems/battle-formulas.md#the-special-battle-words-readers`).

use super::*;
use crate::battle_round::{PendingPartyAction, RoundPhase};
use legaia_engine_vm::status_effects::StatusKind;

/// A battle with a party of `party` standing members and one living monster
/// seated behind them.
fn special_world(party: u8, word: u32) -> World {
    let mut world = World {
        party: crate::world::PartyState {
            party_count: party,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    for i in 0..=usize::from(party) {
        let b = &mut world.actors[i].battle;
        b.max_hp = 100;
        b.hp = 100;
        b.liveness = 1;
        world.battle.speed[i] = 20;
    }
    world.actors[usize::from(party)].battle_monster_id = Some(0x10);
    world.battle.special_word = word;
    world
}

/// `0x801E6578..0x801E65AC`: three Rot rolls on the leader end a special
/// battle as a party wipe with everyone standing; the same leader in an
/// ordinary battle fights on. The rolls reach the rule through the typed
/// tracker, which now keeps every limb retail's `or` keeps.
#[test]
fn a_leader_rotted_in_every_limb_loses_a_special_battle() {
    let run = |word: u32, limbs: &[u8]| {
        let mut world = special_world(2, word);
        for &limb in limbs {
            world.battle.status_effects.apply(0, StatusKind::Rot);
            world.battle.status_effects.set_rot_limb(0, limb);
        }
        world.battle_ctx.action_state = vm::battle_action::ActionState::EndOfAction.as_byte();
        (world.step_battle(), world.battle.end)
    };
    assert_eq!(
        run(0x100, &[0, 1, 2]),
        (StepOutcome::BattleComplete, Some(BattleEndCause::PartyWipe))
    );
    assert_ne!(run(0, &[0, 1, 2]).0, StepOutcome::BattleComplete);
    assert_ne!(run(0x100, &[0, 2]).0, StepOutcome::BattleComplete);
}

/// `0x801D3228..0x801D328C`: a Run round in a special battle dispatches the
/// leader first, past a monster whose key would have won; the monster keeps
/// its key for later in the round.
#[test]
fn a_special_battle_run_hands_the_leader_the_first_turn() {
    let mut world = special_world(1, 0x100);
    world.actors[0].battle.init_key = 10;
    world.actors[1].battle.init_key = 50;
    world.battle.round_flow.phase = RoundPhase::Execute;
    world.battle.round_flow.leader_first = true;
    assert_eq!(world.leader_first_combatant(), Some(0));
    assert_eq!(
        world.actors[0].battle.init_key, 0,
        "the leader's key is spent"
    );
    assert_eq!(
        world.actors[1].battle.init_key, 50,
        "the pick keeps its key"
    );

    // The arm itself: raised on a Run commit, not for the Ra-Seru fights.
    let arm = |word: u32, first_monster: u16| {
        let mut world = special_world(1, word);
        world.battle.active_formation = Some(crate::monster_catalog::FormationDef::new(
            1,
            vec![crate::monster_catalog::FormationSlot::new(first_monster)],
        ));
        world.actors[0].battle.init_key = 10;
        world.actors[1].battle.init_key = 50;
        world.battle.round_flow.pending[0] = Some(PendingPartyAction::Run);
        world.begin_round_execution();
        world.battle_ctx.active_actor
    };
    assert_eq!(arm(0x100, 0x10), 0, "the leader acts first");
    assert_eq!(arm(0, 0x10), 1, "an ordinary Run keeps the initiative pick");
    assert_eq!(arm(0x200, 0x3F), 1, "the Rim Elm ambush is exempt");
}

/// `0x8004F614` / `0x8004F8F0`: the results frame of a special battle opens
/// no window, so no spoils panel is drawn; an ordinary win opens it.
#[test]
fn a_special_battle_opens_no_result_window() {
    let run = |word: u32| {
        let mut world = special_world(1, word);
        world.battle.end = Some(BattleEndCause::MonsterWipe);
        world.begin_battle_end_sequence();
        for _ in 0..World::VICTORY_LOAD_FRAMES {
            world.tick_battle_end_sequence();
        }
        let seq = world.battle.victory.expect("sequence armed");
        assert!(matches!(seq.phase, VictoryPhase::Results { .. }));
        (seq.window_opened, world.battle_spoils_banner().is_some())
    };
    assert_eq!(run(0), (true, false), "window up (no rewards staged)");
    assert_eq!(run(0x200), (false, false));

    // The loss window takes the same gate.
    let mut world = special_world(1, 0x100);
    world.battle.end = Some(BattleEndCause::PartyWipe);
    world.begin_battle_end_sequence();
    assert!(!world.battle.victory.expect("armed").window_opened);
}

/// `0x801E7978..0x801E7A14`: bit `0x100` of the word forces the flee, past
/// even the scripted no-escape flag, and still takes the roll's two draws.
#[test]
fn the_word_s_0x100_bit_forces_the_flee() {
    for seed in 0..20u32 {
        let mut world = special_world(1, 0x100);
        world.battle.speed[0] = 1;
        world.battle.speed[1] = 1000;
        world.battle.no_escape = true;
        world.rng_state = seed;
        assert!(world.roll_battle_escape(), "seed {seed}");
        let mut plain = special_world(1, 0x100);
        plain.rng_state = seed;
        let _ = plain.next_rand();
        let _ = plain.next_rand();
        assert_eq!(world.rng_state, plain.rng_state, "two draws taken");
    }
    let mut world = special_world(1, 0x200);
    world.battle.no_escape = true;
    assert!(!world.roll_battle_escape(), "0x200 alone forces nothing");
}

/// A monster seat's ailments end with the battle: retail builds each
/// battle's monster actors afresh, and the slot-indexed tracker would
/// otherwise hand a Venom to the next battle's monster in the same seat.
#[test]
fn a_monster_seat_s_ailments_end_with_its_battle() {
    let mut world = special_world(1, 0);
    world.battle.status_effects.apply(1, StatusKind::Venom);
    world.battle.status_effects.apply(0, StatusKind::Venom);
    world.battle.end = Some(BattleEndCause::Escaped);
    world.finish_battle();
    assert_eq!(world.battle.status_effects.display_flags(1), 0);
    assert_ne!(
        world.battle.status_effects.display_flags(0),
        0,
        "the party's are the record's question and stay"
    );
}
