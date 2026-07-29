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

/// Which objects of a stage TMD the backdrop actually draws.
///
/// Retail's registration drops object index 1 and keeps the rest
/// (`FUN_800513f0`, ported as `battle_backdrop::backdrop_object_indices`), so
/// the two stage shapes on the disc resolve differently: the two-object shells
/// keep object 0 alone, and the four-object overworld domes keep 0, 2 and 3.
/// Drawing object 0 alone - the port's old behaviour - is right for the shells
/// and loses the mountains and the ground ring on the domes.
#[test]
fn backdrop_object_selection_matches_the_two_stage_shapes() {
    use legaia_engine_core::battle_backdrop::backdrop_object_indices;

    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");

    // Rim Elm: 2 objects -> object 0 only. Unchanged from the old truncation,
    // which is why the Tetsu ground truth cannot regress.
    let (rim_objs, _) = dome_shape(&host, 7);
    assert_eq!(rim_objs, 2);
    assert_eq!(backdrop_object_indices(rim_objs), vec![0]);

    // map01 overworld dome: 4 objects -> sky, mountains, ground ring.
    let (map_objs, _) = dome_shape(&host, 88);
    assert_eq!(map_objs, 4);
    assert_eq!(backdrop_object_indices(map_objs), vec![0, 2, 3]);

    // Object 3 of the overworld dome is the flat ground ring at Y = 0 - the
    // piece the truncation was dropping, and the reason an overworld battle
    // had sky but no floor behind the procedural grid.
    let bytes = host.index.entry_bytes(88).expect("read stage entry");
    let s = legaia_asset::scene_tmd_stream::detect(&bytes).expect("scene_tmd_stream");
    let tmd = legaia_tmd::parse(&bytes[s.tmd_range()]).expect("parse dome TMD");
    let ys: Vec<i16> = tmd.objects[3].vertices.iter().map(|v| v.y).collect();
    assert!(
        ys.iter().all(|y| *y == 0),
        "map01 object 3 is the flat Y=0 ground ring"
    );
    // ...and object 2 is the mountain band, which reaches well above it.
    let min_y2 = tmd.objects[2].vertices.iter().map(|v| v.y).min().unwrap();
    assert!(
        min_y2 < -2000,
        "map01 object 2 is the mountain ring, got min y {min_y2}"
    );
}

/// The four-object domes are a small, closed set: only the three overworld
/// maps have them, and every other stage stream on the disc is a two-object
/// shell (or smaller). This is what makes "drop index 1" a one-line rule
/// rather than a per-scene table.
#[test]
fn four_object_stages_are_only_the_overworld_domes() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");

    let mut multi = Vec::new();
    let mut counts = std::collections::BTreeMap::<usize, usize>::new();
    for idx in 0..host.index.entry_count() as u32 {
        let Ok(bytes) = host.index.entry_bytes(idx) else {
            continue;
        };
        let Some(s) = legaia_asset::scene_tmd_stream::detect(&bytes) else {
            continue;
        };
        let Ok(tmd) = legaia_tmd::parse(&bytes[s.tmd_range()]) else {
            continue;
        };
        *counts.entry(tmd.objects.len()).or_default() += 1;
        if tmd.objects.len() > 2 {
            multi.push(idx);
        }
    }
    eprintln!("stage stream object-count histogram: {counts:?}");
    eprintln!("stage streams with >2 objects: {multi:?}");
    assert_eq!(
        multi,
        vec![88, 89, 90, 247, 248, 249, 394],
        "only the map01/map02/map03 overworld domes carry more than two objects"
    );
}
