//! The field NPC `0x18` wander and `0x06` home-step `rand()` draws run on the
//! world's stream, in the actor list's tick order.
//!
//! `FUN_80038158` draws through the BIOS thunk (`FUN_80056798`), one seed
//! for the whole process, and the actor list `FUN_8002519C` walks head to
//! tail is spawn order. So two channels' draws land on successive stream
//! words, in list order, and the world's `rng_state` advances with them.

use legaia_engine_core::world::{FieldNpcAmbient, World};
use legaia_engine_motion_vm::ambient_motion::AmbientMotion;
use legaia_engine_system::rand::world_lcg_step;

fn channel(slot: u8, code: Vec<u8>) -> FieldNpcAmbient {
    FieldNpcAmbient {
        defers: false,
        walks: true,
        variants: vec![(legaia_scene_formats::man_motion::SELECTOR_DEFAULT, code)],
        live: None,
        // Tile `(5, 5)`, centred in the wander box below.
        vm: AmbientMotion::new(u32::from(slot), 0).with_position(704, 704),
    }
}

/// `[18, min x, min z, max x, max z]`, a box round tile `(5, 5)`.
const WANDER: [u8; 5] = [0x18, 1, 1, 10, 10];

fn dir(state: u32) -> u8 {
    legaia_engine_system::rand::bios_rand_shape(state) as u8 & 6
}

/// A seed whose first two stream words pick different wander directions, so
/// the assertions can tell the two draws apart.
fn distinct_seed() -> u32 {
    (1u32..)
        .find(|&s| dir(world_lcg_step(s)) != dir(world_lcg_step(world_lcg_step(s))))
        .unwrap()
}

#[test]
fn a_wander_draw_advances_the_world_stream_and_is_the_seed_word() {
    let mut w = World::new();
    let seed = distinct_seed();
    w.rng_state = seed;
    w.npcs.ambient.insert(7, channel(7, WANDER.to_vec()));
    w.tick_field_npc_ambient();
    let next = world_lcg_step(seed);
    assert_eq!(w.rng_state, next, "one LCG step on the world's stream");
    assert_eq!(w.npcs.ambient[&7].vm.wander_dir, dir(next));
    assert_eq!(w.npcs.ambient[&7].vm.rand_draws, 1);
}

#[test]
fn a_home_step_draws_from_the_world_stream() {
    let mut w = World::new();
    let seed = 0x5EED_0001;
    w.rng_state = seed;
    // `[06, b1..b4]`: the home-relative box, wide enough that it does not
    // retire on the draw.
    w.npcs
        .ambient
        .insert(7, channel(7, vec![0x06, 0x02, 0x02, 0x02, 0x02]));
    w.tick_field_npc_ambient();
    assert_eq!(w.rng_state, world_lcg_step(seed));
    assert_eq!(w.npcs.ambient[&7].vm.rand_draws, 1);
}

/// Two channels' draws land in `ambient_order`, not the map's slot order:
/// the list is spawn order, and a `4C D2` re-seat links a node at the tail.
#[test]
fn draws_land_in_the_actor_list_order_not_slot_order() {
    let mut w = World::new();
    let seed = distinct_seed();
    w.rng_state = seed;
    w.npcs.ambient.insert(2, channel(2, WANDER.to_vec()));
    w.npcs.ambient.insert(5, channel(5, WANDER.to_vec()));
    w.npcs.ambient_order = vec![5, 2];
    w.tick_field_npc_ambient();
    let s1 = world_lcg_step(seed);
    let s2 = world_lcg_step(s1);
    assert_eq!(w.rng_state, s2);
    assert_eq!(
        w.npcs.ambient[&5].vm.wander_dir,
        dir(s1),
        "head of the list first"
    );
    assert_eq!(w.npcs.ambient[&2].vm.wander_dir, dir(s2));
}

#[test]
fn a_retired_channel_draws_nothing() {
    let mut w = World::new();
    let seed = 0x5EED_0001;
    w.rng_state = seed;
    w.npcs.ambient.insert(7, channel(7, WANDER.to_vec()));
    w.field_vm.retired_placements.insert(7);
    w.tick_field_npc_ambient();
    assert_eq!(w.rng_state, seed);
}

/// A `0x23` / `4C 51` on a placement relinks it at the list's tail
/// (`FUN_801D81E0`, `FUN_800204A4` then `FUN_80020454`), so it draws last.
#[test]
fn a_seat_op_relinks_the_placement_at_the_tail() {
    let mut w = World::new();
    w.npcs.ambient.insert(2, channel(2, WANDER.to_vec()));
    w.npcs.ambient.insert(5, channel(5, WANDER.to_vec()));
    w.npcs.ambient.insert(9, channel(9, WANDER.to_vec()));
    w.npcs.ambient_order = vec![2, 5, 9];
    w.relink_placement_for_seat(5);
    assert_eq!(w.npcs.ambient_order, vec![2, 9, 5]);
}

/// In a scene entry's spawn prologue the running placement is the list's
/// tail and later placements are not linked yet: its own seat op changes
/// nothing, a seat of an earlier placement lands behind the running one and
/// ahead of the unspawned ones.
#[test]
fn a_seat_op_inside_the_spawn_prologue_respects_the_spawn_loop() {
    let mut w = World::new();
    for slot in [2u8, 5, 9] {
        w.npcs.ambient.insert(slot, channel(slot, WANDER.to_vec()));
    }
    w.npcs.ambient_order = vec![2, 5, 9];
    w.field_vm.prerun_running_slot = Some(5);
    w.relink_placement_for_seat(5);
    assert_eq!(
        w.npcs.ambient_order,
        vec![2, 5, 9],
        "the running slot's own seat"
    );
    w.relink_placement_for_seat(2);
    assert_eq!(
        w.npcs.ambient_order,
        vec![5, 2, 9],
        "behind the running slot"
    );
}
