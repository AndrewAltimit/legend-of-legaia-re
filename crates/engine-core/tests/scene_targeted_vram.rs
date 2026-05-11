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
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};

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

    // Floor: the targeted-upload CLUT pass uses merge-zeros semantics
    // (`Vram::upload_tim_partial_opts(..., merge_clut_zeros: true)`) so
    // multiple scene-pack TIMs that target the same CLUT row but each
    // populate a different subset of the 16-color slots can coexist.
    // For town01 specifically, 7 row-479 TIMs across entries 6..9 split
    // into "full" (slots 0..14) and "partial" (slots 0..7); without
    // merge mode the partials' zero entries clobber the full uploads
    // and the keep ratio drops back to 78.6%. Anything below 90% here
    // indicates a regression in the prim-filter / targeted-upload
    // pipeline or the merge semantics.
    assert!(
        keep_ratio >= 0.90,
        "town01 targeted prim keep ratio dropped to {:.1}% (kept={} of textured={})",
        100.0 * keep_ratio,
        total_kept,
        total_textured
    );
}

#[test]
fn town01_field_mode_skips_battle_tim_chunks() {
    // Field-mode dispatch (matching retail's lazy upload) excludes every
    // scene_tmd_stream PROT entry's type-0x01 TIM upload chunks. For
    // town01 specifically that's the 14 TIMs across slots 3..6 that
    // target CLUT rows y=473/479 (and IMG fb_x=768/832). The drop is
    // material - the field NPC TMDs lose their texture support and most
    // of their textured prims fall through the renderer's filter.
    //
    // Asserting *both* that the prim keep ratio drops AND that the TIM
    // upload count drops below the battle-mode count gives us a
    // characterisation regression test: any future engine-side
    // pre-upload of `battle_data` row 479 slots will lift the keep
    // ratio back up without changing the upload count.
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
        if let Ok(s) = Scene::load(&index, name) {
            shared_scenes.push(s);
        }
    }
    let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();

    let (battle, battle_stats) = SceneResources::build_targeted_with_options(
        &scene,
        &shared_refs,
        BuildOptions {
            kind: SceneLoadKind::Battle,
        },
    )
    .expect("build_targeted town01 battle");
    let (field, field_stats) = SceneResources::build_targeted_with_options(
        &scene,
        &shared_refs,
        BuildOptions {
            kind: SceneLoadKind::Field,
        },
    )
    .expect("build_targeted town01 field");

    // Field mode must drop at least one TIM relative to Battle - the
    // scene_tmd_stream battle chunks live in town01's slot 3..6.
    assert!(
        field.tim_count < battle.tim_count,
        "field mode should upload fewer TIMs than battle mode (field={} battle={})",
        field.tim_count,
        battle.tim_count
    );
    // total_tims (the candidate set the upload pass saw) shrinks by the
    // same amount - confirms the skip happens at TIM collection, not in
    // the per-block arbitration step.
    assert!(
        field_stats.total_tims < battle_stats.total_tims,
        "field-mode TIM candidate count should drop (field={} battle={})",
        field_stats.total_tims,
        battle_stats.total_tims
    );

    let dropped_field = {
        let mut kept = 0usize;
        let mut textured = 0usize;
        for rtmd in &field.tmds {
            let (_mesh, stats) = rtmd.build_filtered_vram_mesh_reasoned(&field.vram);
            kept += stats.kept;
            textured += stats.kept
                + stats.missing_clut
                + stats.clut_depth_mismatch
                + stats.missing_texture_page;
        }
        if textured > 0 {
            kept as f32 / textured as f32
        } else {
            1.0
        }
    };
    eprintln!(
        "town01 field mode: TIMs={}, prim keep ratio={:.1}%; battle mode: TIMs={}",
        field.tim_count,
        100.0 * dropped_field,
        battle.tim_count,
    );

    // Field-mode keep ratio is allowed to be anywhere up to but not
    // including the battle floor - we assert it's strictly less than
    // 0.90 (the battle floor) so a future fix that pre-loads
    // `battle_data` slots and lifts field-mode rendering will be a
    // *deliberate* update to this test, not a silent regression.
    assert!(
        dropped_field < 0.90,
        "field-mode keep ratio {:.1}% unexpectedly matches battle-mode floor; \
         either battle_data pre-load is now wired (update this test) or the \
         scene_tmd_stream filter regressed (investigate)",
        100.0 * dropped_field,
    );
}
