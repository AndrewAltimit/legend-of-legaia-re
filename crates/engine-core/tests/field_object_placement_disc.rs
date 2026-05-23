//! Disc-gated: the field static-object placement table (`FUN_8003A55C`)
//! parsed from `town01`'s field map file reproduces the known building
//! anchors. Validates [`legaia_asset::field_objects`] + the
//! [`Scene::field_object_placements`] wiring against real disc bytes.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.
use std::path::PathBuf;

use legaia_engine_core::scene::{Scene, SceneHost};
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};

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

    // Floor-height LUT: Vahn's house tile floor nibble is 6, lut[6] = 192, so
    // its world Y is -192 (matches the live actor). Validates the MAN-header
    // LUT accessor + the `-lut[nibble] + y_off` elevation formula.
    let lut = scene
        .field_floor_height_lut(&index)
        .expect("read floor LUT")
        .expect("town01 MAN carries a floor LUT");
    let nib = vahns_house
        .floor_nibble
        .expect("Vahn's house tile floor nibble");
    let world_y = -(lut[(nib & 0x0F) as usize] as i32) + vahns_house.y_off as i32;
    assert_eq!(world_y, -192, "Vahn's house world Y from floor LUT");
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

/// The render mapping is sound: every placement's `pack_index` lands inside
/// the scene_asset_table TMD pack the engine loads, so the play-window
/// static-draw pass resolves a real mesh for each placed object.
#[test]
fn town01_placement_pack_indices_resolve_in_loaded_pack() {
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
        .expect("field map");

    // The environment meshes are the scene bundle entry's TMD pack, in scan
    // order (the same indexing `pack_index` uses).
    let bundle =
        legaia_engine_core::scene_bundle::find_bundle(&scene).expect("town01 has a scene bundle");
    let bundle_entry = bundle.entry_idx();

    let mut shared: Vec<Scene> = Vec::new();
    for name in FIELD_SHARED_BLOCKS {
        if let Ok(s) = Scene::load(&index, name) {
            shared.push(s);
        }
    }
    let refs: Vec<&Scene> = shared.iter().collect();
    let (res, _) = SceneResources::build_targeted_with_options(
        &scene,
        &refs,
        BuildOptions {
            kind: SceneLoadKind::Field,
            upload_all_tims: true,
        },
    )
    .expect("build town01 field");

    let env_mesh_count = res
        .tmds
        .iter()
        .filter(|t| t.entry_idx == bundle_entry)
        .count();
    assert!(
        env_mesh_count >= 100,
        "town01 env mesh pack unexpectedly small: {env_mesh_count}"
    );

    let mut resolved = 0usize;
    for p in &placements {
        if let Some(pack_index) = p.pack_index {
            assert!(
                (pack_index as usize) < env_mesh_count,
                "object {} pack_index {} out of range (env pack has {})",
                p.obj_idx,
                pack_index,
                env_mesh_count
            );
            resolved += 1;
        }
    }
    assert!(
        resolved >= 10,
        "expected most placements to resolve a mesh; only {resolved} did"
    );
    eprintln!(
        "town01: {resolved}/{} placements resolve into the {env_mesh_count}-mesh pack",
        placements.len()
    );
}
