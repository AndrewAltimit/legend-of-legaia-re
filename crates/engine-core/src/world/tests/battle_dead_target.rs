//! The turn picker's dead-target redirect (`FUN_801DABA4` party arm ->
//! `FUN_801DB124`): a party member whose committed strike names a monster
//! that died earlier in the round re-rolls a living slot on that side, rather
//! than walking at the corpse. Without it the attack short step `0x19`
//! (no timeout) held the round forever - the full-game ladder's map02
//! formation-21 stall.

use super::*;
use crate::battle_round::{PendingPartyAction, RoundPhase};
use vm::battle_action::ActionState;

/// One party member (slot 0) against three monsters (engine slots 1..=3).
fn world_one_vs_three() -> World {
    let mut world = World::new();
    world.party.party_count = 1;
    world.battle.player_driven = true;
    world.toggles.live_gameplay_loop = true;
    world.mode = SceneMode::Battle;
    for i in 0..4 {
        let a = &mut world.actors[i];
        a.active = true;
        a.battle.liveness = 1;
        a.battle.hp = 100;
        a.battle.max_hp = 100;
    }
    world.actors.truncate(4);
    world
}

fn kill(world: &mut World, slot: usize) {
    world.actors[slot].battle.hp = 0;
    world.actors[slot].battle.liveness = 0;
}

/// Put the round in its execution band with only `slot`'s turn left.
fn arm_turn(world: &mut World, slot: usize, action: PendingPartyAction) {
    for a in world.actors.iter_mut() {
        a.battle.init_key = 0;
    }
    world.actors[slot].battle.init_key = 1;
    world.battle.round_flow.phase = RoundPhase::Execute;
    world.battle.round_flow.pending[slot] = Some(action);
    world.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
}

#[test]
fn a_strike_at_a_dead_monster_rerolls_onto_a_living_one() {
    let mut world = world_one_vs_three();
    kill(&mut world, 1);
    kill(&mut world, 2);
    arm_turn(&mut world, 0, PendingPartyAction::Attack { target: 1 });
    world.cycle_battle_turn();
    assert_eq!(
        world.battle_ctx.active_actor, 0,
        "the member was dispatched"
    );
    assert_eq!(
        world.actors[0].battle.active_target, 3,
        "the only living monster takes the strike"
    );
    assert_eq!(world.actors[0].battle.action_category, 3);
}

#[test]
fn a_strike_at_a_living_monster_keeps_its_target_and_draws_nothing() {
    let mut world = world_one_vs_three();
    kill(&mut world, 3);
    let rng_before = world.rng_state;
    assert_eq!(world.redirect_dead_battle_target(2, 3, 0), 2);
    assert_eq!(
        world.rng_state, rng_before,
        "a living target spends no RNG draw (the redirect returns first)"
    );
    arm_turn(&mut world, 0, PendingPartyAction::Attack { target: 2 });
    world.cycle_battle_turn();
    assert_eq!(world.actors[0].battle.active_target, 2);
}

#[test]
fn the_redirect_stays_on_the_dead_targets_side_and_lands_alive() {
    let mut world = world_one_vs_three();
    kill(&mut world, 2);
    for _ in 0..32 {
        let t = world.redirect_dead_battle_target(2, 3, 0);
        assert!(t == 1 || t == 3, "rolled {t}");
        assert_ne!(world.actors[usize::from(t)].battle.hp, 0);
    }
    // Not a category the redirect covers (Spirit): left alone.
    assert_eq!(world.redirect_dead_battle_target(2, 4, 0), 2);
}
