//! `FUN_801CF8AC`'s collision-exempt early-out: an ambient walker whose
//! placement context carries `+0x10 & 3` (op `0x31` bit 0 / 1) never stops
//! for the player; one without it does.

use super::*;

/// A field world with the player standing one probe ahead of an ambient
/// walker in slot 5, whose stream is a `+Z` directional step.
fn walker_world(flags: u32) -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.install_field_player(0);
    w.npcs.animate = true;
    // `0x03`, LUT 4 (+Z), pace bits 0, three tiles.
    let code = vec![0x03, 4 << 4, 3];
    w.npcs.ambient.insert(
        5,
        FieldNpcAmbient {
            walks: true,
            variants: vec![(legaia_asset::man_motion::SELECTOR_DEFAULT, code)],
            live: None,
            vm: vm::ambient_motion::AmbientMotion::new(5, 0).with_position(1000, 2000),
        },
    );
    // The +Z probe is `(x, z + 64)`: stand the player there.
    w.actors[0].move_state.world_x = 1000;
    w.actors[0].move_state.world_z = 2064;
    w.field_vm
        .channels
        .push(crate::field_channels::FieldChannel {
            placement_index: 5,
            ctx: FieldCtx {
                flags,
                ..FieldCtx::default()
            },
            record_offset: 0,
            pc: 0,
            done: false,
            object_bind: false,
        });
    w
}

fn walker_z(w: &World) -> i16 {
    w.npcs.ambient[&5].vm.z
}

#[test]
fn the_player_stops_an_ordinary_walker() {
    let mut w = walker_world(0);
    for _ in 0..4 {
        w.tick_field_npc_ambient();
    }
    assert_eq!(walker_z(&w), 2000, "the class arm refuses the step");
}

#[test]
fn a_collision_exempt_walker_walks_through_the_player() {
    for bit in [1u32, 2] {
        let mut w = walker_world(bit);
        for _ in 0..4 {
            w.tick_field_npc_ambient();
        }
        assert!(
            walker_z(&w) > 2000,
            "`+0x10 & {bit}` returns 0 before the box test (0x801CF8B8)"
        );
    }
}
