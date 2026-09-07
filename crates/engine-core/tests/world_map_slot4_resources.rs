//! Disc-gated regression test: a world-map scene built with
//! [`SceneLoadKind::WorldMap`] must surface the kingdom's slot-4 **animation
//! bank** on [`SceneResources::world_map_slot4`], and a field/town scene must
//! NOT (it is overworld-only). This is the wiring the live world-map
//! renderer's `LEGAIA_WORLDMAP_SLOT4` inspection overlay consumes.
//!
//! Slot 4 is an asset-type-`0x05` ANM container of world-map actor clips, not
//! a "vertex pool" or an object-mesh library - the two readings this file's
//! comments used to carry. Each 8-byte entry is a rigid transform (three
//! packed 12-bit translations plus three 8-bit angles), so the only geometry
//! in the bytes is each animated part's translation path across its clip,
//! which is what the assertions below measure. See
//! `docs/formats/world-map-overlay.md`.
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

    // Every kingdom's overworld scene carries a parseable slot-4 clip bank.
    for map in ["map01", "map02", "map03"] {
        let scene = Scene::load(&index, map).unwrap_or_else(|e| panic!("load {map}: {e}"));
        let res = build(SceneLoadKind::WorldMap, &scene);
        let slot4 = res
            .world_map_slot4
            .as_ref()
            .unwrap_or_else(|| panic!("{map}: world_map_slot4 should be Some"));
        assert!(
            slot4.bodies.len() >= 15,
            "{map}: expected >=15 slot-4 clips, got {}",
            slot4.bodies.len()
        );

        // Every clip has to decode as an ANM record: a part count, a frame
        // count, and the 0x080C record marker the parser magic-checks.
        for b in &slot4.bodies {
            assert!(
                b.part_count() > 0 && b.frame_count() > 0,
                "{map}: clip {} has {} parts x {} frames",
                b.index,
                b.part_count(),
                b.frame_count()
            );
        }

        // The decoded bank must yield a non-empty translation-path plot with
        // motion on more than one axis - the live overlay's geometry source.
        let segs = legaia_asset::world_map_overlay::translation_path_segments(slot4);
        assert!(
            !segs.is_empty(),
            "{map}: no slot-4 translation-path segments"
        );
        assert!(
            segs.iter().any(|s| s.a[1] != s.b[1]),
            "{map}: no clip translates on Y"
        );
        assert!(
            segs.iter().any(|s| s.a[0] != s.b[0] || s.a[2] != s.b[2]),
            "{map}: no clip translates on X or Z"
        );

        // The SAME scene built as a field scene must not surface the bank:
        // it is overworld-only, gated on `SceneLoadKind::WorldMap`.
        let field = build(SceneLoadKind::Field, &scene);
        assert!(
            field.world_map_slot4.is_none(),
            "{map}: slot-4 clip bank leaked into a field-mode build"
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
    // entry has no slot-4 clip bank, so the resolver returns None rather
    // than fabricating one.
    let res = build(SceneLoadKind::WorldMap, &scene);
    assert!(
        res.world_map_slot4.is_none(),
        "town01 should not yield a kingdom slot-4 clip bank"
    );
}
