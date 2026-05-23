//! Disc-gated: the field static-object placement table (`FUN_8003A55C`)
//! parsed from `town01`'s field map file reproduces the known building
//! anchors. Validates [`legaia_asset::field_objects`] + the
//! [`Scene::field_object_placements`] wiring against real disc bytes.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.
use std::path::PathBuf;

use legaia_engine_core::scene::{Scene, SceneHost};

fn extracted_dir() -> Option<PathBuf> {
    for d in ["extracted", "../../extracted"] {
        let p = PathBuf::from(d);
        if p.join("PROT.DAT").exists() && p.join("CDNAME.TXT").exists() {
            return Some(p);
        }
    }
    None
}

#[test]
fn town01_placements_reproduce_building_anchors() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let index = host.index.clone();
    let scene = Scene::load(&index, "town01").expect("load town01");

    let placements = scene
        .field_object_placements(&index)
        .expect("read placements")
        .expect("town01 has a field map");

    // town01's environment is laid out by a few dozen placed objects (the
    // retail static-object actor count via FUN_8003A55C). A regression that
    // mis-reads the object grid / record table would collapse this to ~0 or
    // explode it across the whole 16384-tile grid.
    assert!(
        (20..=200).contains(&placements.len()),
        "town01 placed-object count out of plausible range: {} (expected 20..=200)",
        placements.len()
    );

    // Vahn's house: object id 137, anchor tile (col 38, row 25), world
    // (4864, _, 3208) - byte-validated against the live town01 actor.
    let vahns_house = placements
        .iter()
        .find(|p| p.obj_idx == 137)
        .unwrap_or_else(|| panic!("town01 placements missing object id 137 (Vahn's house)"));
    assert_eq!(
        (vahns_house.col, vahns_house.row),
        (38, 25),
        "Vahn's house anchor tile"
    );
    assert_eq!(
        (vahns_house.world_x, vahns_house.world_z),
        (4864, 3208),
        "Vahn's house world position"
    );
    // Mesh selection (byte-verified against the live actor's geometry): the
    // house (id 137, >=120) draws scene-pack mesh 36 via the record's +0x10
    // field; the windmill (id 96, in the 93..=118 band) draws mesh 91 (96-5).
    assert_eq!(vahns_house.pack_index, Some(36), "Vahn's house pack mesh");
    if let Some(windmill) = placements.iter().find(|p| p.obj_idx == 96) {
        assert_eq!(windmill.pack_index, Some(91), "windmill pack mesh (96-5)");
    }
    if let Some(obj230) = placements.iter().find(|p| p.obj_idx == 230) {
        assert_eq!(obj230.pack_index, Some(15), "obj230 pack mesh (+0x10)");
    }

    // Every placement lands on-grid with the placed flag set, and within the
    // ~16384-unit town extent (128 tiles * 128 units).
    for p in &placements {
        assert!(p.flags & legaia_asset::field_objects::FLAG_PLACED != 0);
        assert!((0..128).contains(&p.col) && (0..128).contains(&p.row));
        assert!(
            (0..0x4040).contains(&p.world_x) && (0..0x4040).contains(&p.world_z),
            "placement off the town extent: ({}, {})",
            p.world_x,
            p.world_z
        );
    }

    eprintln!(
        "town01: {} placed static objects; Vahn's house @ tile (38,25) -> (4864, 3208)",
        placements.len()
    );
}
