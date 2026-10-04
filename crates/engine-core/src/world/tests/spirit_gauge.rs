//! A Spirit turn, the way a player sees it: the AP bar and plate the Spirit
//! arm raises (`World::spirit_gauge_view`, the view both hosts draw through
//! the arts-entry chrome) grow while the band plays, no readout bar sits over
//! them, and the next turn's arts entry opens on the extended gauge.

use super::*;
use crate::battle_hud::battle_readout_bar_slot;
use crate::battle_input::{BattleCommandSession, CommandPhase};
use crate::input::PadButton;
use vm::battle_action::ActionState;

fn spirit_world() -> World {
    let mut world = World::new();
    world.party.party_count = 1;
    world.battle.player_driven = true;
    world.toggles.live_gameplay_loop = true;
    world.mode = SceneMode::Battle;
    for i in 0..3 {
        world.actors[i].active = true;
        world.actors[i].battle.liveness = 1;
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    // A battle-entry seeded gauge (`+0x154` / `+0x156`), the retail capture's
    // 194 AGL.
    world.actors[0].battle.agl = 194;
    world.actors[0].battle.agl_base = 194;
    world
}

fn commit_spirit_and_begin(world: &mut World) {
    world.battle.command = Some(BattleCommandSession {
        actor: 0,
        party_slot: 0,
        no_escape: false,
        phase: CommandPhase::SpiritGuard,
    });
    world.tick_battle_command();
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_command();
    world.set_pad(0);
}

#[test]
fn a_spirit_turn_draws_a_growing_ap_bar_and_no_readout_bar() {
    let mut world = spirit_world();
    assert!(
        world.spirit_gauge_view().is_none(),
        "nothing before the turn"
    );
    commit_spirit_and_begin(&mut world);

    let mut bars = Vec::new();
    let mut plates = Vec::new();
    for _ in 0..3000 {
        world.tick();
        let s = world.battle_ctx.action_state;
        if world.battle_ctx.active_actor == 0
            && (ActionState::SpiritArtsEntry.as_byte()..=ActionState::SpiritArtsFlush.as_byte())
                .contains(&s)
        {
            let view = world
                .spirit_gauge_view()
                .expect("the Spirit band carries the AP bar + plate");
            assert!(view.pennants.is_empty() && !view.chips_visible());
            bars.push(view.pool_max);
            plates.push(view.plate_value);
            assert_eq!(
                battle_readout_bar_slot(&world),
                None,
                "the Spirit arm raises no readout bar over the gauge"
            );
        }
        if world.battle_ctx.action_state == ActionState::DoneCleanup.as_byte() && !bars.is_empty() {
            break;
        }
    }
    assert!(!bars.is_empty(), "the Spirit band ran");
    assert_eq!(bars.first().copied(), Some(194), "opens on the live gauge");
    // 194 * 7 / 5 + 8 = 279: the bar grows to the extended gauge.
    assert_eq!(bars.last().copied(), Some(279));
    assert!(bars.windows(2).all(|w| w[1] >= w[0]));
    assert_eq!(plates.last().copied(), Some(0x20), "the plate climbs +0x20");
}

#[test]
fn the_next_arts_entry_opens_on_the_spirit_extended_gauge() {
    let mut world = spirit_world();
    commit_spirit_and_begin(&mut world);
    // Play the round out to the boundary, which restores a Spirit-charged
    // actor's gauge to the extended value (`FUN_801D88CC`).
    for _ in 0..4000 {
        world.tick();
        if world.actors[0].battle.agl == 279 {
            break;
        }
    }
    assert_eq!(world.actors[0].battle.agl, 279, "the boundary extended it");
    world.open_arts_command_input(0);
    let view = world.arts_input_view().expect("the entry is open");
    assert_eq!(view.pool_max, 279, "the entry pool is the live gauge");
}
