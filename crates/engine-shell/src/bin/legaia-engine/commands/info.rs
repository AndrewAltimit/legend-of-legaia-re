//! Scene inspection subcommands (`scenarios`, `info`, `list-scenes`).
//!
//! Mechanical split from `commands.rs` (behavior-preserving).

use super::*;

pub(crate) fn cmd_scenarios(
    manifest_override: Option<&Path>,
    extracted_root: &Path,
    bless: bool,
) -> Result<()> {
    use legaia_engine_shell::scenarios::{
        ScenariosManifest, bless as bless_manifest, default_manifest_path, run_all,
    };

    let manifest_path = manifest_override
        .map(PathBuf::from)
        .unwrap_or_else(default_manifest_path);
    let manifest = ScenariosManifest::from_toml_path(&manifest_path)?;
    println!(
        "engine scenarios: manifest={} ({} scenarios)  extracted_root={}",
        manifest_path.display(),
        manifest.scenarios.len(),
        extracted_root.display()
    );
    let results = run_all(&manifest, extracted_root)?;

    let mut passed = 0;
    let mut failed = 0;
    let mut unblessed = 0;
    for r in &results {
        match (&r.expected_sha256, r.passed()) {
            (None, _) => {
                unblessed += 1;
                println!(
                    "  [unblessed]   {:<32} scene={:<8} frames={:>3}  observed={}",
                    r.name, r.scene, r.frames, r.observed_sha256
                );
            }
            (Some(_), true) => {
                passed += 1;
                println!(
                    "  [ok]          {:<32} scene={:<8} frames={:>3}  hash={}",
                    r.name, r.scene, r.frames, r.observed_sha256
                );
            }
            (Some(exp), false) => {
                failed += 1;
                println!(
                    "  [DRIFT]       {:<32} scene={:<8} frames={:>3}",
                    r.name, r.scene, r.frames
                );
                println!("                expected:  {exp}");
                println!("                observed:  {}", r.observed_sha256);
            }
        }
    }
    println!("summary: {passed} passed, {failed} drifted, {unblessed} unblessed");

    if bless {
        let updated = bless_manifest(&manifest_path, &results)?;
        println!(
            "blessed: {updated} hash row(s) updated in {}",
            manifest_path.display()
        );
    }

    if failed > 0 {
        anyhow::bail!("{failed} scenario(s) drifted from manifest");
    }
    if unblessed > 0 && !bless {
        anyhow::bail!("{unblessed} scenario(s) need blessing - rerun with --bless after review");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_info(
    scene_name: &str,
    extracted_root: &std::path::Path,
    disc: Option<&std::path::Path>,
    vram_png: Option<&Path>,
    vram_bin: Option<&Path>,
    runtime_vram: Option<&Path>,
    vram_diff_png: Option<&Path>,
    tmd_stats: bool,
    targeted: bool,
) -> Result<()> {
    let index = open_index_from_args(extracted_root, disc)?;
    let scene =
        Scene::load(&index, scene_name).with_context(|| format!("load scene '{scene_name}'"))?;
    let assets = SceneAssets::build(&scene);

    // Load the field-shared blocks (`init_data`, `player_data`) when we
    // can, so the engine VRAM mirrors the retail boot-then-scene layout.
    // Missing blocks (e.g. when running against a region whose CDNAME
    // doesn't carry one of the names) skip with a warning rather than
    // failing - the comparison still works against the rest.
    let shared_scenes = crate::shared::load_shared_scenes(&index, |name, e| {
        eprintln!("warning: shared block '{name}' not loaded: {e}");
    });
    let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();
    let (resources, targeted_stats) = if targeted {
        let (r, s) = SceneResources::build_targeted(&scene, &shared_refs)?;
        (r, Some(s))
    } else {
        (
            SceneResources::build_with_shared(&scene, &shared_refs)?,
            None,
        )
    };

    println!("scene '{}'", scene.name);
    println!(
        "  CDNAME range:           PROT [{}..{})",
        scene.start, scene.end
    );
    println!("  entries swept:          {}", scene.entries.len());
    println!(
        "  shared blocks loaded:   {:?}",
        shared_scenes
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
    );
    println!(
        "  TIMs uploaded to VRAM:  {} (scene-local: {}, shared: {}, parse failures: {})",
        resources.tim_count,
        resources.tim_count - resources.shared_tim_count,
        resources.shared_tim_count,
        resources.tim_parse_failures
    );
    println!(
        "  TMDs parsed:            {} (scene-local: {}, shared: {})",
        resources.tmds.len(),
        resources.tmds.len() - resources.shared_tmd_count,
        resources.shared_tmd_count
    );
    println!(
        "  MES container:          {}",
        if assets.mes.is_some() {
            "present"
        } else {
            "absent"
        }
    );
    println!(
        "  SEQ entries (raw):      {} (in stream wrappers: {})",
        assets.seq_entries.len(),
        assets.seq_in_stream_entries.len()
    );
    println!("  VAB entries:            {}", assets.vab_entries.len());
    println!("  Event-script records:   {}", assets.event_records.len());
    if let Some(s) = &targeted_stats {
        println!(
            "  targeted VRAM upload:   total_tims={} uploaded={} both={} image_only={} clut_only={}",
            s.total_tims,
            s.uploaded_tims,
            s.uploaded_both,
            s.uploaded_image_only,
            s.uploaded_clut_only
        );
    }

    if tmd_stats {
        println!("  per-TMD filter stats (drop reasons):");
        let mut total_kept = 0usize;
        let mut total_miss_clut = 0usize;
        let mut total_depth_mismatch = 0usize;
        let mut total_miss_page = 0usize;
        let mut total_skipped = 0usize;
        for (i, rtmd) in resources.tmds.iter().enumerate() {
            let (_mesh, stats) = rtmd.build_filtered_vram_mesh_reasoned(&resources.vram);
            total_kept += stats.kept;
            total_miss_clut += stats.missing_clut;
            total_depth_mismatch += stats.clut_depth_mismatch;
            total_miss_page += stats.missing_texture_page;
            total_skipped += stats.skipped_bad_vert_index + stats.skipped_untextured;
            println!(
                "    tmd[{i:2}] entry={:4} off=0x{:06X}  kept={:4} miss_clut={:3} depth_mm={:3} miss_page={:4} no_uv={:3}  keep={:5.1}%",
                rtmd.entry_idx,
                rtmd.offset,
                stats.kept,
                stats.missing_clut,
                stats.clut_depth_mismatch,
                stats.missing_texture_page,
                stats.skipped_untextured,
                100.0 * stats.keep_ratio()
            );
        }
        let textured = total_kept + total_miss_clut + total_depth_mismatch + total_miss_page;
        let aggregate_keep = if textured > 0 {
            100.0 * total_kept as f32 / textured as f32
        } else {
            100.0
        };
        println!(
            "  aggregate filter:        kept={} miss_clut={} depth_mm={} miss_page={} skipped={} (textured kept={:.1}%)",
            total_kept,
            total_miss_clut,
            total_depth_mismatch,
            total_miss_page,
            total_skipped,
            aggregate_keep
        );
    }

    if vram_png.is_some() || vram_bin.is_some() || runtime_vram.is_some() {
        let engine_bytes = vram_to_le_bytes(&resources.vram);
        if let Some(p) = vram_png {
            write_vram_png(p, &engine_bytes)?;
            println!("[ok] wrote engine VRAM PNG to {}", p.display());
        }
        if let Some(p) = vram_bin {
            std::fs::write(p, &engine_bytes)
                .with_context(|| format!("writing engine VRAM bin to {}", p.display()))?;
            println!(
                "[ok] wrote engine VRAM bin to {} ({} bytes)",
                p.display(),
                engine_bytes.len()
            );
        }
        if let Some(p) = runtime_vram {
            let runtime_bytes = std::fs::read(p)
                .with_context(|| format!("reading runtime VRAM blob from {}", p.display()))?;
            if runtime_bytes.len() != engine_bytes.len() {
                anyhow::bail!(
                    "runtime VRAM size {} != expected {} (1 MiB BGR555)",
                    runtime_bytes.len(),
                    engine_bytes.len()
                );
            }
            let report = vram_coverage_report(&engine_bytes, &runtime_bytes);
            print_vram_coverage(&report);
            if let Some(diff_path) = vram_diff_png {
                write_vram_diff_png(diff_path, &engine_bytes, &runtime_bytes)?;
                println!("[ok] wrote VRAM diff PNG to {}", diff_path.display());
            }
        } else if vram_diff_png.is_some() {
            eprintln!("warning: --vram-diff-png requires --runtime-vram; skipping diff");
        }
    }
    Ok(())
}

pub(crate) fn cmd_list_scenes(
    extracted_root: &std::path::Path,
    disc: Option<&std::path::Path>,
) -> Result<()> {
    let map: cdname::IndexMap = if let Some(disc_path) = disc {
        // Pull CDNAME.TXT bytes out of the disc image once.
        use legaia_engine_core::{DiscVfs, Vfs};
        let vfs = DiscVfs::open(disc_path)?;
        let bytes = vfs
            .read("cdname.txt")
            .or_else(|_| vfs.read("data/cdname.txt"))
            .context("CDNAME.TXT not present in disc image")?;
        let text = String::from_utf8(bytes).context("CDNAME.TXT is not valid UTF-8")?;
        cdname::parse_str(&text)?
    } else {
        let cdname_path = extracted_root.join("CDNAME.TXT");
        if !cdname_path.exists() {
            anyhow::bail!(
                "missing {} (run `legaia-extract` first or pass --disc PATH)",
                cdname_path.display()
            );
        }
        cdname::parse(&cdname_path).with_context(|| format!("parse {}", cdname_path.display()))?
    };

    let mut names: Vec<String> = map.values().cloned().collect();
    names.sort();
    names.dedup();

    println!("{} distinct scene names:", names.len());
    for name in &names {
        // Report windows in the retail EXTRACTION frame (what Scene::load
        // reads and what extracted/PROT/NNNN_*.BIN filenames use), not the
        // raw #define frame, so the listing matches the loaders.
        if let Some((start, end)) = cdname::block_range_for_name_extraction(&map, name) {
            println!(
                "  {:<24} PROT [{}..{}) ({} entries)",
                name,
                start,
                end,
                end - start
            );
        }
    }
    Ok(())
}
