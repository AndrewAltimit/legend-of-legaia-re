//! Disc-gated regression test: a world-map scene built with
//! [`SceneLoadKind::WorldMap`] must surface the kingdom slot-4 vertex pool
//! on [`SceneResources::world_map_slot4`], and a field/town scene must NOT
//! (the pool is the per-kingdom object-mesh library, present only on the
//! overworld). This is the wiring the live world-map renderer consumes to
//! draw the slot-4 inspection wireframe.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::scene::{Scene, SceneHost};
use legaia_engine_core::scene_resources::{BuildOptions, SceneLoadKind, SceneResources};

fn extracted_dir() -> Option<PathBuf> {
    let d = PathBuf::from("extracted");
    if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
        return Some(d);
    }
    let alt = PathBuf::from("../../extracted");
    if alt.join("PROT.DAT").exists() && alt.join("CDNAME.TXT").exists() {
        Some(alt)
    } else {
        None
    }
}

fn build(kind: SceneLoadKind, scene: &Scene) -> SceneResources {
    SceneResources::build_targeted_with_options(
        scene,
        &[],
        BuildOptions {
            kind,
            upload_all_tims: true,
            ..Default::default()
        },
    )
    .expect("build scene resources")
    .0
}

#[test]
fn world_map_scene_surfaces_slot4_pool() {
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

    // Every kingdom's overworld scene carries a parseable slot-4 pool.
    for map in ["map01", "map02", "map03"] {
        let scene = Scene::load(&index, map).unwrap_or_else(|e| panic!("load {map}: {e}"));
        let res = build(SceneLoadKind::WorldMap, &scene);
        let slot4 = res
            .world_map_slot4
            .as_ref()
            .unwrap_or_else(|| panic!("{map}: world_map_slot4 should be Some"));
        assert!(
            slot4.bodies.len() >= 15,
            "{map}: expected >=15 slot-4 bodies, got {}",
            slot4.bodies.len()
        );

        // The decoded pool must yield a non-empty 3D inspection wireframe
        // with real elevation (the live renderer's geometry source).
        let opts = legaia_asset::world_map_overlay::WireframeOptions::default();
        let segs = legaia_asset::world_map_overlay::wireframe_segments_3d(slot4, &opts);
        assert!(
            segs.len() >= 1000,
            "{map}: only {} slot-4 wireframe segments",
            segs.len()
        );
        assert!(
            segs.iter().any(|s| s.a[1] != 0 || s.b[1] != 0),
            "{map}: slot-4 wireframe has no elevation"
        );

        // The SAME scene built as a field scene must not surface the pool:
        // it is overworld-only, gated on `SceneLoadKind::WorldMap`.
        let field = build(SceneLoadKind::Field, &scene);
        assert!(
            field.world_map_slot4.is_none(),
            "{map}: slot-4 pool leaked into a field-mode build"
        );
    }
}

#[test]
fn town_scene_has_no_slot4_pool() {
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

    // Even when (incorrectly) built as a world-map scene, a non-kingdom
    // entry has no slot-4 vertex pool, so the resolver returns None rather
    // than fabricating one.
    let res = build(SceneLoadKind::WorldMap, &scene);
    assert!(
        res.world_map_slot4.is_none(),
        "town01 should not yield a kingdom slot-4 pool"
    );
}
