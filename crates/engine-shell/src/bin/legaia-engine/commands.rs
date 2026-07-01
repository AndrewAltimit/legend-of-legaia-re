//! Headless subcommand implementations: scene inspection (`info`,
//! `list-scenes`, `clut-trace`, `man-scripts`), the parity/trace oracles
//! (`vram-oracle`, `mode-trace`, `audio-trace`, `pcm-trace`, `replay`,
//! `scenarios`), the save/load smoke commands, and the synthetic
//! session drivers (`battle`, `inventory`, `equip`, `title`, ...).

use crate::cli::ConfigCmd;
use crate::{AudioTraceArgs, ModeTraceArgs, PcmTraceArgs, VramOracleArgs, decode_str_frame_count};
use anyhow::{Context, Result};
use legaia_engine_core::scene::{ProtIndex, Scene, SceneTickEvent};
use legaia_engine_core::scene_assets::SceneAssets;
use legaia_engine_core::scene_resources::{FIELD_SHARED_BLOCKS, SceneResources};
use legaia_engine_shell::audio_trace_oracle::{
    AudioTraceFrame, audio_trace_to_jsonl, engine_trace_from_paths, first_audio_trace_divergence,
    first_audio_trace_divergence_multi, load_runtime_audio_trace_from_save,
    load_runtime_audio_trace_jsonl,
};
use legaia_engine_shell::mode_trace_oracle::{
    ModeTraceFrame, build_engine_mode_trace, first_mode_trace_divergence,
    load_runtime_mode_trace_from_save, mode_trace_to_jsonl,
};
use legaia_engine_shell::pcm_oracle::{
    EnginePcmTrace, PcmStats, build_engine_pcm_trace, first_pcm_divergence, pcm_stats,
    retail_reference_pcm, write_wav,
};
use legaia_engine_shell::replay::ReplayFile;
use legaia_engine_shell::vram_oracle::{
    TexpageDivergence, build_engine_vram_bytes_with_frames, first_texpage_divergence,
    load_runtime_vram_from_save, vram_to_le_bytes,
};
use legaia_engine_shell::{BootConfig, BootSession};
use legaia_prot::cdname;
use std::path::{Path, PathBuf};
use std::time::Duration;

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
    let mut shared_scenes: Vec<Scene> = Vec::new();
    for name in FIELD_SHARED_BLOCKS {
        match Scene::load(&index, name) {
            Ok(s) => shared_scenes.push(s),
            Err(e) => eprintln!("warning: shared block '{name}' not loaded: {e}"),
        }
    }
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

/// Walk a scene's TMD pool, locate every primitive that drops as
/// `MissingClut`, and report which PROT entries on the disc carry a TIM
/// whose CLUT block lands at the missing row. Optional runtime VRAM
/// cross-check distinguishes "row absent from the engine but present at
/// runtime" (engine loader gap) from "row absent from runtime too"
/// (mesh references unreachable CLUT - likely a parser issue).
pub(crate) fn cmd_clut_trace(
    scene_name: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    runtime_vram: Option<&Path>,
    max_sources: usize,
) -> Result<()> {
    use legaia_asset::tim_scan;
    use legaia_tim::vram::PrimTextureStatus;
    use std::collections::BTreeMap;

    let index = open_index_from_args(extracted_root, disc)?;
    let scene =
        Scene::load(&index, scene_name).with_context(|| format!("load scene '{scene_name}'"))?;

    let mut shared_scenes: Vec<Scene> = Vec::new();
    for name in FIELD_SHARED_BLOCKS {
        if let Ok(s) = Scene::load(&index, name) {
            shared_scenes.push(s);
        }
    }
    let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();
    let (resources, _upload_stats) = SceneResources::build_targeted(&scene, &shared_refs)?;

    println!("scene '{}'", scene.name);
    println!(
        "  shared blocks loaded: {:?}",
        shared_scenes
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
    );
    println!(
        "  TMDs: {}  TIMs uploaded: {}",
        resources.tmds.len(),
        resources.tim_count
    );

    // Group dropped prims by (cba, depth). Multiple prims in multiple TMDs
    // often share the same CLUT row; we only need to find the supplier
    // once per unique row.
    let mut dropped: BTreeMap<(u16, u8), DroppedClut> = BTreeMap::new();
    for rtmd in &resources.tmds {
        for obj in &rtmd.tmd.objects {
            let groups = legaia_tmd::legaia_prims::iter_groups_lenient(
                &rtmd.raw,
                obj.primitives_byte_offset,
                obj.primitives_byte_size,
            );
            for g in &groups {
                for prim in &g.prims {
                    if prim.uvs.is_empty() {
                        continue;
                    }
                    let depth = match (prim.tsb >> 7) & 0x3 {
                        0 => 4u8,
                        1 => 8,
                        _ => continue, // 16bpp / direct: no CLUT to be missing
                    };
                    let status = resources
                        .vram
                        .prim_texture_status(prim.cba, prim.tsb, &prim.uvs);
                    if let PrimTextureStatus::MissingClut { .. } = status {
                        let entry = dropped.entry((prim.cba, depth)).or_default();
                        entry.prim_count += 1;
                        entry.tmd_locations.insert((rtmd.entry_idx, rtmd.offset));
                    }
                }
            }
        }
    }

    if dropped.is_empty() {
        println!("  no MissingClut drops detected in this scene");
        return Ok(());
    }

    let runtime_bytes = match runtime_vram {
        Some(p) => Some(
            std::fs::read(p).with_context(|| format!("read runtime VRAM blob {}", p.display()))?,
        ),
        None => None,
    };
    if let Some(b) = &runtime_bytes
        && b.len() != 1024 * 512 * 2
    {
        anyhow::bail!("runtime VRAM size {} != 1 MiB (1024*512*2)", b.len());
    }

    // Pre-scan every PROT entry once: collect (entry_idx, cba_fb_x,
    // cba_fb_y, depth). One pass through the disc; subsequent lookups
    // are cheap.
    println!("  scanning PROT corpus for CLUT suppliers ...");
    let mut suppliers: Vec<TimSupplier> = Vec::new();
    for idx in 0..index.entry_count() as u32 {
        let Ok(bytes) = index.entry_bytes(idx) else {
            continue;
        };
        for hit in tim_scan::scan_buffer(&bytes) {
            let Ok(tim) = legaia_tim::parse(&bytes[hit.offset..hit.offset + hit.byte_len]) else {
                continue;
            };
            let Some(clut) = tim.clut.as_ref() else {
                continue;
            };
            suppliers.push(TimSupplier {
                entry_idx: idx,
                offset: hit.offset,
                fb_x: clut.fb_x,
                fb_y: clut.fb_y,
                width: clut.w,
                bpp: hit.bpp,
            });
        }
    }
    println!(
        "  scanned {} PROT entries, found {} TIMs with CLUT blocks",
        index.entry_count(),
        suppliers.len()
    );

    // For each unique missing (cba, depth) report what we found.
    println!();
    println!(
        "  {} unique missing CLUT row(s) across the scene's TMDs:",
        dropped.len()
    );
    let mut supplier_entries: BTreeMap<u32, BTreeMap<&str, ()>> = BTreeMap::new();
    let mut shared_block_recommend: BTreeMap<&'static str, u32> = BTreeMap::new();
    for ((cba, depth), info) in &dropped {
        let cx = (cba & 0x3F) * 16;
        let cy = (cba >> 6) & 0x1FF;
        let clut_w: usize = match depth {
            4 => 16,
            8 => 256,
            _ => 0,
        };
        let in_runtime = match runtime_bytes.as_ref() {
            Some(b) => row_has_data(b, cx as usize, cy as usize, clut_w),
            None => false,
        };

        // Match by rectangle CONTAINMENT - a TIM CLUT block covers the
        // missing slot if its (fb_x, fb_y, width, 1) rect contains
        // (cx, cy). PSX games commonly pack 16 distinct 4bpp palettes
        // into one 256-wide CLUT block, so the CBA's 16-pixel slot
        // sits inside a wider supplier rect.
        let matching: Vec<&TimSupplier> = suppliers
            .iter()
            .filter(|s| s.fb_y == cy && s.fb_x <= cx && (cx + clut_w as u16) <= (s.fb_x + s.width))
            .collect();

        println!(
            "    cba=0x{:04X} depth={}bpp clut@({:4},{:3}) prims={:4} tmds={:2} runtime_has_row={}",
            cba,
            depth,
            cx,
            cy,
            info.prim_count,
            info.tmd_locations.len(),
            in_runtime
        );
        if matching.is_empty() {
            println!("      ! no PROT entry on disc supplies this row");
        } else {
            for s in matching.iter().take(max_sources) {
                let scene_name = index.scene_for_index(s.entry_idx).unwrap_or("?");
                supplier_entries
                    .entry(s.entry_idx)
                    .or_default()
                    .insert(scene_name, ());
                if let Some(static_name) = known_scene_block_for(scene_name) {
                    *shared_block_recommend.entry(static_name).or_default() += 1;
                }
                println!(
                    "      supplier: PROT {:4} ({}) off=0x{:06X} clut_w={} bpp={}",
                    s.entry_idx, scene_name, s.offset, s.width, s.bpp
                );
            }
            if matching.len() > max_sources {
                println!(
                    "      ... {} more supplier(s) suppressed",
                    matching.len() - max_sources
                );
            }
        }
    }

    println!();
    println!("  PROT entries the engine would need to keep resident:");
    for (entry, scenes) in &supplier_entries {
        let scene_list = scenes.keys().copied().collect::<Vec<_>>().join(", ");
        println!("    PROT {entry:4} (scene blocks: {scene_list})");
    }

    if !shared_block_recommend.is_empty() {
        println!();
        println!("  recommended FIELD_SHARED_BLOCKS additions (by supplier hit count):");
        for (name, hits) in &shared_block_recommend {
            println!("    \"{name}\"   (supplies {hits} missing row(s))");
        }
    }

    Ok(())
}

/// Map a free-form CDNAME scene label to a stable shared-block name
/// the engine knows how to load. Conservative: only return a name if it
/// matches one of the well-known shared blocks we'd actually pin into
/// VRAM, not a per-scene town/field block.
fn known_scene_block_for(scene_name: &str) -> Option<&'static str> {
    match scene_name {
        "init_data" => Some("init_data"),
        "player_data" => Some("player_data"),
        "battle_data" => Some("battle_data"),
        "befect_data" => Some("befect_data"),
        "sound_data" => Some("sound_data"),
        "sound_data2" => Some("sound_data2"),
        "gameover_data" => Some("gameover_data"),
        "card_data" => Some("card_data"),
        _ => None,
    }
}

#[derive(Default)]
struct DroppedClut {
    prim_count: usize,
    tmd_locations: std::collections::BTreeSet<(u32, usize)>,
}

