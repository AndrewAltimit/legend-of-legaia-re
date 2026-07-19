//! Disc-gated: which PROT entry a battle uses as its stage backdrop.
//!
//! A scene bundle is a fixed slot array - `.MAP`, v12 table, event scripts,
//! asset table, texture pack, then one `scene_tmd_stream` per sub-area. The
//! battle backdrop is the stream the retail scene loader leaves in
//! `_DAT_8007B864`, and it is **not** uniformly the block's first stream:
//! `map01` (overworld) uses its first (PROT 88), Rim Elm `town01` uses its
//! second (PROT 7) - the entry the three Tetsu tutorial anchors hold resident.
//!
//! Entry 6, the block's first stream, is a *different* sub-area's backdrop.
//! It byte-matches the Tetsu battle's resident dome only in its over-read tail
//! (past `(next_lba - lba) * 0x800`), which is exactly the phantom hit any
//! "scan the block for the dome" sweep has to reject.
use std::path::PathBuf;

use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for d in ["extracted", "../../extracted"] {
        let p = PathBuf::from(d);
        if p.join("PROT.DAT").exists() && p.join("CDNAME.TXT").exists() {
            return Some(p);
        }
    }
    None
}

/// Objects + total vertices of a stage entry's leading dome TMD.
fn dome_shape(host: &SceneHost, idx: u32) -> (usize, usize) {
    let bytes = host.index.entry_bytes(idx).expect("read stage entry");
    let s = legaia_asset::scene_tmd_stream::detect(&bytes).expect("stage is a scene_tmd_stream");
    let tmd = legaia_tmd::parse(&bytes[s.tmd_range()]).expect("parse dome TMD");
    let verts = tmd.objects.iter().map(|o| o.vertices.len()).sum();
    (tmd.objects.len(), verts)
}

/// Object 0's vertex pool - the byte run a resident dome is identified by.
fn dome_vertex_pool(host: &SceneHost, idx: u32) -> Vec<u8> {
    let bytes = host.index.entry_bytes(idx).expect("read stage entry");
    let s = legaia_asset::scene_tmd_stream::detect(&bytes).expect("stage is a scene_tmd_stream");
    let tmd = legaia_tmd::parse(&bytes[s.tmd_range()]).expect("parse dome TMD");
    tmd.objects[0]
        .vertices
        .iter()
        .flat_map(|v| {
            [v.x, v.y, v.z, 0i16]
                .into_iter()
                .flat_map(|c| c.to_le_bytes())
        })
        .collect()
}

#[test]
fn map01_battle_stage_is_prot_88() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let stages = host.index.battle_stage_entries("map01");
    eprintln!("map01 battle-stage entries: {stages:?}");
    assert!(
        stages.contains(&88),
        "map01 stage should include PROT 88, got {stages:?}"
    );
    // The overworld backdrop is the block's first stage stream.
    assert_eq!(host.index.battle_stage_entry_for_scene("map01"), Some(88));
    assert_eq!(dome_shape(&host, 88), (4, 340), "map01 dome shape");
}

#[test]
fn town01_battle_stage_is_prot_7_not_the_blocks_first_stream() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");

    // Rim Elm's bundle carries four sub-area backdrops at bundle slots 5..=8.
    let stages = host.index.battle_stage_entries("town01");
    eprintln!("town01 battle-stage entries: {stages:?}");
    assert_eq!(
        stages,
        vec![6, 7, 8, 9],
        "town01 bundle slots 5..=8 are its stage streams"
    );

    // The Tetsu battle is fought inside the second, not the first.
    assert_eq!(
        host.index.battle_stage_entry_for_scene("town01"),
        Some(7),
        "Rim Elm's own backdrop is bundle slot 6 = PROT 7"
    );

    // Shape of the dome the retail Tetsu-battle states hold resident: two
    // objects, 311 + 30 vertices.
    assert_eq!(dome_shape(&host, 7), (2, 341), "Rim Elm dome shape");
    // Slots 5 and 8 are separable on shape alone.
    for other in [6u32, 9] {
        assert_ne!(
            dome_shape(&host, other),
            (2, 341),
            "PROT {other} must not be confusable with the Rim Elm dome"
        );
    }
    // Slot 7 (PROT 8) is the same *shape* but different geometry - only the
    // bytes separate them, which is why a shape-only match is not enough to
    // identify a resident dome.
    assert_eq!(dome_shape(&host, 8), (2, 341));
    assert_ne!(
        dome_vertex_pool(&host, 7),
        dome_vertex_pool(&host, 8),
        "PROT 7 and 8 share a vertex count but not their vertices"
    );
}

#[test]
fn battle_stage_overlay_entry_matches_the_plus_0x47_band() {
    use legaia_engine_core::overlay_loader::battle_stage_overlay_entry;
    // Stage id 0 = no stage overlay (the `beq v1, zero` arm of FUN_800520F0).
    assert_eq!(battle_stage_overlay_entry(0), None);
    // The Tetsu tutorial battle: `_DAT_8007B64A = 1`, loader-B tracker 0x48.
    assert_eq!(battle_stage_overlay_entry(1), Some(967));
    // The `*_DAT_8007BD0C == 0xB5` per-formation override.
    assert_eq!(battle_stage_overlay_entry(2), Some(968));
}

/// The picked stage entry must actually surface as a parsed TMD in a
/// `SceneLoadKind::Battle` build - otherwise `build_battle_stage` silently
/// returns `None` and the battle renders with no backdrop at all.
#[test]
fn town01_battle_build_surfaces_the_stage_mesh() {
    use legaia_engine_core::scene::Scene;
    use legaia_engine_core::scene_resources::{
        BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
    };

    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let stage_entry = host
        .index
        .battle_stage_entry_for_scene("town01")
        .expect("town01 has a stage entry");

    let scene = Scene::load(&host.index, "town01").expect("load town01");
    let shared: Vec<Scene> = FIELD_SHARED_BLOCKS
        .iter()
        .filter_map(|n| Scene::load(&host.index, n).ok())
        .collect();
    let refs: Vec<&Scene> = shared.iter().collect();
    let (res, _) = SceneResources::build_targeted_with_options(
        &scene,
        &refs,
        BuildOptions {
            kind: SceneLoadKind::Battle,
            upload_all_tims: true,
            system_ui: None,
        },
    )
    .expect("build town01 in battle mode");

    let dome = res
        .tmds
        .iter()
        .find(|t| t.entry_idx == stage_entry)
        .unwrap_or_else(|| panic!("battle build has no TMD for stage entry {stage_entry}"));
    assert_eq!(
        dome.tmd.objects.len(),
        2,
        "the Rim Elm backdrop is the 2-object dome"
    );
}
