//! The Counterattack passive: the turn picker's roll arming the latch, and
//! the strike loop's swap handing a monster's strike to the countering
//! member (`World::roll_counterattack` / `World::begin_counterattack`).

use super::*;
use crate::battle_round::PendingPartyAction;
use legaia_engine_vm::battle_action::{ActionState, BattleActionHost};

/// Vahn (seat 0) committed a strike on the monster in seat 1, wearing the
/// Counterattack passive when `counter` is set.
fn counter_world(counter: bool) -> World {
    let mut world = World::new();
    world.load_party(legaia_save::Party::zeroed(1));
    world.party.party_count = 1;
    world.mode = SceneMode::Battle;
    if counter {
        let rec = &mut world.party.roster.members[0];
        let mut bits = rec.ability_bits();
        bits[1] |= 0x80; // word +0xF4, bit 0x8000
        rec.set_ability_bits(bits);
    }
    for i in 0..2 {
        world.actors[i].active = true;
        world.actors[i].battle.liveness = 1;
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    world.actors[0].battle.init_key = 30;
    world.battle.round_flow.pending[0] = Some(PendingPartyAction::Attack { target: 1 });
    world
}

/// The roll arms the latch only on a coin win, for a target that wears the
/// passive and still holds its turn - roughly half of many seeds.
#[test]
fn the_picker_arms_the_counter_latch_on_half_its_coins() {
    let mut armed = 0;
    for seed in 0..200u32 {
        let mut world = counter_world(true);
        world.rng_state = seed.wrapping_mul(0x9E37_79B9);
        world.roll_counterattack(1, 0);
        armed += usize::from(world.battle_ctx.counter_pending == 1);
    }
    assert!((60..=140).contains(&armed), "~half of 200, got {armed}");

    for seed in 0..50u32 {
        let mut world = counter_world(false);
        world.rng_state = seed.wrapping_mul(0x9E37_79B9);
        world.roll_counterattack(1, 0);
        assert_eq!(world.battle_ctx.counter_pending, 0, "no passive, no latch");
        let mut world = counter_world(true);
        world.actors[0].battle.init_key = 0;
        world.rng_state = seed.wrapping_mul(0x9E37_79B9);
        world.roll_counterattack(1, 0);
        assert_eq!(world.battle_ctx.counter_pending, 0, "the turn is spent");
    }
}

/// The swap through the world host: the latch on the monster's first strike
/// frame hands the loop to Vahn, whose committed strike is built and
/// retired, aimed back at the monster; the target plaque is cleared.
#[test]
fn the_strike_loop_hands_the_monsters_strike_to_the_counterer() {
    let mut world = counter_world(true);
    world.actors[1].battle.action_category = 3;
    world.actors[1].battle.active_target = 0;
    world.actors[1].battle.params[0] = 0x05;
    world.battle_ctx.active_actor = 1;
    world.battle_ctx.action_state = ActionState::AttackChain.as_byte();
    world.battle_ctx.counter_pending = 1;
    world.step_battle();
    assert_eq!(world.battle_ctx.active_actor, 0, "Vahn counters");
    assert_eq!(world.battle_ctx.counter_pending, 0);
    assert!(
        world.battle.round_flow.pending[0].is_none(),
        "the counter is Vahn's turn"
    );
    assert_eq!(world.actors[0].battle.active_target, 1);
    assert_eq!(world.actors[0].battle.init_key, 0);
    assert_ne!(world.actors[0].battle.queued_anim, 0, "Vahn's first swing");
    assert!(world.battle.target_plate_cleared);
    assert_eq!(crate::battle_hud::battle_target_plaque(&world), None);
}

/// Without a committed attack there is nothing to counter with.
#[test]
fn a_member_without_a_committed_attack_does_not_counter() {
    let mut world = counter_world(true);
    world.battle.round_flow.pending[0] = Some(PendingPartyAction::Spirit);
    let mut host = BattleHostImpl { world: &mut world };
    assert!(!host.counter_ready(0));
    assert!(!host.begin_counterattack(0, 1));
}

/// The timed message holds for its frames and then unloads itself.
#[test]
fn the_timed_message_unloads_when_its_hold_runs_out() {
    let mut world = World::new();
    world.mode = SceneMode::Battle;
    world.battle.message_banner = Some(crate::world::BattleMessageBanner {
        element: crate::world::TIMED_MESSAGE_ELEMENT,
        text: "line".into(),
        hold: 3,
    });
    world.clock.frame_step = 1;
    world.tick_timed_message();
    world.tick_timed_message();
    assert!(world.battle.message_banner.is_some());
    world.tick_timed_message();
    assert!(world.battle.message_banner.is_none());
}