struct TimSupplier {
    entry_idx: u32,
    offset: usize,
    fb_x: u16,
    fb_y: u16,
    width: u16,
    bpp: u32,
}

/// True when any of the next `w` 16-bit words starting at `(x, y)` in
/// the 1 MiB BGR555 LE blob are non-zero. Used by `cmd_clut_trace` to
/// decide whether the runtime captured this CLUT row.
fn row_has_data(blob: &[u8], x: usize, y: usize, w: usize) -> bool {
    const VW: usize = 1024;
    const VH: usize = 512;
    if y >= VH {
        return false;
    }
    let row_start = (y * VW + x) * 2;
    let end = ((x + w).min(VW) * 2) + y * VW * 2;
    let end = end.min(blob.len());
    if row_start >= end {
        return false;
    }
    let mut i = row_start;
    while i + 1 < end {
        if blob[i] != 0 || blob[i + 1] != 0 {
            return true;
        }
        i += 2;
    }
    false
}

/// Resolves the (scene_name, runtime_bytes, source_label) triple from
/// either explicit args or a scenario lookup. Scenario mode reads the
/// VRAM blob in-process via `legaia-mednafen`'s GPU section parser, so
/// no temp file is needed.
///
/// In scenario mode with `frames > 0`, additionally boots a
/// [`BootSession`] on the resolved scene and ticks it `frames` times
/// before returning the sampled engine VRAM. This catches dynamic
/// uploads (NPC palette swaps, fog ramps, per-frame CLUT mutations)
/// that the pure pre-pass doesn't see.
fn resolve_vram_inputs(args: &VramOracleArgs<'_>) -> Result<ResolvedVram> {
    use legaia_mednafen::ScenarioManifest;

    match (args.scenario, args.scene, args.runtime_vram) {
        (Some(label), _, _) => {
            let manifest = ScenarioManifest::from_path(args.manifest)?;
            let scn = manifest.by_label(label).with_context(|| {
                format!("scenario {label:?} not in {}", args.manifest.display())
            })?;
            let scene_name = scn.expected_active_scene.clone().with_context(|| {
                format!("scenario {label:?} has no `expected_active_scene`; cannot derive scene",)
            })?;
            let save_path = manifest.save_path(scn.slot)?;
            if !save_path.exists() {
                anyhow::bail!(
                    "scenario {label:?} slot {} save not found at {}",
                    scn.slot,
                    save_path.display()
                );
            }
            let runtime_bytes = load_runtime_vram_from_save(&save_path)?;
            let source_label = format!(
                "scenario {label:?} (slot {}, {})",
                scn.slot,
                save_path.display()
            );
            Ok(ResolvedVram {
                scene_name,
                runtime_bytes,
                source_label,
            })
        }
        (None, Some(scene_name), Some(runtime_path)) => {
            let runtime_bytes = std::fs::read(runtime_path)
                .with_context(|| format!("read runtime VRAM blob {}", runtime_path.display()))?;
            Ok(ResolvedVram {
                scene_name: scene_name.to_owned(),
                runtime_bytes,
                source_label: runtime_path.display().to_string(),
            })
        }
        _ => anyhow::bail!(
            "vram-oracle: provide either `--scenario <label>` or both `--scene` + `--runtime-vram`"
        ),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_man_scripts(
    scene_name: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    all: bool,
    disasm_record: Option<usize>,
    disasm_partition: usize,
    dump_man: Option<&Path>,
    gflag_partition: Option<usize>,
    narration: bool,
) -> Result<()> {
    use legaia_engine_core::man_field_scripts::{
        partition_record_span, walk_partition_gflag_sites, walk_partition1_scripts,
    };
    use legaia_engine_core::scene_bundle;

    let index = open_index_from_args(extracted_root, disc)?;
    let scene =
        Scene::load(&index, scene_name).with_context(|| format!("load scene '{scene_name}'"))?;
    let bundle = scene_bundle::find_bundle(&scene).with_context(|| {
        format!("scene '{scene_name}' has no scene_asset_table bundle (no MAN)")
    })?;
    let entry_bytes = index
        .entry_bytes_extended(bundle.entry_idx())
        .with_context(|| format!("entry bytes for PROT[{}]", bundle.entry_idx()))?;
    let man = scene_bundle::extract_man_payload(&bundle, &entry_bytes)?
        .with_context(|| format!("scene '{scene_name}' MAN payload did not decode"))?;
    let man_file = legaia_asset::man_section::parse(&man)?;

    if let Some(path) = dump_man {
        std::fs::write(path, &man)
            .with_context(|| format!("write decoded MAN to {}", path.display()))?;
        println!(
            "wrote decoded MAN payload ({} bytes) to {}",
            man.len(),
            path.display()
        );
    }

    let records = walk_partition1_scripts(&man_file, &man);
    println!(
        "scene '{}' (PROT[{}]): {} partition-1 records, counts {:?}",
        scene.name,
        bundle.entry_idx(),
        records.len(),
        man_file.header.partition_counts,
    );

    let mut total_yields = 0usize;
    let mut total_records = 0usize;
    let mut tetsu = 0usize;
    for rec in &records {
        total_yields += rec.arm_sites.len();
        let candidates: Vec<_> = rec.encounter_arm_candidates().collect();
        total_records += candidates.len();
        let show = all || !candidates.is_empty() || rec.index == 0;
        if show {
            println!(
                "  P1[{:3}] start=0x{:05X} pc0={:3} body={:5}b insns={:4} errs={:3} yields={} candidates={}",
                rec.index,
                rec.script_start,
                rec.pc0,
                rec.body_len,
                rec.insn_count,
                rec.decode_errors,
                rec.arm_sites.len(),
                candidates.len(),
            );
        }
        for site in &rec.arm_sites {
            let Some(record) = site.record else { continue };
            if site.matches_tetsu() {
                tetsu += 1;
            }
            if show {
                println!(
                    "      yield 0x{:02X}{} @ 0x{:05X}  window={:02X?}  -> count={} ids={:02X?}{}",
                    site.opcode,
                    if site.wide { "(wide)" } else { "" },
                    site.abs_pc,
                    site.window,
                    record.count,
                    &record.monster_ids[..record.count as usize],
                    if site.matches_tetsu() {
                        "  <<< Tetsu (count=1 id=0x4F)"
                    } else {
                        ""
                    },
                );
            }
        }
    }
    println!(
        "summary: {} yield sites, {} decode as inline records, {} match the Tetsu signature",
        total_yields, total_records, tetsu,
    );

    if let Some(target) = disasm_record {
        use legaia_engine_vm::field_disasm::{LinearWalker, format_instruction};
        let (script_start, pc0, body_len) =
            partition_record_span(&man_file, &man, disasm_partition, target).with_context(
                || format!("partition {disasm_partition} record {target} has no decodable span"),
            )?;
        let end = script_start + body_len;
        let body = man
            .get(script_start..end)
            .with_context(|| format!("record {target} body slice out of range"))?;
        println!(
            "\n--- disasm P{disasm_partition}[{target}] (start=0x{script_start:05X} pc0={pc0} body={body_len}b) ---",
        );
        for insn in LinearWalker::new(body, pc0) {
            match insn {
                Ok(insn) => println!(
                    "  0x{:05X} (+0x{:04X})  {}",
                    script_start + insn.pc,
                    insn.pc,
                    format_instruction(&insn, body),
                ),
                Err((pc, e)) => {
                    let raw = body.get(pc).copied().unwrap_or(0);
                    println!(
                        "  0x{:05X} (+0x{:04X})  .byte 0x{raw:02X}  [{e:?}]",
                        script_start + pc,
                        pc,
                    );
                }
            }
        }
    }

    if let Some(partition) = gflag_partition {
        let sites = walk_partition_gflag_sites(&man_file, &man, partition);
        println!(
            "\n--- GFLAG writes in partition {partition} ({} sites) ---",
            sites.len(),
        );
        for s in &sites {
            println!(
                "  P{}[{}] GFLAG.{} bit={:<2} @ 0x{:05X} (op 0x{:02X})",
                s.partition,
                s.record,
                if s.set { "Set  " } else { "Clear" },
                s.bit,
                s.abs_pc,
                s.opcode,
            );
        }
    }

    if narration {
        use legaia_asset::cutscene_text::parse_narration;
        // Either a specific `--disasm-record` in `--disasm-partition`, or a
        // sweep of every record in `disasm_partition` (defaulting to 2, the
        // cutscene-timeline partition).
        let candidates: Vec<usize> = match disasm_record {
            Some(r) => vec![r],
            None => {
                let count = man_file
                    .header
                    .partition_counts
                    .get(disasm_partition)
                    .copied()
                    .unwrap_or(0)
                    .max(0) as usize;
                (0..count).collect()
            }
        };
        println!("\n--- inline cutscene narration (partition {disasm_partition}) ---",);
        let mut total = 0usize;
        for r in candidates {
            let Some((script_start, _pc0, body_len)) =
                partition_record_span(&man_file, &man, disasm_partition, r)
            else {
                continue;
            };
            let body = &man[script_start..script_start + body_len];
            let blocks = parse_narration(body);
            for (bi, block) in blocks.iter().enumerate() {
                total += block.pages.len();
                println!(
                    "  P{disasm_partition}[{r}] block {bi} @ 0x{:05X}: declared {} page(s), decoded {}{}",
                    script_start + block.op_offset,
                    block.declared_pages,
                    block.pages.len(),
                    if block.count_matches() {
                        ""
                    } else {
                        "  [count mismatch]"
                    },
                );
                for page in &block.pages {
                    println!(
                        "      0x{:05X}  {:?}",
                        script_start + page.offset,
                        page.text
                    );
                }
            }
        }
        println!("summary: {total} narration page(s) total");
    }
    Ok(())
}

struct ResolvedVram {
    scene_name: String,
    runtime_bytes: Vec<u8>,
    source_label: String,
}

pub(crate) fn cmd_vram_oracle(args: VramOracleArgs<'_>) -> Result<()> {
    let resolved = resolve_vram_inputs(&args)?;
    let engine_bytes = build_engine_vram_bytes_with_frames(
        &resolved.scene_name,
        args.extracted_root,
        args.disc,
        args.frames,
    )?;
    let runtime_bytes = resolved.runtime_bytes;
    if runtime_bytes.len() != engine_bytes.len() {
        anyhow::bail!(
            "runtime VRAM size {} != expected {} (1 MiB BGR555)",
            runtime_bytes.len(),
            engine_bytes.len()
        );
    }

    let report = vram_coverage_report(&engine_bytes, &runtime_bytes);
    println!(
        "scene '{}'  vs runtime {}  (frames={})",
        resolved.scene_name, resolved.source_label, args.frames
    );
    print_vram_coverage(&report);
    let diff_png = args.diff_png;
    let tiles = args.tiles;
    let rows_csv = args.rows_csv;
    let clut_regions = args.clut_regions;

    if tiles {
        println!("  per-64x64-tile coverage (runtime non-zero / engine non-zero / overlap):");
        const W: usize = 1024;
        const H: usize = 512;
        for ty in 0..(H / 64) {
            for tx in 0..(W / 64) {
                let mut rt = 0u32;
                let mut en = 0u32;
                let mut ov = 0u32;
                for dy in 0..64 {
                    let y = ty * 64 + dy;
                    for dx in 0..64 {
                        let x = tx * 64 + dx;
                        let off = (y * W + x) * 2;
                        let rw = u16::from_le_bytes([runtime_bytes[off], runtime_bytes[off + 1]]);
                        let ew = u16::from_le_bytes([engine_bytes[off], engine_bytes[off + 1]]);
                        if rw != 0 {
                            rt += 1;
                        }
                        if ew != 0 {
                            en += 1;
                        }
                        if rw != 0 && ew != 0 {
                            ov += 1;
                        }
                    }
                }
                if rt > 0 || en > 0 {
                    println!(
                        "    tile ({:>3},{:>3})  rt={:5}  en={:5}  ov={:5}",
                        tx * 64,
                        ty * 64,
                        rt,
                        en,
                        ov
                    );
                }
            }
        }
    }

    if let Some(p) = diff_png {
        write_vram_diff_png(p, &engine_bytes, &runtime_bytes)?;
        println!("[ok] wrote VRAM diff PNG to {}", p.display());
    }

    if let Some(p) = rows_csv {
        write_vram_rows_csv(p, &engine_bytes, &runtime_bytes)?;
        println!("[ok] wrote per-row VRAM CSV to {}", p.display());
    }

    if clut_regions {
        print_vram_clut_region_report(&engine_bytes, &runtime_bytes);
    }

    if args.strict {
        match first_texpage_divergence(&engine_bytes, &runtime_bytes) {
            None => {
                println!("[strict] texpage region (y >= 256): byte-exact match");
            }
            Some(TexpageDivergence {
                y,
                x,
                engine_word,
                runtime_word,
            }) => {
                anyhow::bail!(
                    "[strict] texpage region diverged at row {y} col {x}: engine=0x{engine_word:04X} runtime=0x{runtime_word:04X}",
                );
            }
        }
    }

    Ok(())
}

/// Resolved input triple - `(scene_name, retail_snapshot, source_label)`.
/// `retail_snapshot` is `None` in explicit mode (no comparison).
struct ResolvedModeTrace {
    scene_name: String,
    retail: Option<ModeTraceFrame>,
    source_label: String,
}

fn resolve_mode_trace_inputs(args: &ModeTraceArgs<'_>) -> Result<ResolvedModeTrace> {
    use legaia_mednafen::ScenarioManifest;

    match (args.scenario, args.scene) {
        (Some(label), _) => {
            let manifest = ScenarioManifest::from_path(args.manifest)?;
            let scn = manifest.by_label(label).with_context(|| {
                format!("scenario {label:?} not in {}", args.manifest.display())
            })?;
            let scene_name = scn.expected_active_scene.clone().with_context(|| {
                format!("scenario {label:?} has no `expected_active_scene`; cannot derive scene",)
            })?;
            let save_path = manifest.save_path(scn.slot)?;
            if !save_path.exists() {
                anyhow::bail!(
                    "scenario {label:?} slot {} save not found at {}",
                    scn.slot,
                    save_path.display()
                );
            }
            let retail = load_runtime_mode_trace_from_save(&save_path)?;
            let source_label = format!(
                "scenario {label:?} (slot {}, {})",
                scn.slot,
                save_path.display()
            );
            Ok(ResolvedModeTrace {
                scene_name,
                retail: Some(retail),
                source_label,
            })
        }
        (None, Some(scene_name)) => Ok(ResolvedModeTrace {
            scene_name: scene_name.to_owned(),
            retail: None,
            source_label: "explicit (no retail comparison)".into(),
        }),
        _ => anyhow::bail!("mode-trace: provide either `--scenario <label>` or `--scene <name>`"),
    }
}

pub(crate) fn cmd_mode_trace(args: ModeTraceArgs<'_>) -> Result<()> {
    if args.strict && args.scenario.is_none() {
        anyhow::bail!(
            "mode-trace: `--strict` requires `--scenario` (no retail snapshot in explicit mode)"
        );
    }
    let resolved = resolve_mode_trace_inputs(&args)?;
    let trace = build_engine_mode_trace(
        &resolved.scene_name,
        args.extracted_root,
        args.disc,
        args.frames,
    )?;
    let jsonl = mode_trace_to_jsonl(&trace);

    let out_label = if args.out.as_os_str() == "-" {
        print!("{jsonl}");
        "<stdout>".to_string()
    } else {
        std::fs::write(args.out, jsonl.as_bytes())
            .with_context(|| format!("write mode-trace JSONL to {}", args.out.display()))?;
        args.out.display().to_string()
    };

    eprintln!(
        "scene '{}' vs {} (frames={}, trace_len={})  -> {}",
        resolved.scene_name,
        resolved.source_label,
        args.frames,
        trace.len(),
        out_label
    );

    if let Some(retail) = resolved.retail.as_ref() {
        let last = trace.last().unwrap();
        eprintln!(
            "  engine[last] scene_mode={:<10} active_scene={:?}",
            last.scene_mode, last.active_scene
        );
        eprintln!(
            "  retail       scene_mode={:<10} active_scene={:?}  game_mode={:?} ({})",
            retail.scene_mode,
            retail.active_scene,
            retail.game_mode,
            retail.game_mode_name.as_deref().unwrap_or("?"),
        );
        match first_mode_trace_divergence(&trace, retail) {
            None => {
                eprintln!("[ok] engine trace converges with retail snapshot");
            }
            Some(d) => {
                let msg = format!(
                    "[DRIFT] {:?}: engine(scene_mode={}, active_scene={:?}) vs retail(scene_mode={}, active_scene={:?})",
                    d.kind,
                    d.engine.scene_mode,
                    d.engine.active_scene,
                    d.retail.scene_mode,
                    d.retail.active_scene,
                );
                if args.strict {
                    anyhow::bail!("{msg}");
                } else {
                    eprintln!("{msg}");
                }
            }
        }
    }
    Ok(())
}

/// Resolved retail input for the convergence walk.
enum ResolvedRetail {
    /// Scenario-mode single SPU snapshot lifted from a mednafen `.mc{slot}`
    /// save. Compared via [`first_audio_trace_divergence`].
    Snapshot(AudioTraceFrame),
    /// Multi-frame trace lifted from a PCSX-Redux per-vsync capture (Lua
    /// probe → Python extractor → JSONL). Compared via
    /// [`first_audio_trace_divergence_multi`].
    Multi(Vec<AudioTraceFrame>),
}

/// Resolved input triple - `(scene_name, retail, source_label)`.
/// `retail` is `None` in explicit mode (no comparison).
struct ResolvedAudioTrace {
    scene_name: String,
    retail: Option<ResolvedRetail>,
    source_label: String,
}

fn resolve_audio_trace_inputs(args: &AudioTraceArgs<'_>) -> Result<ResolvedAudioTrace> {
    use legaia_mednafen::ScenarioManifest;

    // The retail-JSONL path is the multi-frame mode; it doesn't require a
    // scenario lookup because the JSONL is self-contained.
    if let Some(jsonl_path) = args.retail_jsonl {
        let scene_name = match (args.scenario, args.scene) {
            (Some(label), _) => {
                let manifest = ScenarioManifest::from_path(args.manifest)?;
                let scn = manifest.by_label(label).with_context(|| {
                    format!("scenario {label:?} not in {}", args.manifest.display())
                })?;
                scn.expected_active_scene.clone().with_context(|| {
                    format!(
                        "scenario {label:?} has no `expected_active_scene`; cannot derive scene"
                    )
                })?
            }
            (None, Some(name)) => name.to_owned(),
            _ => anyhow::bail!(
                "audio-trace --retail-jsonl: provide `--scene` or `--scenario` for the engine side"
            ),
        };
        let frames = load_runtime_audio_trace_jsonl(jsonl_path)?;
        let source_label = format!(
            "retail-jsonl {} ({} frame(s))",
            jsonl_path.display(),
            frames.len()
        );
        return Ok(ResolvedAudioTrace {
            scene_name,
            retail: Some(ResolvedRetail::Multi(frames)),
            source_label,
        });
    }

    match (args.scenario, args.scene) {
        (Some(label), _) => {
            let manifest = ScenarioManifest::from_path(args.manifest)?;
            let scn = manifest.by_label(label).with_context(|| {
                format!("scenario {label:?} not in {}", args.manifest.display())
            })?;
            let scene_name = scn.expected_active_scene.clone().with_context(|| {
                format!("scenario {label:?} has no `expected_active_scene`; cannot derive scene")
            })?;
            let save_path = manifest.save_path(scn.slot)?;
            if !save_path.exists() {
                anyhow::bail!(
                    "scenario {label:?} slot {} save not found at {}",
                    scn.slot,
                    save_path.display()
                );
            }
            let retail = load_runtime_audio_trace_from_save(&save_path)?;
            let source_label = format!(
                "scenario {label:?} (slot {}, {})",
                scn.slot,
                save_path.display()
            );
            Ok(ResolvedAudioTrace {
                scene_name,
                retail: Some(ResolvedRetail::Snapshot(retail)),
                source_label,
            })
        }
        (None, Some(scene_name)) => Ok(ResolvedAudioTrace {
            scene_name: scene_name.to_owned(),
            retail: None,
            source_label: "explicit (no retail comparison)".into(),
        }),
        _ => anyhow::bail!("audio-trace: provide either `--scenario <label>` or `--scene <name>`"),
    }
}

pub(crate) fn cmd_audio_trace(args: AudioTraceArgs<'_>) -> Result<()> {
    if args.strict && args.scenario.is_none() && args.retail_jsonl.is_none() {
        anyhow::bail!(
            "audio-trace: `--strict` requires `--scenario` or `--retail-jsonl` (no retail in explicit mode)"
        );
    }
    let resolved = resolve_audio_trace_inputs(&args)?;
    let trace = engine_trace_from_paths(
        &resolved.scene_name,
        args.extracted_root,
        args.disc,
        args.frames,
        args.bgm_id,
    )?;
    let jsonl = audio_trace_to_jsonl(&trace);

    let out_label = if args.out.as_os_str() == "-" {
        print!("{jsonl}");
        "<stdout>".to_string()
    } else {
        std::fs::write(args.out, jsonl.as_bytes())
            .with_context(|| format!("write audio-trace JSONL to {}", args.out.display()))?;
        args.out.display().to_string()
    };

    eprintln!(
        "scene '{}' vs {} (frames={}, trace_len={}, bgm_id={:?})  -> {}",
        resolved.scene_name,
        resolved.source_label,
        args.frames,
        trace.len(),
        args.bgm_id,
        out_label
    );

    let divergence = match resolved.retail.as_ref() {
        None => return Ok(()),
        Some(ResolvedRetail::Snapshot(retail)) => {
            let last = trace.last().unwrap();
            eprintln!(
                "  engine[last] mask=0b{:024b} master={:?} reverb_mode={:?}",
                last.active_voice_mask, last.master_volume, last.reverb_mode,
            );
            eprintln!(
                "  retail       mask=0b{:024b} master={:?} reverb_mode={:?}",
                retail.active_voice_mask, retail.master_volume, retail.reverb_mode,
            );
            first_audio_trace_divergence(&trace, retail)
        }
        Some(ResolvedRetail::Multi(retail_frames)) => {
            let retail_active = retail_frames
                .iter()
                .filter(|f| f.active_voice_mask != 0)
                .count();
            eprintln!(
                "  retail-trace frames={} ({} with active voices)",
                retail_frames.len(),
                retail_active,
            );
            first_audio_trace_divergence_multi(&trace, retail_frames)
        }
    };

    match divergence {
        None => eprintln!("[ok] engine trace converges with retail"),
        Some(d) => {
            let msg = format!(
                "[DRIFT] {:?}: engine(mask=0b{:024b}) vs retail(mask=0b{:024b})",
                d.kind, d.engine.active_voice_mask, d.retail.active_voice_mask,
            );
            if args.strict {
                anyhow::bail!("{msg}");
            } else {
                eprintln!("{msg}");
            }
        }
    }
    Ok(())
}

struct ResolvedPcmTrace {
    scene_name: String,
    retail_save: Option<PathBuf>,
    source_label: String,
}

fn resolve_pcm_trace_inputs(args: &PcmTraceArgs<'_>) -> Result<ResolvedPcmTrace> {
    use legaia_mednafen::ScenarioManifest;

    // Explicit `--retail-save` always wins; needs `--scene` to know what
    // to boot.
    if let Some(save) = args.retail_save {
        let scene_name = args.scene.with_context(
            || "pcm-trace: `--retail-save` requires `--scene` (no scenario lookup)",
        )?;
        if !save.exists() {
            anyhow::bail!("pcm-trace: retail save not found at {}", save.display());
        }
        return Ok(ResolvedPcmTrace {
            scene_name: scene_name.to_owned(),
            retail_save: Some(save.to_path_buf()),
            source_label: format!("explicit save ({})", save.display()),
        });
    }
    match (args.scenario, args.scene) {
        (Some(label), _) => {
            let manifest = ScenarioManifest::from_path(args.manifest)?;
            let scn = manifest.by_label(label).with_context(|| {
                format!("scenario {label:?} not in {}", args.manifest.display())
            })?;
            let scene_name = scn.expected_active_scene.clone().with_context(|| {
                format!("scenario {label:?} has no `expected_active_scene`; cannot derive scene")
            })?;
            let save_path = manifest.save_path(scn.slot)?;
            if !save_path.exists() {
                anyhow::bail!(
                    "scenario {label:?} slot {} save not found at {}",
                    scn.slot,
                    save_path.display()
                );
            }
            let source_label = format!(
                "scenario {label:?} (slot {}, {})",
                scn.slot,
                save_path.display()
            );
            Ok(ResolvedPcmTrace {
                scene_name,
                retail_save: Some(save_path),
                source_label,
            })
        }
        (None, Some(scene_name)) => Ok(ResolvedPcmTrace {
            scene_name: scene_name.to_owned(),
            retail_save: None,
            source_label: "explicit (no retail comparison)".into(),
        }),
        _ => anyhow::bail!(
            "pcm-trace: provide either `--scenario`, `--scene`, or `--retail-save` + `--scene`"
        ),
    }
}

pub(crate) fn cmd_pcm_trace(args: PcmTraceArgs<'_>) -> Result<()> {
    if args.strict && args.scenario.is_none() && args.retail_save.is_none() {
        anyhow::bail!(
            "pcm-trace: `--strict` requires a retail source (`--scenario` or `--retail-save`)"
        );
    }
    let resolved = resolve_pcm_trace_inputs(&args)?;

    let opts = legaia_engine_shell::audio_trace_oracle::AudioTraceBuildOptions {
        scene: resolved.scene_name.clone(),
        bgm_id: args.bgm_id,
        us_per_frame: 1_000_000.0 / 60.0,
        frames: args.frames,
    };
    let engine: EnginePcmTrace = build_engine_pcm_trace(args.extracted_root, args.disc, &opts)?;
    let engine_stats = pcm_stats(&engine.pcm);

    if let Some(path) = args.engine_wav {
        write_wav(path, &engine.pcm)?;
    }

    eprintln!(
        "scene '{}' vs {} (frames={}, samples_per_frame={}, total_samples={})",
        resolved.scene_name,
        resolved.source_label,
        args.frames,
        engine.samples_per_frame,
        engine.pcm.len() / 2,
    );
    eprintln!(
        "  engine peak_abs={} rms={} non_silent_samples={} sample_pairs={}",
        engine_stats.peak_abs,
        engine_stats.rms,
        engine_stats.non_silent_samples,
        engine_stats.sample_pairs,
    );

    let Some(save_path) = resolved.retail_save.as_deref() else {
        return Ok(());
    };
    let retail = retail_reference_pcm(save_path, engine.pcm.len() / 2)?;
    let retail_stats = pcm_stats(&retail);
    if let Some(path) = args.retail_wav {
        write_wav(path, &retail)?;
    }

    eprintln!(
        "  retail peak_abs={} rms={} non_silent_samples={} sample_pairs={}",
        retail_stats.peak_abs,
        retail_stats.rms,
        retail_stats.non_silent_samples,
        retail_stats.sample_pairs,
    );

    // Conservative byte-level inspection: report first divergence at a
    // generous tolerance so callers see "is engine even close" without
    // false-positive spam.
    if let Some(d) = first_pcm_divergence(&engine.pcm, &retail, 4096) {
        eprintln!(
            "  first divergence sample_pair={} channel={} engine={} retail={} delta={}",
            d.sample_pair, d.channel, d.engine, d.retail, d.delta,
        );
    } else {
        eprintln!("  engine and retail PCM agree within +/-4096 on every sample");
    }

    let hard_fail = retail_stats.rms >= 256 && engine_stats.rms == 0;
    if hard_fail {
        let msg = format!(
            "[FAIL] retail had audible output (rms={}) but engine produced complete silence over {} frames",
            retail_stats.rms, args.frames,
        );
        if args.strict {
            anyhow::bail!("{msg}");
        } else {
            eprintln!("{msg}");
        }
    } else if engine_stats.rms == 0 {
        eprintln!(
            "[ok-quiet] retail also quiet (rms={}) - soft pass",
            retail_stats.rms
        );
    } else {
        eprintln!(
            "[ok] engine produced non-zero PCM (rms={})",
            engine_stats.rms
        );
    }

    // PcmStats / EnginePcmTrace are re-exported but the CLI doesn't
    // otherwise need them; reference the type to avoid an unused-import
    // warning on the `EnginePcmTrace` binding.
    let _ = std::mem::size_of::<PcmStats>();
    Ok(())
}

/// Drive a synthetic [`World`] from a [`ReplayFile`] and write the
/// resulting mode-trace JSONL. This mirrors the J2 determinism-gate
/// harness verbatim - the gate asserts byte-identity across two runs of
/// the same input, so the subcommand is just "the determinism gate's
/// driver, plus JSONL output".
///
/// `--strict` exits non-zero when the recorded trace disagrees with the
/// replay file's `[[expected]]` fixture; without it, divergence is
/// printed to stderr but doesn't fail.
pub(crate) fn cmd_replay(input: &Path, out: &Path, strict: bool) -> Result<()> {
    let replay = ReplayFile::from_path(input)?;
    let trace = synthetic_replay_trace(&replay);
    let jsonl = mode_trace_to_jsonl(&trace);
    let out_label = if out.as_os_str() == "-" {
        print!("{jsonl}");
        "<stdout>".to_string()
    } else {
        std::fs::write(out, jsonl.as_bytes())
            .with_context(|| format!("write replay trace JSONL to {}", out.display()))?;
        out.display().to_string()
    };
    eprintln!(
        "replay '{}' (frames={}, events={}, expected={}) -> {}",
        input.display(),
        replay.meta.frames,
        replay.events.len(),
        replay.expected.len(),
        out_label,
    );
    if let Some(d) = replay.diff(&trace) {
        let msg = format!(
            "[DRIFT] frame={} kind={:?}: expected(scene_mode={}, active_scene={:?}) vs recorded(scene_mode={}, active_scene={:?})",
            d.frame,
            d.kind,
            d.expected.scene_mode,
            d.expected.active_scene,
            d.recorded.scene_mode,
            d.recorded.active_scene,
        );
        if strict {
            anyhow::bail!("{msg}");
        }
        eprintln!("{msg}");
    } else if !replay.expected.is_empty() {
        eprintln!("[ok] recorded trace matches replay [[expected]] fixture");
    }
    Ok(())
}

/// Build the engine-side mode trace by driving a synthetic [`World`]
/// through `replay`'s frame count. Mirrors
/// `crates/engine-shell/tests/determinism_j2.rs::build_mode_trace` so
/// the subcommand's behaviour is the same the determinism gate tests.
fn synthetic_replay_trace(replay: &ReplayFile) -> Vec<ModeTraceFrame> {
    let pad_stream = replay.expand_pad_stream();
    let mut world = legaia_engine_core::world::World::new();
    while world.actors.len() < 8 {
        world
            .actors
            .push(legaia_engine_core::world::Actor::default());
    }
    world.rng_state = replay.meta.rng_seed;
    let mut out = Vec::with_capacity(pad_stream.len());
    out.push(synthetic_replay_sample(&world));
    for _ in pad_stream.iter().skip(1) {
        let _ = world.tick();
        out.push(synthetic_replay_sample(&world));
    }
    out
}

fn synthetic_replay_sample(world: &legaia_engine_core::world::World) -> ModeTraceFrame {
    ModeTraceFrame {
        frame: world.frame,
        game_mode: None,
        game_mode_name: None,
        scene_mode: synthetic_replay_scene_mode_name(world.mode).to_string(),
        active_scene: None,
    }
}

fn synthetic_replay_scene_mode_name(m: legaia_engine_core::world::SceneMode) -> &'static str {
    use legaia_engine_core::world::SceneMode;
    match m {
        SceneMode::Title => "Title",
        SceneMode::Field => "Field",
        SceneMode::Battle => "Battle",
        SceneMode::Cutscene => "Cutscene",
        SceneMode::WorldMap => "WorldMap",
        SceneMode::Dance => "Dance",
        SceneMode::Fishing => "Fishing",
        SceneMode::Menu => "Menu",
    }
}

/// VRAM regions known to carry CLUT (colour-lookup-table) data, by Y row
/// and approximate X span. The renderer treats CLUTs as 16- or 256-entry
/// rows of u16 BGR555 anywhere in VRAM; the project's RE has surfaced
/// specific bands that scene-pack uploads target.
///
/// Each entry is `(label, y, x_start, width)`; width is in pixels (not
/// bytes), and a CLUT row is one pixel tall by definition.
const VRAM_CLUT_BANDS: &[(&str, usize, usize, usize)] = &[
    // Row-479 NPC palette band (see docs/formats/npc-palette.md +
    // project_row479_global_hue_ramp memory). Scene-pack TIMs upload
    // 16- and 32-entry CLUTs into this row at fb_x=0..256.
    ("npc-clut row 479           x=  0..256", 479, 0, 256),
    // Common low-pages-area CLUT rows used by character / scene
    // textures. Most scenes touch at least one row in 480..512.
    ("char-clut row 480           x=  0..256", 480, 0, 256),
    ("char-clut row 481           x=  0..256", 481, 0, 256),
    ("char-clut row 496           x=  0..256", 496, 0, 256),
    // Display framebuffer scan rows. These are normally rewritten
    // every frame so any "engine populated this from the static
    // upload" content is suspect.
    ("framebuffer scanline y= 16  x=  0..640", 16, 0, 640),
    ("framebuffer scanline y=128  x=  0..640", 128, 0, 640),
];

fn write_vram_rows_csv(path: &Path, engine: &[u8], runtime: &[u8]) -> Result<()> {
    const W: usize = 1024;
    const H: usize = 512;
    let mut s = String::new();
    s.push_str("y,runtime_nz,engine_nz,overlap,runtime_only,engine_only\n");
    for y in 0..H {
        let mut rt = 0u32;
        let mut en = 0u32;
        let mut ov = 0u32;
        let mut rt_only = 0u32;
        let mut en_only = 0u32;
        let row_base = y * W * 2;
        for x in 0..W {
            let off = row_base + x * 2;
            let rw = u16::from_le_bytes([runtime[off], runtime[off + 1]]);
            let ew = u16::from_le_bytes([engine[off], engine[off + 1]]);
            let rnz = rw != 0;
            let enz = ew != 0;
            if rnz {
                rt += 1;
            }
            if enz {
                en += 1;
            }
            match (rnz, enz) {
                (true, true) => ov += 1,
                (true, false) => rt_only += 1,
                (false, true) => en_only += 1,
                _ => {}
            }
        }
        s.push_str(&format!("{y},{rt},{en},{ov},{rt_only},{en_only}\n"));
    }
    std::fs::write(path, s).with_context(|| format!("write VRAM rows CSV {}", path.display()))?;
    Ok(())
}

fn print_vram_clut_region_report(engine: &[u8], runtime: &[u8]) {
    const W: usize = 1024;
    const H: usize = 512;
    println!();
    println!("VRAM CLUT-region health (engine vs runtime):");
    println!(
        "  {:<48} {:>5} {:>5} {:>5} {:>6} {:>6}",
        "band", "rt", "en", "ov", "rt-only", "en-only"
    );
    for &(label, y, x0, w) in VRAM_CLUT_BANDS {
        if y >= H {
            continue;
        }
        let row_base = y * W * 2;
        let mut rt = 0u32;
        let mut en = 0u32;
        let mut ov = 0u32;
        let mut rt_only = 0u32;
        let mut en_only = 0u32;
        let x_end = (x0 + w).min(W);
        for x in x0..x_end {
            let off = row_base + x * 2;
            let rw = u16::from_le_bytes([runtime[off], runtime[off + 1]]);
            let ew = u16::from_le_bytes([engine[off], engine[off + 1]]);
            let rnz = rw != 0;
            let enz = ew != 0;
            if rnz {
                rt += 1;
            }
            if enz {
                en += 1;
            }
            match (rnz, enz) {
                (true, true) => ov += 1,
                (true, false) => rt_only += 1,
                (false, true) => en_only += 1,
                _ => {}
            }
        }
        let pct = if rt > 0 {
            100.0 * (ov as f64) / (rt as f64)
        } else {
            0.0
        };
        let flag = if rt_only > 0 && rt > 0 {
            " <-- gap"
        } else {
            ""
        };
        println!(
            "  {label:<48} {rt:>5} {en:>5} {ov:>5} {rt_only:>6} {en_only:>6}  ({pct:5.1}%){flag}"
        );
    }
}

fn write_vram_png(path: &Path, bgr555_le: &[u8]) -> Result<()> {
    const W: u32 = 1024;
    const H: u32 = 512;
    let rgba = legaia_mednafen::vram_to_rgba8(bgr555_le);
    let f = std::fs::File::create(path)
        .with_context(|| format!("create VRAM PNG {}", path.display()))?;
    let bw = std::io::BufWriter::new(f);
    let mut enc = png::Encoder::new(bw, W, H);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&rgba)?;
    Ok(())
}

