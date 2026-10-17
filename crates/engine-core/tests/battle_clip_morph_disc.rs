//! Disc-gated: a monster's playing clip blends its clip-morph lanes onto the
//! mesh both play hosts draw (`World::battle_morphed_tmd`, the port of
//! `FUN_800495C8`).
//!
//! Monster 7's idle loop (entry 0) names lane `+0x04` = 1 (keys `0, 9, 9,
//! 0x1D`) and lane `+0x08` = 2 (keys `0, 0x13, 0x13, 0x1D`). At cursor 0 both
//! weights are 0 and the rest mesh stands; mid-clip the blended mesh's
//! vertices move, only on the runs the lanes name, and the rest TMD is left
//! untouched. Skip-passes without `LEGAIA_DISC_BIN` or the extracted archive.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_actor_formats::monster_archive;
use legaia_engine_battle::battle_anim::MonsterAnimPlayer;
use legaia_engine_core::world::{Actor, World};

const MONSTER: u16 = 7;

fn archive() -> Option<Vec<u8>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let mut dirs = Vec::new();
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR") {
        dirs.push(PathBuf::from(d).join("PROT"));
    }
    dirs.push(PathBuf::from("../../extracted/PROT"));
    dirs.push(PathBuf::from("extracted/PROT"));
    for d in dirs {
        if let Ok(bytes) = std::fs::read(d.join("0867_battle_data.BIN")) {
            return Some(bytes);
        }
    }
    eprintln!("[skip] extracted PROT 0867 missing");
    None
}

fn verts(t: &legaia_tmd::Tmd) -> Vec<[i16; 3]> {
    t.objects
        .iter()
        .flat_map(|o| o.vertices.iter().map(|v| [v.x, v.y, v.z]))
        .collect()
}

#[test]
fn a_morphing_idle_moves_the_drawn_vertices_mid_clip() {
    let Some(archive) = archive() else {
        return;
    };
    let mesh = monster_archive::mesh(&archive, MONSTER).unwrap().unwrap();
    let tmd = legaia_tmd::parse(mesh.tmd_bytes()).unwrap();
    let idle = monster_archive::idle_animation(&archive, MONSTER)
        .unwrap()
        .unwrap();
    let table = monster_archive::clip_morph_table(&archive, MONSTER)
        .unwrap()
        .unwrap();

    let mut world = World::default();
    world.actors.push(Actor {
        active: true,
        battle_monster_id: Some(MONSTER),
        ..Actor::default()
    });
    world.set_actor_battle_animation(0, MonsterAnimPlayer::new(&idle).unwrap());
    world.set_actor_battle_clip_morphs(0, Arc::new(table));
    assert_eq!(
        world.actors[0]
            .battle_animation
            .as_ref()
            .unwrap()
            .morph_lanes(),
        [1, 2],
        "idle entry's +0x04 / +0x08 lane indices"
    );

    // Cursor 0: both envelopes start at weight 0 - the rest mesh draws.
    world.actors[0]
        .battle_animation
        .as_mut()
        .unwrap()
        .seek_sixteenths(0);
    assert!(world.battle_clip_morph_lanes(0).is_empty());
    assert!(world.battle_morphed_tmd(0, &tmd).is_none());

    // Frame 9: lane 1 at full weight, lane 2 ramping (9/19 of the way).
    world.actors[0]
        .battle_animation
        .as_mut()
        .unwrap()
        .seek_sixteenths(9 * 16);
    let lanes = world.battle_clip_morph_lanes(0);
    let weights: Vec<i16> = lanes.iter().map(|(_, w)| *w).collect();
    assert_eq!(
        weights,
        vec![0x1000, (((9 * 16i32) << 12) / (0x13 * 16)) as i16]
    );
    let morphed = world.battle_morphed_tmd(0, &tmd).expect("lanes active");

    let (rest, moved) = (verts(&tmd), verts(&morphed));
    assert_eq!(
        rest,
        verts(&legaia_tmd::parse(mesh.tmd_bytes()).unwrap()),
        "rest untouched"
    );
    let changed = rest.iter().zip(&moved).filter(|(a, b)| a != b).count();
    assert!(changed > 0, "mid-clip vertices move");
    // Only object 0's vertices are named by the slime's lanes.
    let obj0 = tmd.objects[0].vertices.len();
    assert!(rest[obj0..] == moved[obj0..], "other objects stay at rest");

    // The drawn mesh (the builder both hosts pose) moves with it.
    let a = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, mesh.tmd_bytes()).positions;
    let b = legaia_tmd::mesh::tmd_to_vram_mesh(&morphed, mesh.tmd_bytes()).positions;
    assert_eq!(a.len(), b.len());
    assert!(a != b, "the uploaded positions differ mid-clip");
    eprintln!(
        "[clip-morph] {changed} of {} vertices moved at frame 9",
        rest.len()
    );
}
