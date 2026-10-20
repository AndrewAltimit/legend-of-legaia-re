//! Flow state `0x0B` of `FUN_801D0748` leaves only on the pass its intro
//! timer `ctx[+0x6D6]` expires (`bgtz` at `0x801D0E58`), and both of its
//! exits - `0x14`, or the execute arm `0xFE` when `ctx[+0x290]` reads `1` -
//! sweep the text actors on that pass (`jal 0x800355F0` at `0x801D0E7C` /
//! `0x801D0EB4`). No action runs under the enemy names, so a round the port
//! hands to the action SM with the labels still up takes them down.

use super::*;
use legaia_engine_mode::mode::SceneMode;

#[test]
fn a_round_handed_to_the_action_sm_sweeps_the_intro_names() {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    for i in 0..=1 {
        let b = &mut world.actors[i].battle;
        b.max_hp = 100;
        b.hp = 100;
        b.liveness = 1;
        world.battle.speed[i] = 20;
    }
    world.actors[1].battle_monster_id = Some(0x10);
    world.arm_battle_intro_names();
    assert_eq!(world.battle.intro_names_frames, 0x5A);
    world.begin_round_execution();
    assert_eq!(world.battle.intro_names_frames, 0);
}
