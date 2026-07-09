//! MAN-script + mode/audio/pcm trace oracles (`man-scripts`, `mode-trace`, `audio-trace`, `pcm-trace`).
//!
//! Mechanical split from `commands.rs` (behavior-preserving).

use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_man_scripts(
    scene_name: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    all: bool,
    disasm_record: Option<usize>,
    disasm_partition: usize,
    dump_man: Option<&Path>,
    variant: Option<u32>,
    gflag_partition: Option<usize>,
    narration: bool,
    system_flag_census: bool,
    motion_flag_census: bool,
    op49_window_census: bool,
    p2_gates: bool,
) -> Result<()> {
    use legaia_engine_core::man_field_scripts::{
        FlagBank, partition_record_span, scene_man_carriers,
        system_flag_census as run_system_flag_census, walk_partition_gflag_sites,
        walk_partition1_scripts,
    };
    use legaia_engine_vm::field_disasm::FlagKind;

    let index = open_index_from_args(extracted_root, disc)?;
    let scene =
        Scene::load(&index, scene_name).with_context(|| format!("load scene '{scene_name}'"))?;
    let carriers = scene_man_carriers(&index, &scene);
    let carrier = match variant {
        Some(idx) => carriers
            .iter()
            .find(|c| c.is_variant() && c.entry_idx == idx)
            .with_context(|| {
                let have: Vec<u32> = carriers
                    .iter()
                    .filter(|c| c.is_variant())
                    .map(|c| c.entry_idx)
                    .collect();
                format!(
                    "scene '{scene_name}' has no variant MAN at PROT[{idx}] (variants: {have:?})"
                )
            })?,
        None => carriers.first().with_context(|| {
            format!("scene '{scene_name}' has no scene_asset_table bundle (no MAN)")
        })?,
    };
    let entry_idx = carrier.entry_idx;
    let man = carrier.payload.clone();
    let man_file = legaia_asset::man_section::parse(&man)?;
    if carrier.is_variant() {
        println!(
            "using VARIANT MAN from PROT[{entry_idx}] (chunk offset 0x{:X})",
            carrier.chunk_offset.unwrap_or(0),
        );
    }

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
        entry_idx,
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

    if p2_gates {
        use legaia_engine_core::man_field_scripts::{
            partition2_record_gates, partition2_record_name,
        };
        let n2 = *man_file.header.partition_counts.get(2).unwrap_or(&0) as usize;
        println!("\n--- partition-2 record C1/C2 header gates ({n2} records) ---");
        for i in 0..n2 {
            let name = partition2_record_name(&man_file, &man, i)
                .map(|b| {
                    b.iter()
                        .map(|&c| {
                            if c.is_ascii_graphic() || c == b' ' {
                                c as char
                            } else {
                                '.'
                            }
                        })
                        .collect::<String>()
                })
                .unwrap_or_default();
            match partition2_record_gates(&man_file, &man, i) {
                Some((c1, c2)) => {
                    let fmt = |v: &[u16]| {
                        v.iter()
                            .map(|f| format!("0x{f:03X}"))
                            .collect::<Vec<_>>()
                            .join(",")
                    };
                    println!(
                        "  P2[{i:3}] C1=[{}] C2=[{}]  name={name:?}",
                        fmt(&c1),
                        fmt(&c2),
                    );
                }
                None => println!("  P2[{i:3}] <header overruns record body>  name={name:?}"),
            }
        }
    }

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
            "\n--- flag writes/tests in partition {partition} ({} sites) ---",
            sites.len(),
        );
        for s in &sites {
            let bank = match s.bank {
                FlagBank::Scratchpad => "SCRATCH",
                FlagBank::System => "SYSTEM ",
            };
            let kind = match s.kind {
                FlagKind::Set => "Set  ",
                FlagKind::Clear => "Clear",
                FlagKind::Test => "Test ",
            };
            println!(
                "  P{}[{}] {bank} {kind} flag=0x{:04X} ({:>5}) @ 0x{:05X} (op 0x{:02X})",
                s.partition, s.record, s.flag, s.flag, s.abs_pc, s.opcode,
            );
        }
    }

    if system_flag_census {
        let scenes = index.cdname_scene_names();
        let census = run_system_flag_census(&index, &scenes);
        println!(
            "\n--- disc-wide SYSTEM-flag census ({} scenes scanned, {} flags with sites) ---",
            scenes.len(),
            census.len(),
        );
        for (flag, hits) in &census {
            println!("flag 0x{flag:04X} ({flag:>5}): {} site(s)", hits.len());
            for h in hits {
                let kind = match h.kind {
                    FlagKind::Set => "Set  ",
                    FlagKind::Clear => "Clear",
                    FlagKind::Test => "Test ",
                };
                println!(
                    "    {kind} scene={:<10} PROT[{:04}]{} P{}[{}] (op 0x{:02X}){}",
                    h.scene_name,
                    h.entry_idx,
                    if h.variant { " VARIANT-MAN" } else { "" },
                    h.partition,
                    h.record,
                    h.opcode,
                    if h.clean { "" } else { "  DESYNCED?" },
                );
            }
        }
    }

    if motion_flag_census {
        use legaia_engine_core::man_field_scripts::motion_flag_census as run_motion_flag_census;
        let scenes = index.cdname_scene_names();
        let census = run_motion_flag_census(&index, &scenes);
        let total: usize = census.values().map(Vec::len).sum();
        println!(
            "\n--- disc-wide MOTION-VM flag census ({} scenes scanned, {} flags, {} sites) ---",
            scenes.len(),
            census.len(),
            total,
        );
        for (flag, hits) in &census {
            println!("flag 0x{flag:04X} ({flag:>5}): {} site(s)", hits.len());
            for h in hits {
                use legaia_asset::man_motion::MotionFlagKind;
                let kind = match h.site.kind {
                    MotionFlagKind::Set => "Set  ",
                    MotionFlagKind::Clear => "Clear",
                };
                let gate = match h.site.gate {
                    Some(g) => format!("gate=0x{g:03X}"),
                    None => "default".to_string(),
                };
                let binds: Vec<String> = h
                    .bindings
                    .iter()
                    .map(|b| format!("0x{:02X}", b.actor_id))
                    .collect();
                println!(
                    "    {kind} scene={:<10} PROT[{:04}]{} rec{} var{} ({gate}) actors=[{}] @0x{:05X}",
                    h.scene_name,
                    h.entry_idx,
                    if h.carrier_variant {
                        " VARIANT-MAN"
                    } else {
                        ""
                    },
                    h.site.record,
                    h.site.variant,
                    binds.join(","),
                    h.site.offset,
                );
            }
        }
    }

    if op49_window_census {
        use legaia_engine_core::man_field_scripts::op49_window_census as run_op49_window_census;
        let scenes = index.cdname_scene_names();
        let sites = run_op49_window_census(&index, &scenes);
        println!(
            "\n--- disc-wide op-0x49 flag-WINDOW census ({} scenes scanned, {} sites) ---",
            scenes.len(),
            sites.len(),
        );
        for s in &sites {
            let window = match s.window() {
                Some((lo, hi)) => format!("[0x{lo:04X}..0x{hi:04X}]"),
                None => "(empty)".to_string(),
            };
            println!(
                "  scene={:<10}{} P{}[{:3}] @0x{:05X} sub=0x{:02X} base=0x{:04X} count={:3} default={:3} rows={:3} window={window}{}",
                s.scene_name,
                if s.variant { " VARIANT-MAN" } else { "" },
                s.partition,
                s.record,
                s.abs_pc,
                s.sub_op,
                s.base_flag,
                s.count,
                s.default_index,
                s.rows,
                if s.in_footprint {
                    ""
                } else {
                    "  [past-footprint]"
                },
            );
        }
        // Spine-flag verdicts: containment + near-miss (+/-8) per target.
        const TARGETS: [u16; 4] = [0x142, 0x482, 0x1BE, 0x225];
        const MARGIN: u32 = 8;
        for target in TARGETS {
            let contained = sites.iter().filter(|s| s.covers(target)).count();
            let near: Vec<&legaia_engine_core::man_field_scripts::Op49WindowSite> = sites
                .iter()
                .filter(|s| !s.covers(target) && s.min_distance(target) <= MARGIN)
                .collect();
            let nearest = sites.iter().map(|s| s.min_distance(target)).min();
            println!(
                "target 0x{target:04X} ({target:>4}): contained by {contained} site(s), {} near-miss(es) within +/-{MARGIN}, nearest distance {:?}",
                near.len(),
                nearest,
            );
            for s in near {
                println!(
                    "    near-miss scene={} P{}[{}] @0x{:05X} sub=0x{:02X} base=0x{:04X} count={} (distance {})",
                    s.scene_name,
                    s.partition,
                    s.record,
                    s.abs_pc,
                    s.sub_op,
                    s.base_flag,
                    s.count,
                    s.min_distance(target),
                );
            }
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
        let Some(last) = trace.last() else {
            anyhow::bail!("engine trace is empty (need at least one frame)");
        };
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
            let Some(last) = trace.last() else {
                anyhow::bail!("engine trace is empty (need at least one frame)");
            };
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
