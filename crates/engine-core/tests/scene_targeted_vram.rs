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
    BATTLE_BOOT_BLOCKS, BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
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
fn town01_field_mode_skips_battle_only_scene_tmd_stream() {
    // Field-mode dispatch (matching retail's lazy upload) excludes
    // every scene_tmd_stream PROT entry's contributions: the leading
    // TMD (`FUN_8001FE70` writes it to the battle character TMD
    // register `_DAT_8007B864`, never drawn from a field scene) and
    // its type-0x01 TIM upload chunks (CLUTs / textures the same
    // mesh samples). For town01 the leading-TMD set is exactly the
    // 7 battle character meshes in entries 6..9 - in field mode
    // none of them should land in the TMD pool, and the matching
    // 14 type-0x01 TIM chunks targeting CLUT rows 473/479 must
    // also be filtered out at TIM collection.
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
            ..Default::default()
        },
    )
    .expect("build_targeted town01 battle");
    let (field, field_stats) = SceneResources::build_targeted_with_options(
        &scene,
        &shared_refs,
        BuildOptions {
            kind: SceneLoadKind::Field,
            ..Default::default()
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

    // Field mode also drops the battle-only TMDs from the parsed
    // pool. Town01's only TMDs (per the current parser coverage)
    // are scene_tmd_stream battle character meshes, so field mode
    // produces an empty TMD pool - any future field-NPC parser
    // additions will lift this strictly above zero, and that's a
    // deliberate update.
    assert!(
        field.tmds.len() < battle.tmds.len(),
        "field-mode TMD pool should be smaller than battle-mode (field={} battle={})",
        field.tmds.len(),
        battle.tmds.len()
    );

    // Every TMD that DID survive into field mode must render cleanly.
    // The whole point of the field-mode skip is "don't drag down
    // the keep ratio with battle meshes whose textures aren't
    // resident"; if any field-mode TMD has textured prims that fail
    // the prim filter, the skip is missing something.
    let mut field_textured = 0usize;
    let mut field_kept = 0usize;
    for rtmd in &field.tmds {
        let (_mesh, stats) = rtmd.build_filtered_vram_mesh_reasoned(&field.vram);
        field_kept += stats.kept;
        field_textured += stats.kept
            + stats.missing_clut
            + stats.clut_depth_mismatch
            + stats.missing_texture_page;
    }
    let field_ratio = if field_textured > 0 {
        field_kept as f32 / field_textured as f32
    } else {
        1.0
    };
    eprintln!(
        "town01 field mode: TMDs={} TIMs={} kept={}/{} ({:.1}%); battle: TMDs={} TIMs={}",
        field.tmds.len(),
        field.tim_count,
        field_kept,
        field_textured,
        100.0 * field_ratio,
        battle.tmds.len(),
        battle.tim_count,
    );
    assert!(
        field_ratio >= 0.90,
        "field-mode keep ratio {:.1}% (kept={}/{}); field-mode TMD pool \
         should not contain battle meshes that can't texture",
        100.0 * field_ratio,
        field_kept,
        field_textured,
    );
}

#[test]
fn battle_boot_vram_parses_real_battle_data() {
    // Sanity-check `SceneResources::build_battle_boot_vram` against
    // the real `battle_data` CDNAME block (PROT 865..869). The
    // builder walks every record's LZS stream and uploads any
    // standard-PSX-TIM textures it finds + invokes the descriptor
    // CLUT pass. The CLUT pass is a no-op until
    // `battle_data_pack::clut_uploads` is wired - this test
    // characterizes the current state and will flip from zero to
    // positive when descriptor decoding lands.
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let host = SceneHost::open_extracted(&extracted).expect("open");
    let index = host.index.clone();
    let mut scenes: Vec<Scene> = Vec::new();
    for name in BATTLE_BOOT_BLOCKS {
        match Scene::load(&index, name) {
            Ok(s) => scenes.push(s),
            Err(e) => eprintln!("warning: boot block '{name}' missing: {e}"),
        }
    }
    if scenes.is_empty() {
        eprintln!("[skip] no battle_data blocks resolved");
        return;
    }
    let refs: Vec<&Scene> = scenes.iter().collect();
    let (_vram, stats) = SceneResources::build_battle_boot_vram(&refs);
    eprintln!(
        "battle_boot: packs={} records={} tims={} cluts={}",
        stats.packs, stats.records, stats.tims_uploaded, stats.cluts_uploaded,
    );
    // At least one pack must detect (PROT 0865 carries the canonical
    // pack shape). Records can vary; assert >0 since the parser
    // emits at least the populated record slice.
    assert!(
        stats.packs >= 1,
        "battle_data should detect at least one pack (got {})",
        stats.packs,
    );
    assert!(
        stats.records >= 1,
        "battle_data should yield at least one decoded record (got {})",
        stats.records,
    );
}
