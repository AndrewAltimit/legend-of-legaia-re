//! The placement spawner's clip-phase `rand()` draw (`FUN_8003A1E4`,
//! `0x8003A410`): one draw per placement with a move, in spawn order, before
//! the record's prologue slice.

use crate::world::World;
use legaia_scene_formats::man_section::{ManFile, ManHeader, SectionRef};

/// A MAN whose partition-1 records carry `anims` as their header anim byte
/// (`[n = 0][model][anim][tile x][tile z]`, then an idle `21`).
fn man_with_placements(anims: &[u8]) -> (ManFile, Vec<u8>) {
    let data_region_offset = 0x40usize;
    let mut man = vec![0u8; data_region_offset];
    let mut offsets = Vec::new();
    for &anim in anims {
        offsets.push((man.len() - data_region_offset) as u32);
        man.extend_from_slice(&[0x00, 0x01, anim, 0x05, 0x05, 0x21]);
    }
    let man_file = ManFile {
        header: ManHeader {
            status_flags: 0,
            low_flag: false,
            depth_lut: [0; 16],
            partition_counts: [0, anims.len() as i16, 0],
            u24_at_28: 0,
        },
        partitions: [vec![], offsets, vec![]],
        data_region_offset,
        sections: std::array::from_fn(|_| SectionRef {
            offset: man.len(),
            length: 0,
        }),
    };
    (man_file, man)
}

fn step(seed: u32) -> u32 {
    legaia_engine_system::rand::world_lcg_step(seed)
}

#[test]
fn each_placement_with_a_move_draws_once_in_spawn_order() {
    // Record 0 is the scene controller (the spawn loop runs `1..N1`); the
    // second and fourth placements carry a move.
    let (man_file, man) = man_with_placements(&[9, 3, 0, 7]);
    let mut w = World::new();
    w.rng_state = 0x0ABC_1234;
    w.draw_placement_spawn_phases(&man_file, &man);
    let s1 = step(0x0ABC_1234);
    let s2 = step(s1);
    assert_eq!(w.rng_state, s2, "one draw per placement with a move");
    let shape = legaia_engine_system::rand::bios_rand_shape;
    assert_eq!(w.npcs.phase_draws.get(&1), Some(&shape(s1)));
    assert_eq!(w.npcs.phase_draws.get(&3), Some(&shape(s2)));
    assert!(!w.npcs.phase_draws.contains_key(&2), "no move, no draw");
    assert!(
        !w.npcs.phase_draws.contains_key(&0),
        "the controller is not spawned"
    );
}
