//! A Spirit turn, the way a player sees it: the AP bar and plate the Spirit
//! arm raises (`World::spirit_gauge_view`, the view both hosts draw through
//! the arts-entry chrome) grow while the band plays, no readout bar sits over
//! them, and the next turn's arts entry opens on the extended gauge.

use super::*;
use legaia_engine_battle_menus::battle_input::{BattleCommandSession, CommandPhase};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::input::PadButton;

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