/// Compact per-region VRAM coverage report.
struct VramCoverage {
    /// Per-tile counts. Each tile is 64x64 pixels (16 tiles wide, 8 rows tall).
    runtime_nonzero_pixels: u64,
    engine_nonzero_pixels: u64,
    overlap_pixels: u64,
    runtime_only_pixels: u64,
    engine_only_pixels: u64,
    /// `(y_range_label, runtime_nonzero, engine_nonzero, overlap)` for
    /// the common VRAM regions.
    bands: Vec<(&'static str, u64, u64, u64)>,
}

fn vram_coverage_report(engine: &[u8], runtime: &[u8]) -> VramCoverage {
    const W: usize = 1024;
    const H: usize = 512;
    let mut runtime_nz = 0u64;
    let mut engine_nz = 0u64;
    let mut overlap = 0u64;
    let mut runtime_only = 0u64;
    let mut engine_only = 0u64;
    for i in 0..(W * H) {
        let off = i * 2;
        let rw = u16::from_le_bytes([runtime[off], runtime[off + 1]]);
        let ew = u16::from_le_bytes([engine[off], engine[off + 1]]);
        let rnz = rw != 0;
        let enz = ew != 0;
        if rnz {
            runtime_nz += 1;
        }
        if enz {
            engine_nz += 1;
        }
        match (rnz, enz) {
            (true, true) => overlap += 1,
            (true, false) => runtime_only += 1,
            (false, true) => engine_only += 1,
            _ => {}
        }
    }
    // Band reports split VRAM into top half (display + scratch) and bottom
    // half (texture pages + CLUTs), then split the bottom into upper-256
    // (typical character / scene textures) and lower-256 (extra texture
    // pages, CLUT rows).
    let band = |y0: usize, y1: usize| -> (u64, u64, u64) {
        let mut rt = 0u64;
        let mut en = 0u64;
        let mut ov = 0u64;
        for y in y0..y1 {
            for x in 0..W {
                let off = (y * W + x) * 2;
                let rw = u16::from_le_bytes([runtime[off], runtime[off + 1]]);
                let ew = u16::from_le_bytes([engine[off], engine[off + 1]]);
                let rnz = rw != 0;
                let enz = ew != 0;
                if rnz {
                    rt += 1;
                }
                if enz {
                    en += 1;
                }
                if rnz && enz {
                    ov += 1;
                }
            }
        }
        (rt, en, ov)
    };
    let mut bands = Vec::new();
    let (rt, en, ov) = band(0, 256);
    bands.push(("top half y=  0..256 (display FB + scratch)", rt, en, ov));
    let (rt, en, ov) = band(256, 384);
    bands.push(("texpage rows y=256..384 (primary textures)", rt, en, ov));
    let (rt, en, ov) = band(384, 512);
    bands.push(("texpage rows y=384..512 (textures + CLUTs)", rt, en, ov));
    VramCoverage {
        runtime_nonzero_pixels: runtime_nz,
        engine_nonzero_pixels: engine_nz,
        overlap_pixels: overlap,
        runtime_only_pixels: runtime_only,
        engine_only_pixels: engine_only,
        bands,
    }
}

fn print_vram_coverage(c: &VramCoverage) {
    let total_runtime = c.runtime_nonzero_pixels.max(1);
    println!("VRAM coverage (engine vs runtime, BGR555 != 0 pixel mask)");
    println!(
        "  runtime non-zero pixels:  {:>8}   (= the loaded VRAM ground truth)",
        c.runtime_nonzero_pixels
    );
    println!("  engine  non-zero pixels:  {:>8}", c.engine_nonzero_pixels);
    println!(
        "  overlap (engine ∩ rt):    {:>8}   ({:.1}% of runtime)",
        c.overlap_pixels,
        100.0 * c.overlap_pixels as f64 / total_runtime as f64
    );
    println!(
        "  runtime-only (gap):       {:>8}   ({:.1}% missing in engine)",
        c.runtime_only_pixels,
        100.0 * c.runtime_only_pixels as f64 / total_runtime as f64
    );
    println!("  engine-only (extra):      {:>8}", c.engine_only_pixels);
    println!("  per-band breakdown:");
    for (label, rt, en, ov) in &c.bands {
        let pct = if *rt > 0 {
            100.0 * (*ov as f64) / (*rt as f64)
        } else {
            0.0
        };
        println!("    {label:<48} runtime={rt:>7} engine={en:>7} overlap={ov:>7} ({pct:5.1}%)");
    }
}

fn write_vram_diff_png(path: &Path, engine: &[u8], runtime: &[u8]) -> Result<()> {
    const W: u32 = 1024;
    const H: u32 = 512;
    let mut rgba = Vec::with_capacity((W * H * 4) as usize);
    for i in 0..(W as usize * H as usize) {
        let off = i * 2;
        let rw = u16::from_le_bytes([runtime[off], runtime[off + 1]]);
        let ew = u16::from_le_bytes([engine[off], engine[off + 1]]);
        let rnz = rw != 0;
        let enz = ew != 0;
        let color = match (rnz, enz) {
            (false, false) => [0u8, 0, 0, 0xFF],
            // Engine matches runtime exactly (same word) → grey
            (true, true) if rw == ew => [0x60, 0x60, 0x60, 0xFF],
            // Both non-zero but different content → blue
            (true, true) => [0x30, 0x80, 0xFF, 0xFF],
            // Runtime has content engine doesn't → red (the gap)
            (true, false) => [0xFF, 0x40, 0x40, 0xFF],
            // Engine has content runtime doesn't → green (extras / wrong slot)
            (false, true) => [0x40, 0xFF, 0x40, 0xFF],
        };
        rgba.extend_from_slice(&color);
    }
    let f = std::fs::File::create(path)
        .with_context(|| format!("create diff PNG {}", path.display()))?;
    let bw = std::io::BufWriter::new(f);
    let mut enc = png::Encoder::new(bw, W, H);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&rgba)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_play(
    scene: &str,
    extracted_root: &std::path::Path,
    disc: Option<&std::path::Path>,
    frames: u64,
    enable_audio: bool,
    frame_ms: u64,
    str_file: Option<&Path>,
    cutscene_map_path: Option<&Path>,
) -> Result<()> {
    // If the user supplied a `--cutscene-map` TOML doc, install it as the
    // explicit override layer; otherwise fall back to the heuristic.
    let cutscene_map = if let Some(p) = cutscene_map_path {
        legaia_engine_core::scene::CutsceneMap::from_toml_path(p)
            .with_context(|| format!("load cutscene map {}", p.display()))?
    } else {
        legaia_engine_core::scene::CutsceneMap::default()
    };
    if cutscene_map_path.is_some() {
        eprintln!(
            "info: cutscene-map loaded with {} explicit entry/entries",
            cutscene_map.len()
        );
    }
    // Auto-resolve a `--scene op*` / `--scene edteien` request to its
    // paired FMV via the cutscene map (which falls through to the
    // hard-coded heuristic) when the user didn't explicitly pass
    // `--str-file` and the extracted root has the file on disk.
    let auto_str = match (str_file, disc) {
        (Some(_), _) => None,
        (None, None) => cutscene_map
            .resolve(scene)
            .map(|rel| extracted_root.join(rel))
            .filter(|p| p.exists()),
        // Disc-mode resolution would need an ISO9660 read; punt.
        (None, Some(_)) => None,
    };
    let resolved_str: Option<&Path> = str_file.or(auto_str.as_deref());

    // If a STR file was supplied (explicitly or auto-resolved), pre-decode
    // it headlessly and log the frame count. This is phase 1 for
    // `op*`/`ed*` in-engine cutscene scenes where an FMV precedes the
    // dialogue-overlay scene proper. The scene ticking (phase 2) runs
    // unconditionally after this block.
    if let Some(str_path) = resolved_str {
        let decoded = decode_str_frame_count(str_path)
            .with_context(|| format!("read STR file {}", str_path.display()))?;
        println!(
            "play: pre-decoded {} STR frames from {}",
            decoded,
            str_path.display()
        );
    }

    let cfg = BootConfig {
        scene: scene.to_string(),
        enable_audio,
    };
    let mut session = match disc {
        Some(disc_path) => BootSession::open_disc(disc_path, &cfg)?,
        None => BootSession::open(extracted_root, &cfg)?,
    };
    println!(
        "play: scene='{}' frames={} audio={} (entries={}, MES={}, VAB={}, SEQ={})",
        scene,
        if frames == 0 {
            "∞".into()
        } else {
            frames.to_string()
        },
        if session.audio.is_some() { "on" } else { "off" },
        session
            .host
            .scene
            .as_ref()
            .map(|s| s.entries.len())
            .unwrap_or(0),
        if session
            .host
            .assets
            .as_ref()
            .map(|a| a.mes.is_some())
            .unwrap_or(false)
        {
            "yes"
        } else {
            "no"
        },
        session
            .host
            .assets
            .as_ref()
            .map(|a| a.vab_entries.len())
            .unwrap_or(0),
        session
            .host
            .assets
            .as_ref()
            .map(|a| a.seq_entries.len() + a.seq_in_stream_entries.len())
            .unwrap_or(0),
    );

    let mut transitions = 0u64;
    let mut bgm_events = 0u64;
    let mut last_log = 0u64;
    let mut tick_count = 0u64;
    while frames == 0 || tick_count < frames {
        let event = session.tick()?;
        match event {
            SceneTickEvent::SceneEntered { name } => {
                transitions += 1;
                println!("frame {}: entered scene '{}'", tick_count, name);
            }
            SceneTickEvent::UnknownMapId { map_id } => {
                println!(
                    "frame {}: scene_transition({}) had no mapped scene",
                    tick_count, map_id
                );
            }
            SceneTickEvent::Stepped => {}
        }
        // Field -> Cutscene -> Field flow: when the field VM's FMV-trigger op
        // flips the world into the cutscene mode (game mode 26 / StrInit), play
        // the resolved `MV*.STR` here (headless MDEC decode) and tell the world
        // playback finished so the field resumes. The STR overlay owns the
        // frame in retail; the world keeps the field VM suspended until then.
        if let Some(fmv_id) = session.host.world.active_fmv() {
            match session.host.world.active_fmv_str_filename() {
                Some(rel) => {
                    let path = extracted_root.join(rel);
                    match decode_str_frame_count(&path) {
                        Ok(n) => println!(
                            "frame {tick_count}: cutscene fmv_id={fmv_id} {rel} ({n} frames)"
                        ),
                        Err(_) => println!(
                            "frame {tick_count}: cutscene fmv_id={fmv_id} {rel} (not extracted; skipped)"
                        ),
                    }
                }
                None => {
                    println!("frame {tick_count}: cutscene fmv_id={fmv_id} (cut path; skipped)")
                }
            }
            session.host.world.finish_cutscene();
        }
        if let Some(bgm) = session.bgm.as_ref()
            && bgm.last_started.is_some()
        {
            bgm_events = bgm_events.max(1);
        }
        if tick_count - last_log >= 60 {
            last_log = tick_count;
            log::info!(
                "frame {}: world.frame={}, transitions={}, bgm_started={}",
                tick_count,
                session.host.world.frame,
                transitions,
                bgm_events
            );
        }
        if frame_ms > 0 {
            std::thread::sleep(Duration::from_millis(frame_ms));
        }
        tick_count += 1;
    }
    println!(
        "exit: ticked {} frames, world.frame={}, transitions={}",
        tick_count, session.host.world.frame, transitions
    );
    Ok(())
}

pub(crate) fn cmd_save(
    extracted_root: &std::path::Path,
    disc: Option<&std::path::Path>,
    save_dir: &std::path::Path,
    slot: u8,
    party_size: usize,
) -> Result<()> {
    use legaia_engine_core::menu_runtime::MenuRuntime;
    use legaia_engine_core::world::World;
    use legaia_save::{CharacterRecord, Party};

    let _ = (extracted_root, disc);
    let mut world = World::default();
    let members = (0..party_size).map(|_| CharacterRecord::zeroed()).collect();
    world.load_party(Party { members });
    world.story_flags = 0;
    world.money = 0;
    let runtime = MenuRuntime::new(save_dir.to_path_buf());
    let path = runtime.save_to_slot(&mut world, slot)?;
    let sf = world.save_full();
    println!(
        "saved slot {} to {} (party={}, story_flags={:#010X}, money={}, inventory={})",
        slot,
        path.display(),
        sf.party.members.len(),
        sf.ext.story_flags,
        sf.ext.money,
        sf.ext.inventory.len()
    );
    Ok(())
}

pub(crate) fn cmd_load(save_dir: &std::path::Path, slot: u8) -> Result<()> {
    use legaia_engine_core::menu_runtime::MenuRuntime;
    use legaia_engine_core::world::World;

    let runtime = MenuRuntime::new(save_dir.to_path_buf());
    let mut world = World::default();
    let path = runtime.load_from_slot(&mut world, slot)?;
    println!(
        "loaded slot {} from {} (party={}, story_flags={:#010X}, money={}, inventory={}, actors={})",
        slot,
        path.display(),
        world.roster.members.len(),
        world.story_flags,
        world.money,
        world.inventory.len(),
        world.actors.iter().filter(|a| a.active).count()
    );
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
        if let Some((start, end)) = cdname::block_range_for_name(&map, name) {
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

/// Open a `ProtIndex` from either an extracted directory (default) or a
/// disc image (when `--disc` was provided). Used by subcommands that
/// accept either source.
fn open_index_from_args(
    extracted_root: &std::path::Path,
    disc: Option<&std::path::Path>,
) -> Result<ProtIndex> {
    if let Some(disc_path) = disc {
        use legaia_engine_core::{DiscVfs, Vfs};
        let vfs = DiscVfs::open(disc_path)
            .with_context(|| format!("open disc image {}", disc_path.display()))?;
        let prot_bytes = vfs
            .read("prot.dat")
            .context("PROT.DAT not present in disc image")?;
        let cdname_text = vfs
            .read("cdname.txt")
            .or_else(|_| vfs.read("data/cdname.txt"))
            .ok()
            .map(|b| String::from_utf8(b).context("CDNAME.TXT is not valid UTF-8"))
            .transpose()?;
        ProtIndex::from_bytes(prot_bytes, cdname_text.as_deref())
            .with_context(|| format!("build ProtIndex from {}", disc_path.display()))
    } else {
        let prot = extracted_root.join("PROT.DAT");
        if !prot.exists() {
            anyhow::bail!(
                "missing {} (run `legaia-extract` first or pass --disc PATH)",
                prot.display()
            );
        }
        ProtIndex::open_extracted(extracted_root)
            .with_context(|| format!("open ProtIndex at {}", extracted_root.display()))
    }
}

pub(crate) fn cmd_config(cmd: ConfigCmd) -> Result<()> {
    use legaia_engine_core::input::Mapping;
    match cmd {
        ConfigCmd::Show { config_file } => {
            let mapping = Mapping::load_or_default(&config_file);
            let mut pairs: Vec<_> = mapping.bindings.iter().collect();
            pairs.sort_by_key(|(k, _)| k.as_str());
            println!("input mapping ({})", config_file.display());
            for (key, btn) in &pairs {
                println!("  {key:<12} → {btn}");
            }
        }
        ConfigCmd::Set {
            binding,
            config_file,
        } => {
            let Some((key, btn)) = binding.split_once('=') else {
                anyhow::bail!("--binding must be KEY=BUTTON (e.g. Z=Cross)");
            };
            let key = key.trim().to_string();
            let btn = btn.trim().to_string();
            // Validate that the button name is known.
            if legaia_engine_core::input::PadButton::from_name(&btn).is_none() {
                anyhow::bail!(
                    "unknown pad button '{}'; valid names: Select L3 R3 Start Up Right Down Left L2 R2 L1 R1 Triangle Circle Cross Square",
                    btn
                );
            }
            let mut mapping = Mapping::load_or_default(&config_file);
            mapping.bindings.insert(key.clone(), btn.clone());
            mapping.save(&config_file)?;
            println!("binding saved: {key} → {btn} ({})", config_file.display());
        }
        ConfigCmd::DumpCutsceneMap { out } => {
            let map = legaia_engine_core::scene::CutsceneMap::from_heuristic();
            let toml_doc = map.to_toml_string();
            if out.as_os_str() == "-" {
                print!("{toml_doc}");
            } else {
                std::fs::write(&out, &toml_doc)
                    .with_context(|| format!("write {}", out.display()))?;
                println!(
                    "wrote {} cutscene-map entry/entries → {}",
                    map.len(),
                    out.display()
                );
            }
        }
    }
    Ok(())
}

/// Drive a synthetic [`BattleSession`] end-to-end. Reports per-frame
/// session events and the final phase. Intended as a smoke test for the
/// orchestrator wiring; engines that want a full UI use `play-window`
/// (which can host a `BattleSession` via the renderer's HUD draws).
pub(crate) fn cmd_battle(
    monsters: u8,
    monster_hp: u16,
    max_ticks: u64,
    script: &str,
) -> Result<()> {
    use legaia_art::Character;
    use legaia_engine_core::ap_gauge::ApGauge;
    use legaia_engine_core::battle_session::{
        BattlePhase, BattleSession, SessionInput, SessionSlotInfo,
    };
    use legaia_engine_core::battle_stats::StatRecord;
    use legaia_engine_core::world::{Actor, World};

    let mut session = BattleSession::new();
    session.set_party([Character::Vahn, Character::Noa, Character::Gala]);
    let names = ["Vahn", "Noa", "Gala"];
    for (i, name) in names.iter().enumerate() {
        session.set_slot_info(
            i as u8,
            SessionSlotInfo {
                name: (*name).into(),
                is_party: true,
                record: Some(StatRecord {
                    base_attack: 50,
                    base_udf: 30,
                    base_ldf: 25,
                    base_accuracy: 80,
                    base_evasion: 20,
                    ..Default::default()
                }),
                mp_max: 30,
            },
        );
    }
    let monster_count = monsters.min(5);
    for i in 0..monster_count {
        session.set_slot_info(
            3 + i,
            SessionSlotInfo {
                name: format!("Mon{i}"),
                is_party: false,
                record: Some(StatRecord {
                    base_attack: 30,
                    base_udf: 20,
                    base_ldf: 15,
                    base_accuracy: 70,
                    base_evasion: 10,
                    ..Default::default()
                }),
                mp_max: 0,
            },
        );
    }
    session.set_monster_count(monster_count);

    let mut world = World::new();
    while world.actors.len() < 8 {
        world.actors.push(Actor::default());
    }
    for i in 0..3 {
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
        world.actors[i].battle.mp = 30;
        world.ap_gauges[i] = ApGauge::with_base(8);
    }
    for i in 0..monster_count as usize {
        world.actors[3 + i].battle.hp = monster_hp;
        world.actors[3 + i].battle.max_hp = monster_hp;
    }

    session.begin_round(&mut world);
    println!(
        "battle: party=3 monsters={} phase={:?}",
        monster_count,
        session.phase()
    );

    let mut script_iter = script.chars();
    let mut total_events = 0usize;
    for tick in 0..max_ticks {
        let mut input = SessionInput::default();
        if let Some(c) = script_iter.next() {
            apply_script_char(c, &mut input);
        }
        let events = session.tick(&mut world, input);
        if !events.is_empty() {
            total_events += events.len();
            for ev in &events {
                println!("[t{tick}] {ev:?}");
            }
        }
        if session.is_done() {
            println!("battle ended at tick {tick}: {:?}", session.phase());
            break;
        }
        if matches!(session.phase(), BattlePhase::Idle) {
            break;
        }
    }
    println!(
        "battle: total_events={} final_phase={:?} hud_active_slots={}",
        total_events,
        session.phase(),
        session.hud.active_slots()
    );
    Ok(())
}

fn apply_script_char(c: char, input: &mut legaia_engine_core::battle_session::SessionInput) {
    use legaia_engine_core::battle_session::SessionInput as SI;
    let _: &SI = input;
    match c {
        'R' => input.right = true,
        'L' => input.left = true,
        'U' => input.up = true,
        'D' => input.down = true,
        'c' => input.cross = true,
        'o' => input.circle = true,
        't' => input.triangle = true,
        's' => input.square = true,
        'S' => input.start = true,
        _ => {}
    }
}

/// Drive a synthetic [`InventoryUseSession`] against a small world.
/// Reports cursor moves + the final outcome.
pub(crate) fn cmd_inventory(item: u8, party_size: u8, script: &str) -> Result<()> {
    use legaia_engine_core::inventory_use::{
        InventoryContext, InventoryUseInput, InventoryUseSession, TargetRow,
    };
    use legaia_engine_core::items::ItemCatalog;

    let catalog = ItemCatalog::vanilla();
    if catalog.get(item).is_none() {
        anyhow::bail!(
            "item id 0x{item:02X} not in vanilla catalog - pick from 0x10..0x41 or extend the catalog"
        );
    }
    let mut targets: Vec<TargetRow> = Vec::new();
    for i in 0..party_size {
        targets.push(TargetRow::new(i, format!("Slot{i}")).with_stats(50, 100, 10, 30));
    }

    let mut session =
        InventoryUseSession::new(catalog, vec![item], targets, InventoryContext::Field);
    println!("inventory: item=0x{item:02X} party_size={party_size}");
    for (idx, c) in script.chars().enumerate() {
        let input = match c {
            'U' => InventoryUseInput::Up,
            'D' => InventoryUseInput::Down,
            'c' => InventoryUseInput::Confirm,
            'o' => InventoryUseInput::Cancel,
            _ => continue,
        };
        session.input(input);
        let evs = session.drain_events();
        for ev in &evs {
            println!("[s{idx}={c}] {ev:?}");
        }
        if session.is_done() {
            break;
        }
    }
    println!("inventory: state={:?}", session.state);
    Ok(())
}

/// Run an equip session that confirms `item` into `slot`. Useful as a
/// smoke test for the SM and the BattleStats recompute path.
pub(crate) fn cmd_equip(slot: u8, item: u8) -> Result<()> {
    use legaia_engine_core::battle_stats::{
        EquipmentTable, ItemModifier, StatRecord, StatusModifiers,
    };
    use legaia_engine_core::equip_session::{EquipInput, EquipOutcome, EquipSession};
    use std::collections::HashMap;

    let record = StatRecord {
        base_attack: 50,
        base_udf: 30,
        base_ldf: 25,
        base_accuracy: 80,
        base_evasion: 20,
        base_spd: 35,
        base_int: 18,
        equip: [0; 8],
    };
    let mut inv = HashMap::new();
    // Re-encode the item id so its implied slot matches the requested
    // slot - the synthetic test catalog uses `id >> 5` as the slot bits.
    let encoded_id = (slot << 5) | (item & 0x1F);
    inv.insert(encoded_id, 1);
    let mut eq = EquipmentTable::new();
    eq.set(
        encoded_id,
        ItemModifier {
            atk: 10,
            ..Default::default()
        },
    );
    let mut session = EquipSession::new(record, inv, eq, StatusModifiers::default(), Vec::new());

    println!("equip: requested slot={slot} item=0x{item:02X} (encoded 0x{encoded_id:02X})");

    // Drive: down `slot` times to reach the slot, cross to enter picker,
    // cross to confirm item, cross to commit.
    let mut step_count = 0;
    for _ in 0..slot {
        session.input(EquipInput {
            down: true,
            ..Default::default()
        });
        step_count += 1;
    }
    session.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    step_count += 1;
    session.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    step_count += 1;
    session.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    step_count += 1;

    println!(
        "equip: drove {step_count} inputs; outcome={:?}",
        session.outcome()
    );
    if let Some(EquipOutcome::Committed {
        added,
        slot: out_slot,
        removed,
    }) = session.outcome()
    {
        println!("equip: committed slot={out_slot} added=0x{added:02X} removed=0x{removed:02X}");
        println!(
            "equip: post-commit ATK={} (record.equip[{}]=0x{:02X})",
            session.preview_stats.atk,
            out_slot,
            session.record().equip[out_slot as usize]
        );
    }
    Ok(())
}

/// Load a JSON Cop2Trace and replay it through a fresh emulator. Reports
/// any per-step register divergence; exits 0 on clean replay.
pub(crate) fn cmd_gte_replay(trace_path: &Path, verbose: bool) -> Result<()> {
    use legaia_engine_render::gte_trace::Cop2Trace;
    let bytes = std::fs::read(trace_path)
        .with_context(|| format!("read trace file {}", trace_path.display()))?;
    let json = std::str::from_utf8(&bytes).context("trace file is not valid UTF-8")?;
    let trace = Cop2Trace::read_json(json).context("parse trace JSON")?;
    println!(
        "gte-replay: loaded {} steps (label={})",
        trace.len(),
        trace.label.as_deref().unwrap_or("<none>")
    );
    let mismatches = trace.replay();
    if mismatches.is_empty() {
        println!("gte-replay: clean - every step replayed bit-exact");
        if verbose {
            println!("gte-replay: trace label = {:?}", trace.label);
        }
        return Ok(());
    }
    eprintln!(
        "gte-replay: {} step(s) diverged from the recorded snapshot",
        mismatches.len()
    );
    for m in &mismatches {
        eprintln!("  step {} ({}):", m.step, m.op);
        for f in &m.fields {
            eprintln!(
                "    {} expected={} actual={}",
                f.field, f.expected, f.actual
            );
        }
    }
    anyhow::bail!("trace replay produced mismatches");
}

/// Map an input letter to a [`legaia_engine_core::title::TitleInput`] mask.
fn title_input_for(c: char) -> legaia_engine_core::title::TitleInput {
    use legaia_engine_core::title::TitleInput;
    let mut i = TitleInput::default();
    match c {
        's' => i.start = true,
        'c' => i.cross = true,
        'o' => i.circle = true,
        'U' => i.up = true,
        'D' => i.down = true,
        _ => {}
    }
    i
}

pub(crate) fn cmd_title(script: &str, no_save: bool, fade_frames: u16) -> Result<()> {
    use legaia_engine_core::title::{TitleEvent, TitleSession};
    let mut s = if no_save {
        TitleSession::without_save_data()
    } else {
        TitleSession::new()
    };
    s.fade_in_frames = fade_frames;
    s.skip_fade_in();
    println!("title: starting (no_save={no_save})");
    for (i, ch) in script.chars().enumerate() {
        if s.is_done() {
            break;
        }
        let evs = s.tick(title_input_for(ch));
        for e in evs {
            match e {
                TitleEvent::CursorMoved { row } => println!("  tick {i}: cursor → {row}"),
                TitleEvent::StartPressed => println!("  tick {i}: start pressed"),
                TitleEvent::MenuConfirmed { row } => println!("  tick {i}: confirmed row {row}"),
                TitleEvent::NewGameSelected => println!("  tick {i}: NewGame"),
                TitleEvent::ContinueSelected => println!("  tick {i}: Continue"),
                TitleEvent::OptionsSelected => println!("  tick {i}: Options"),
                TitleEvent::FadeInDone => println!("  tick {i}: fade-in done"),
            }
        }
    }
    println!("title: outcome = {:?}", s.outcome());
    Ok(())
}

fn select_input_for(c: char) -> legaia_engine_core::save_select::SelectInput {
    use legaia_engine_core::save_select::SelectInput;
    let mut i = SelectInput::default();
    match c {
        'c' => i.cross = true,
        'o' => i.circle = true,
        't' => i.triangle = true,
        'U' => i.up = true,
        'D' => i.down = true,
        'L' => i.left = true,
        'R' => i.right = true,
        _ => {}
    }
    i
}

pub(crate) fn cmd_save_select(mode: &str, slots: &str, script: &str) -> Result<()> {
    use legaia_engine_core::save_select::{
        SaveSelectMode, SaveSelectSession, SelectEvent, SlotSnapshot,
    };
    let mode = match mode.to_ascii_lowercase().as_str() {
        "load" => SaveSelectMode::Load,
        "save" => SaveSelectMode::Save,
        other => anyhow::bail!("unknown save-select mode: {other}"),
    };
    let snapshots: Vec<SlotSnapshot> = slots
        .split(',')
        .enumerate()
        .map(|(i, p)| {
            let present = p.trim() == "1";
            if present {
                SlotSnapshot {
                    slot: i as u8,
                    present: true,
                    label: format!("Slot {i}: Vahn  Lv 5"),
                    play_time_seconds: 1234,
                    party_lv: 5,
                    location: "Town01".into(),
                    money: 100,
                    leader_char_id: 0,
                    leader_name: "Vahn".into(),
                    leader_hp: (100, 100),
                    leader_mp: (20, 20),
                }
            } else {
                SlotSnapshot::empty(i as u8)
            }
        })
        .collect();
    let mut s = SaveSelectSession::new(mode, snapshots);
    println!(
        "save-select: mode={:?}, {} slot(s)",
        s.mode(),
        s.slots().len()
    );
    for (i, ch) in script.chars().enumerate() {
        if s.is_done() {
            break;
        }
        let evs = s.tick(select_input_for(ch));
        for e in evs {
            match e {
                SelectEvent::CursorMoved { slot } => {
                    println!("  tick {i}: cursor → slot {slot}")
                }
                SelectEvent::EnteredConfirm { slot, kind } => {
                    println!("  tick {i}: entered {:?} confirm on slot {slot}", kind)
                }
                SelectEvent::Confirmed { slot, kind } => {
                    println!("  tick {i}: confirmed {:?} on slot {slot}", kind)
                }
                SelectEvent::ConfirmCancelled { slot, kind } => {
                    println!("  tick {i}: cancelled {:?} on slot {slot}", kind)
                }
                SelectEvent::InvalidConfirm => println!("  tick {i}: invalid confirm"),
                SelectEvent::EnteredNowChecking { slot } => {
                    println!("  tick {i}: entered NowChecking on slot {slot}")
                }
                SelectEvent::EnteredSlotPreview { slot } => {
                    println!("  tick {i}: entered SlotPreview on slot {slot}")
                }
                SelectEvent::LoadConfirmed { slot } => {
                    println!("  tick {i}: load confirmed on slot {slot}")
                }
                SelectEvent::SlotPreviewCancelled { slot } => {
                    println!("  tick {i}: slot preview cancelled on slot {slot}")
                }
                SelectEvent::Cancelled => println!("  tick {i}: cancelled"),
            }
        }
    }
    println!("save-select: outcome = {:?}", s.outcome());
    Ok(())
}

pub(crate) fn cmd_encounter(rate: u8, steps: u32, seed: u32) -> Result<()> {
    use legaia_engine_core::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("test_scene");
    table.set_trigger_rate(rate);
    table.push(EncounterEntry::new(1, 50));
    table.push(EncounterEntry::new(2, 30));
    table.push(EncounterEntry::new(3, 20));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    let mut rng = seed;
    let mut hit_step = None;
    for step in 0..steps {
        // xorshift32
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        if session.on_step(rng) {
            hit_step = Some(step);
            break;
        }
    }
    if let Some(s) = hit_step {
        // Drain through transition.
        for _ in 0..session.transition_frames + 1 {
            session.tick_frame();
        }
        if let Some(roll) = session.drain_triggered() {
            println!(
                "encounter: triggered at step {s} → formation {} (roll q8={})",
                roll.formation_id, roll.roll_q8
            );
        } else {
            println!("encounter: triggered at step {s} but transition lost");
        }
    } else {
        println!("encounter: no trigger after {steps} step(s)");
    }
    println!(
        "encounter: total_steps={} steps_since_last={}",
        session.tracker().total_steps(),
        session.tracker().steps_since_last_battle()
    );
    Ok(())
}

fn picker_input_for(c: char) -> legaia_engine_core::target_picker::PickerInput {
    use legaia_engine_core::target_picker::PickerInput;
    let mut i = PickerInput::default();
    match c {
        'c' => i.cross = true,
        'o' => i.circle = true,
        'L' => i.left = true,
        'R' => i.right = true,
        'U' => i.up = true,
        'D' => i.down = true,
        _ => {}
    }
    i
}

pub(crate) fn cmd_target_pick(kind: &str, actor: u8, script: &str) -> Result<()> {
    use legaia_engine_core::target_picker::{
        PickerEvent, SlotState, TargetKind, TargetPickerSession,
    };
    let kind = match kind.to_ascii_lowercase().as_str() {
        "enemy" => TargetKind::SingleEnemy,
        "ally" => TargetKind::SingleAlly,
        "ally-or-self" => TargetKind::SingleAllyOrSelf,
        "dead-ally" => TargetKind::DeadAlly,
        "any-ally" => TargetKind::AnyAlly,
        "all-enemies" => TargetKind::AllEnemies,
        "all-allies" => TargetKind::AllAllies,
        "self" => TargetKind::Self_,
        other => anyhow::bail!("unknown target kind: {other}"),
    };
    let party = [SlotState::alive(true, true); 3];
    let monsters = [SlotState::alive(true, true); 5];
    let mut s = TargetPickerSession::new(kind, actor, party, monsters);
    println!("target-pick: kind={:?} actor={actor}", s.kind());
    for ch in script.chars() {
        if s.is_done() {
            break;
        }
        s.input(picker_input_for(ch));
        for e in s.drain_events() {
            match e {
                PickerEvent::CursorMoved { row, slot } => {
                    println!("  cursor → {:?} slot {slot}", row)
                }
                PickerEvent::RowSwitched { row, slot } => {
                    println!("  row switched → {:?} slot {slot}", row)
                }
                PickerEvent::Confirmed { row, slot } => {
                    println!("  confirmed {:?} slot {slot}", row)
                }
                PickerEvent::SweepConfirmed { row } => {
                    println!("  sweep confirmed {:?}", row)
                }
                PickerEvent::Cancelled => println!("  cancelled"),
                PickerEvent::InvalidConfirm => println!("  invalid confirm"),
            }
        }
    }
    println!("target-pick: outcome = {:?}", s.outcome());
    Ok(())
}

fn editor_input_for(c: char) -> legaia_engine_core::tactical_arts_editor::EditInput {
    use legaia_engine_core::tactical_arts_editor::EditInput;
    let mut i = EditInput::default();
    match c {
        'L' => i.left = true,
        'R' => i.right = true,
        'U' => i.up = true,
        'D' => i.down = true,
        'c' => i.cross = true,
        'o' => i.circle = true,
        't' => i.triangle = true,
        'n' => i.name_next = true,
        _ => {}
    }
    i
}

pub(crate) fn cmd_chain_editor(char_slot: u8, script: &str) -> Result<()> {
    use legaia_engine_core::tactical_arts_editor::{ChainEditor, ChainLibrary, EditEvent};
    let lib = ChainLibrary::new();
    let mut ed = ChainEditor::new(char_slot, &lib);
    println!("chain-editor: char_slot={char_slot}");
    for ch in script.chars() {
        if ed.is_done() {
            break;
        }
        for e in ed.tick(editor_input_for(ch)) {
            match e {
                EditEvent::BrowseCursorMoved { row } => println!("  cursor → row {row}"),
                EditEvent::EnteredEdit { editing_slot } => {
                    println!("  entered edit slot={:?}", editing_slot)
                }
                EditEvent::SequenceAppended { command, len } => {
                    println!("  appended {:?} (len={len})", command)
                }
                EditEvent::SequencePopped { len } => println!("  popped (len={len})"),
                EditEvent::InvalidCommit { len } => println!("  invalid commit at len {len}"),
                EditEvent::EnteredNaming => println!("  entered naming"),
                EditEvent::Saved { slot } => println!("  saved slot {slot}"),
                EditEvent::Replaced { slot } => println!("  replaced slot {slot}"),
                EditEvent::Deleted { slot } => println!("  deleted slot {slot}"),
                EditEvent::Cancelled => println!("  cancelled"),
            }
        }
    }
    println!("chain-editor: outcome = {:?}", ed.outcome());
    Ok(())
}

pub(crate) fn cmd_seru_capture(seru: u16, count: u32, party: &str) -> Result<()> {
    use legaia_engine_core::seru_learning::{SeruCaptureLog, SeruRegistry, record_capture};
    let registry = SeruRegistry::retail();
    let party: Vec<u8> = party
        .split(',')
        .filter_map(|s| s.trim().parse::<u8>().ok())
        .collect();
    let mut log = SeruCaptureLog::new();
    println!("seru-capture: seru={seru} count={count} party={:?}", party);
    for i in 0..count {
        let out = record_capture(&registry, &mut log, seru, &party);
        if !out.accepted {
            println!("  capture {i}: rejected (unknown seru)");
            return Ok(());
        }
        if !out.learns.is_empty() {
            for ev in &out.learns {
                println!(
                    "  capture {i}: char {} learned spell {:#04x} from seru {}",
                    ev.char_slot, ev.spell_id, ev.seru_id
                );
            }
        }
    }
    println!(
        "seru-capture: final per-char totals: {:?}",
        party
            .iter()
            .map(|c| (*c, log.total_points(*c)))
            .collect::<Vec<_>>()
    );
    for c in &party {
        println!("  char {c} learned spells: {:?}", log.learned_spells(*c));
    }
    Ok(())
}
