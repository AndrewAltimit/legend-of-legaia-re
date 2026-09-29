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

/// The whole monster side down mid-round: the next idle is not a dispatch
/// at a corpse but the wipe scan, and the fight resolves as a victory. The
/// round still owes the party member its strike (committed before the
/// kill), which is exactly the shape that used to walk at the dead monster.
#[test]
fn a_monster_side_wiped_mid_round_ends_in_victory() {
    let mut world = world_one_vs_three();
    for slot in 1..=3 {
        kill(&mut world, slot);
    }
    arm_turn(&mut world, 0, PendingPartyAction::Attack { target: 1 });
    let mut won = false;
    for _ in 0..2_000 {
        world.live_battle_tick();
        assert_ne!(
            world.battle_ctx.action_state,
            ActionState::AttackShortStep.as_byte(),
            "the member walked at a corpse"
        );
        if world.battle.victory.is_some() || world.battle.end.is_some() {
            won = true;
            break;
        }
    }
    assert!(won, "the wiped monster side never resolved the fight");
    assert!(
        world.battle.round_flow.pending[0].is_some(),
        "the strike committed before the wipe was never dispatched"
    );
}

/// A revive stands its target back up. Retail's `+0x14C` is HP and liveness
/// at once; the port keeps two fields, and a revive item that raised HP alone
/// left the member alive to the monster AI but down to every liveness scan,
/// still kneeling in its downed chain - whose pose puts the body pair the
/// range law measures out of an attacker's reach, so the monster's attack
/// short step `0x19` (no timeout) held the round forever (the full-game
/// ladder's taiku formation-169 stall).
#[test]
fn a_revive_item_stands_the_downed_member_back_up() {
    // The tag-8 kneel's entry and the root latch its commit raises.
    const PARTY_DOWNED_LOOP_ENTRY: u8 = 8;
    const ANIM_FLAG_ROOT_LATCH: u8 = vm::battle_action::ActorFlags::FX_SUPPRESSED;
    let mut world = world_one_vs_three();
    world.tables.item_catalog = crate::items::ItemCatalog::vanilla();
    kill(&mut world, 0);
    world.actors[0].battle_reaction = Some(PARTY_DOWNED_LOOP_ENTRY);
    world.actors[0].battle.flag_bits.set(ANIM_FLAG_ROOT_LATCH);
    let outcome = world.apply_battle_item(0x80, 0);
    assert!(
        matches!(outcome, crate::items::ItemOutcome::Revived { .. }),
        "{outcome:?}"
    );
    let a = &world.actors[0];
    assert!(a.battle.hp > 0);
    assert_ne!(a.battle.liveness, 0, "revived HP without liveness");
    assert_eq!(a.battle_reaction, None, "still kneeling after the revive");
    assert!(!a.battle.flag_bits.has(ANIM_FLAG_ROOT_LATCH));
    assert!(!world.actor_effectively_defeated(0));
}
