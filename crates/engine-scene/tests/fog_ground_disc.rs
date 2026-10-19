//! Disc-gated: the volumetric fog's ground coverage field
//! (`legaia_engine_scene::fog_ground`) follows the drawn floor.
//!
//! Skips when `LEGAIA_DISC_BIN` is unset (disc-gated convention).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_scene::field_env;
use legaia_engine_scene::fog_ground;
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_engine_scene::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};
use legaia_render_kernels::fog_volume::{FogGround, HAZE_WEIGHT};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_index() -> Option<Arc<ProtIndex>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return None;
    };
    Some(Arc::new(
        ProtIndex::open_extracted(&extracted).expect("open prot index"),
    ))
}

/// Build one scene's field the way the hosts do (the static terrain +
/// placement draws into `fog_ground::build_for_scene`), with its walk grid.
fn scene_field(index: &Arc<ProtIndex>, name: &str) -> (FogGround, Vec<u8>) {
    let shared: Vec<Scene> = FIELD_SHARED_BLOCKS
        .iter()
        .filter_map(|n| Scene::load(index, n).ok())
        .collect();
    let shared_refs: Vec<&Scene> = shared.iter().collect();
    let scene = Scene::load(index, name).expect("load scene");
    let (res, _) = SceneResources::build_targeted_with_options(
        &scene,
        &shared_refs,
        BuildOptions {
            kind: SceneLoadKind::Field,
            upload_all_tims: true,
            system_ui: index.system_ui_bundle().ok().as_deref(),
        },
    )
    .expect("build resources");
    let env_tmds = field_env::env_pack_tmd_indices(&scene, &res);
    let floor_lut = scene.field_floor_height_lut(index).ok().flatten();
    let tiles: Vec<_> = scene
        .field_terrain_tiles(index)
        .ok()
        .flatten()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.flags & legaia_scene_formats::field_objects::FLAG_PLACED == 0)
        .collect();
    let (terrain, _) = field_env::resolve_env_draws(&env_tmds, &tiles, floor_lut);
    let placements_rec = scene
        .field_object_placements(index)
        .ok()
        .flatten()
        .unwrap_or_default();
    let binds = scene.field_object_binds(index).ok().flatten();
    let (placements, _) =
        field_env::resolve_placed_env_draws(&env_tmds, &placements_rec, floor_lut, binds.as_ref());
    let field = fog_ground::build_for_scene(&terrain, &placements, &[], &res, &scene, index)
        .expect("a field scene builds a field");
    let grid = scene
        .field_collision_grid(index)
        .ok()
        .flatten()
        .unwrap_or_default();
    (field, grid)
}

/// World X / Z of a walk tile's centre.
fn tile(tx: i32, tz: i32) -> (f32, f32) {
    ((tx * 128 + 64) as f32, (tz * 128 + 64) as f32)
}

/// The Sky Gardens (`teien`): a tower top ringed by open sky. Its walk grid
/// covers the whole tile map (every tile outside the tower reads as plain
/// floor), so the field is the only thing that knows the sky is there.
#[test]
fn sky_gardens_has_open_air_around_the_tower_top() {
    let Some(index) = open_index() else {
        return;
    };
    let (f, grid) = scene_field(&index, "teien");
    // The walk grid alone: tiles far outside the tower carry no wall bits,
    // which is why the bank used to lie on them.
    for (tx, tz) in [(10, 40), (80, 40), (42, 10), (42, 80)] {
        assert_eq!(
            grid[tz as usize * 0x80 + tx as usize] & 0xF0,
            0,
            "tile ({tx},{tz})"
        );
    }
    // The garden in the middle of the tower top is floor...
    let (cx, cz) = tile(42, 40);
    assert!(f.covered_at(cx, cz), "tower top centre");
    assert!(
        f.sheet_at(cx, cz)
            .is_some_and(|p| !p.spill && p.weight == 1.0)
    );
    // ...and so is the hedge maze inside it: the hedge tops are enclosed,
    // not holes to the sky.
    for tz in 32..50 {
        let (x, z) = tile(42, tz);
        assert!(f.covered_at(x, z), "tower top at tile (42,{tz})");
    }
    // Open air all round, well clear of the rim: no bank, at most the
    // faint distant haze far below the tower top.
    let top_y = f.height[f.cell_index(cx, cz).unwrap()];
    for (tx, tz) in [(18, 40), (68, 40), (42, 16), (42, 66)] {
        let (x, z) = tile(tx, tz);
        assert!(!f.covered_at(x, z), "open air at tile ({tx},{tz})");
        assert!(
            f.sheet_at(x, z)
                .is_none_or(|p| p.haze && p.weight <= HAZE_WEIGHT && p.y > top_y + 1000.0),
            "no bank at tile ({tx},{tz})"
        );
    }
    // The void around the tower carries the haze somewhere.
    let hazy = (0..f.h)
        .flat_map(|z| (0..f.w).map(move |x| (x, z)))
        .filter(|&(x, z)| {
            let [px, pz] = f.centre_of(z * f.w + x);
            f.sheet_at(px, pz).is_some_and(|p| p.haze && p.weight > 0.1)
        })
        .count();
    assert!(hazy > 200, "{hazy} hazy cells");
    // Just past the west rim the bank spills: below its rim, thinning.
    let rim_x = (0..96)
        .map(|k| (k * 64) as f32 + 32.0)
        .find(|x| f.covered_at(*x, cz))
        .expect("a west rim on the centre row");
    let p = f
        .sheet_at(rim_x - 128.0, cz)
        .expect("a spill sheet just past the rim");
    assert!(p.spill && p.weight > 0.0 && p.weight < 1.0, "{p:?}");
    let rim_y = f.height[f.cell_index(rim_x, cz).unwrap()];
    assert!(
        p.y > rim_y,
        "the spill falls (Y-down): {} vs rim {rim_y}",
        p.y
    );
    // Most of the field is open air.
    let open = f.w * f.h - f.covered_count();
    assert!(open * 2 > f.w * f.h, "{open} open of {}", f.w * f.h);
}

/// Rim Elm under the Mist (`town0b`): the street the scene opens on is
/// floor.
#[test]
fn rim_elm_street_is_floor() {
    let Some(index) = open_index() else {
        return;
    };
    let (f, _) = scene_field(&index, "town0b");
    assert!(f.covered_count() > 2000, "{}", f.covered_count());
}
