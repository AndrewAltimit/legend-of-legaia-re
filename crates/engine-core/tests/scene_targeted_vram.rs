//! Disc-gated regression test: building [`SceneResources::build_targeted`]
//! for `town01` (Rim Elm) plus the standard field-shared CDNAME blocks
//! (`init_data` + `player_data`) must keep the vast majority of the
//! scene's textured TMD primitives renderable.
//!
//! This catches future regressions in the VRAM-pre-pass / prim-filter
//! pipeline: a change that re-introduces 4bpp-vs-256-wide CLUT-row
//! collisions (the bug that previously dropped 80%+ of town01's prims)
//! would push this number down below the floor.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::scene::{Scene, SceneHost};
use legaia_engine_core::scene_resources::{FIELD_SHARED_BLOCKS, SceneResources};

fn extracted_dir() -> Option<PathBuf> {
    let d = PathBuf::from("extracted");
    if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
        Some(d)
    } else {
        let alt = PathBuf::from("../../extracted");
        if alt.join("PROT.DAT").exists() && alt.join("CDNAME.TXT").exists() {
            Some(alt)
        } else {
            None
        }
    }
}

#[test]
fn town01_targeted_upload_keeps_majority_of_textured_prims() {
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

    let mut shared_scenes: Vec<Scene> = Vec::new();
    for name in FIELD_SHARED_BLOCKS {
        match Scene::load(&index, name) {
            Ok(s) => shared_scenes.push(s),
            Err(e) => eprintln!("warning: shared block '{name}' missing: {e}"),
        }
    }
    let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();

    let (res, upload_stats) =
        SceneResources::build_targeted(&scene, &shared_refs).expect("build_targeted town01");

    assert!(
        !res.tmds.is_empty(),
        "town01 should have at least one parsed TMD"
    );
    assert!(
        upload_stats.uploaded_tims > 0,
        "targeted upload should have contributed at least one TIM block (had {} candidates)",
        upload_stats.total_tims
    );

    // Walk every TMD's prim filter and aggregate keep / drop counts.
    let mut total_kept = 0usize;
    let mut total_textured = 0usize;
    for rtmd in &res.tmds {
        let (_mesh, stats) = rtmd.build_filtered_vram_mesh_reasoned(&res.vram);
        total_kept += stats.kept;
        total_textured += stats.kept
            + stats.missing_clut
            + stats.clut_depth_mismatch
            + stats.missing_texture_page;
    }
    let keep_ratio = if total_textured > 0 {
        total_kept as f32 / total_textured as f32
    } else {
        1.0
    };

    eprintln!(
        "town01 targeted: kept={} textured={} ratio={:.1}%",
        total_kept,
        total_textured,
        100.0 * keep_ratio
    );

    // Floor: the targeted-upload + relaxed-depth-threshold combination
    // currently keeps ~78% of town01 textured prims. Anything below
    // 60% indicates a regression in the prim-filter / targeted-upload
    // pipeline.
    assert!(
        keep_ratio >= 0.60,
        "town01 targeted prim keep ratio dropped to {:.1}% (kept={} of textured={})",
        100.0 * keep_ratio,
        total_kept,
        total_textured
    );
}
