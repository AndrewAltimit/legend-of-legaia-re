//! One script context per bound `.MAP` object (`FUN_8003A55C`), several on
//! one record included: what a cross-context op reaches and what each
//! object's draw reads.

use crate::world::World;
use legaia_engine_field::field_channels::object_instance_key;
use legaia_engine_field::man_field_scripts::ObjectScriptBind;
use legaia_scene_formats::man_section::{ManFile, ManHeader, SectionRef};

/// A MAN of one partition-0 record (`[n = 0][anim]`, then an idle loop).
fn one_record_man() -> (ManFile, Vec<u8>) {
    let data_region_offset = 0x40usize;
    let mut man = vec![0u8; data_region_offset];
    man.extend_from_slice(&[0x00, 0x00, 0x21, 0x26, 0xFE, 0xFF]);
    let man_file = ManFile {
        header: ManHeader {
            status_flags: 0,
            low_flag: false,
            depth_lut: [0; 16],
            partition_counts: [1, 0, 0],
            u24_at_28: 0,
        },
        partitions: [vec![0u32], vec![], vec![]],
        data_region_offset,
        sections: std::array::from_fn(|_| SectionRef {
            offset: man.len(),
            length: 0,
        }),
    };
    (man_file, man)
}

fn bind(record: usize, tile: (u8, u8)) -> ObjectScriptBind {
    ObjectScriptBind {
        tile,
        key_tile: tile,
        ..ObjectScriptBind::at(
            record,
            (
                i16::from(tile.0) * 128 + 0x40,
                i16::from(tile.1) * 128 + 0x40,
            ),
        )
    }
}

/// Three objects on record 0 are three contexts with one script id. A
/// `B1 00 03` reaches the first still linked: `rikuroa`'s summit beat
/// retires six of record 0's props, one per `B1 00 03`, eight frames apart.
/// Each object's draw reads its own actor.
#[test]
fn objects_sharing_a_record_are_separate_contexts() {
    let (man_file, man) = one_record_man();
    let mut w = World::default();
    w.seed_field_channels(&man_file, &man);
    let tiles = [(10u8, 10u8), (12, 10), (14, 10)];
    let binds: Vec<_> = tiles.iter().map(|&t| bind(0, t)).collect();
    w.seed_object_channels(&man_file, &man, &binds);
    let keys: Vec<(usize, u16)> = w
        .field_vm
        .channels
        .iter()
        .filter(|c| c.object_bind)
        .map(|c| (c.placement_index, c.ctx.script_id))
        .collect();
    assert_eq!(
        keys,
        [
            (0, 0),
            (object_instance_key(tiles[1]), 0),
            (object_instance_key(tiles[2]), 0)
        ],
        "the first object keeps the record key, the rest their own"
    );
    assert!(w.hidden_object_records().is_empty());

    // The first `B1 00 03`: only the first object leaves.
    w.field_vm.pending_retire_ids.push(0);
    w.sweep_field_channel_lifetimes();
    let hidden = w.hidden_object_records();
    assert!(hidden.contains(&object_instance_key(tiles[0])));
    assert!(!hidden.contains(&object_instance_key(tiles[1])));
    assert!(!hidden.contains(&object_instance_key(tiles[2])));
    // The second reaches the next object, the actor the walk now finds first.
    w.field_vm.pending_retire_ids.push(0);
    w.sweep_field_channel_lifetimes();
    let hidden = w.hidden_object_records();
    assert!(hidden.contains(&object_instance_key(tiles[1])));
    assert!(!hidden.contains(&object_instance_key(tiles[2])));

    // A move on the last object displaces its draw alone.
    let last = w
        .field_vm
        .channels
        .iter_mut()
        .find(|c| c.object_bind)
        .expect("the third object is still linked");
    last.ctx.world_x = last.ctx.world_x.wrapping_add(128);
    let moves = w.object_draw_displacements();
    assert_eq!(
        moves.get(&object_instance_key(tiles[2])),
        Some(&[128, 0, 0])
    );
    assert_eq!(moves.len(), 1);
}

/// A record index past the MAN's record count still spawns a context: the
/// sweep runs the SCUS default record (zeros, nothing to run) under that id.
#[test]
fn an_out_of_range_record_binds_an_inert_context() {
    let (man_file, man) = one_record_man();
    let mut w = World::default();
    w.seed_field_channels(&man_file, &man);
    w.seed_object_channels(&man_file, &man, &[bind(0, (3, 3)), bind(9, (5, 3))]);
    let c = w
        .field_vm
        .channels
        .iter()
        .find(|c| c.object_bind && c.ctx.script_id == 9)
        .expect("record 9 answers its id");
    assert!(c.done, "the default record runs nothing");
    assert_eq!(
        legaia_engine_field::field_channels::resolve_target(&w.field_vm.channels, 9),
        w.field_vm
            .channels
            .iter()
            .position(|c| c.ctx.script_id == 9)
    );
}

/// A floor follower (`+0x10 & 0x20000`, `FUN_8003BC08`'s snap arm) is baked on
/// the floor under the ladder the scene spawned its objects against, and a
/// ladder a script installs later reaches the draw as this frame's
/// displacement - whenever the host built its placed layer. `rikuroa`'s
/// summit arm (`4C 9E`, rung 8 to `-920`) is the case: a cliff baked on the
/// live floor at scene setup stood `664` units under the player.
#[test]
fn a_floor_follower_rides_a_ladder_installed_after_spawn() {
    let (man_file, man) = one_record_man();
    let mut w = World {
        mode: legaia_engine_mode::mode::SceneMode::Field,
        ..World::default()
    };
    w.seed_field_channels(&man_file, &man);
    let tile = (10u8, 10u8);
    w.seed_object_channels(&man_file, &man, &[bind(0, tile)]);
    let c = w
        .field_vm
        .channels
        .iter_mut()
        .find(|c| c.object_bind)
        .expect("the object's context");
    c.ctx.flags |= 0x0002_0000;
    // Every tile on tier 8; the spawn ladder is the header's linear ramp.
    w.terrain.collision_grid = vec![8u8; 0x4000];
    w.terrain.object_cells = vec![0u16; 0x4000];
    let linear: [i16; 16] = std::array::from_fn(|i| -(i as i16) * 32);
    w.terrain.placed_spawn_lut = linear;
    w.terrain.floor_height_lut = linear;
    let follow = w.object_floor_follow_records();
    assert!(follow.contains(&0));
    let (x, z) = (
        i32::from(tile.0) * 128 + 0x40,
        i32::from(tile.1) * 128 + 0x40,
    );
    assert_eq!(w.object_floor_follow_y(&follow, 0, x, z), Some(-256));
    assert!(w.object_draw_displacements().is_empty());

    // The script installs the summit ladder: the bake stays, the draw rises.
    w.terrain.floor_height_lut[8] = -920;
    assert_eq!(w.object_floor_follow_y(&follow, 0, x, z), Some(-256));
    assert_eq!(w.object_draw_displacements().get(&0), Some(&[0, -664, 0]));
}
