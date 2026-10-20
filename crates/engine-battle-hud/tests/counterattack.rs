//! The Counterattack passive: the turn picker's roll arming the latch, and
//! the strike loop's swap handing a monster's strike to the countering
//! member (`World::roll_counterattack` / `World::begin_counterattack`).
//!
//! These are the cases that read the battle HUD's projections; the ones that
//! read only the `World` stay in engine-core's `world::tests::counterattack`.

use legaia_engine_battle_vm::battle_action::ActionState;
use legaia_engine_core::battle_round::PendingPartyAction;
use legaia_engine_core::world::*;
use legaia_engine_mode::mode::SceneMode;

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
    assert_eq!(legaia_engine_battle_hud::battle_target_plaque(&world), None);
    // The HUD stays the monster's seed's: the bar for its target (Vahn),
    // and no combo cluster (`battle_vahn_tri_somersault_super`).
    assert_eq!(world.battle.counter_hud, Some(0));
    assert_eq!(legaia_engine_battle_hud::battle_combo_style(&world), None);
    assert_eq!(
        legaia_engine_battle_hud::battle_readout_bar_slot(&world),
        Some(0)
    );
}
