//! Running the whole corpus: per-state seed, run, score and image capture,
//! and the corpus-level summary.

use super::*;

/// Corpus-level numbers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CorpusSummary {
    pub states: usize,
    pub seeded: usize,
    pub seed_failed: usize,
    /// Class -> count, over every state.
    pub classes: BTreeMap<String, usize>,
    /// Channel -> `(mean score, states measured)` over the seeded states
    /// made on a retail disc. This is the headline.
    pub channels: BTreeMap<String, (f64, usize)>,
    /// Mean of per-state scores over the seeded retail-disc states.
    pub mean_state_score: f64,
    /// Seeded states made on a patched disc (`resident_patch`).
    pub patched: usize,
    /// The same channel means over the patched-disc states, kept apart: a
    /// difference there may be the patch's, not the engine's.
    pub patched_channels: BTreeMap<String, (f64, usize)>,
    /// Mean of per-state scores over the seeded patched-disc states.
    pub patched_mean_state_score: f64,
}

pub fn summarise(reports: &[StateReport]) -> CorpusSummary {
    let mut s = CorpusSummary {
        states: reports.len(),
        ..Default::default()
    };
    let mut sums: BTreeMap<String, (f64, usize)> = BTreeMap::new();
    let mut state_sum = 0.0;
    let mut retail_scored = 0usize;
    let mut patched_sums: BTreeMap<String, (f64, usize)> = BTreeMap::new();
    let mut patched_state_sum = 0.0;
    for r in reports {
        *s.classes
            .entry(
                serde_json::to_value(r.class)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default(),
            )
            .or_default() += 1;
        if r.class.seedable() {
            if r.unseeded.is_empty() {
                s.seeded += 1;
            } else if r.unseeded.starts_with(BATTLE_NOT_SEEDABLE)
                || r.unseeded.starts_with(MENU_NOT_SEEDABLE)
            {
                // A battle capture whose RAM names why it cannot be seeded
                // (the fight still loading, an empty cell) is a classified
                // instrument limit, not a failure of the seeding path.
            } else {
                s.seed_failed += 1;
            }
        }
        if let Some(sc) = r.score {
            let (sums, state_sum) = if r.resident_patch.is_some() {
                s.patched += 1;
                (&mut patched_sums, &mut patched_state_sum)
            } else {
                retail_scored += 1;
                (&mut sums, &mut state_sum)
            };
            *state_sum += sc;
            for (k, v) in &r.channels {
                let e = sums.entry(k.clone()).or_default();
                e.0 += v;
                e.1 += 1;
            }
        }
    }
    let means = |sums: BTreeMap<String, (f64, usize)>| -> BTreeMap<String, (f64, usize)> {
        sums.into_iter()
            .map(|(k, (sum, n))| (k, (round3(sum / n as f64), n)))
            .collect()
    };
    let mean = |sum: f64, n: usize| if n == 0 { 0.0 } else { round3(sum / n as f64) };
    s.channels = means(sums);
    s.patched_channels = means(patched_sums);
    s.mean_state_score = mean(state_sum, retail_scored);
    s.patched_mean_state_score = mean(patched_state_sum, s.patched);
    s
}

/// Options for one corpus run.
pub struct RunOptions<'a> {
    pub extracted: &'a Path,
    pub library: &'a Path,
    pub manifest: &'a ScenarioManifest,
    /// When set, render each seeded state through `play-window` with this
    /// binary and score the frame.
    pub engine_exe: Option<&'a Path>,
    /// Where side-by-side PNGs and engine captures go (gitignored).
    pub out_dir: Option<&'a Path>,
    /// Only states whose label contains this substring; a comma separates
    /// several alternatives (any one matching keeps the state).
    pub filter: Option<&'a str>,
    /// Save application order (see [`SeedOrder`]).
    pub order: SeedOrder,
}

/// Run the whole corpus.
pub fn run_corpus(opts: &RunOptions<'_>) -> Result<Vec<StateReport>> {
    let scus = std::fs::read(opts.extracted.join("SCUS_942.54"))
        .with_context(|| format!("read {}/SCUS_942.54", opts.extracted.display()))?;
    let cdname = legaia_prot::cdname::parse(&opts.extracted.join("CDNAME.TXT"))?;
    let entries = enumerate_corpus(opts.manifest, opts.library);
    let mut out = Vec::new();
    for entry in entries {
        if let Some(f) = opts.filter
            && !f
                .split(',')
                .filter(|alt| !alt.is_empty())
                .any(|alt| entry.label.contains(alt))
        {
            continue;
        }
        // A state made on a known patched image runs the engine on that
        // image's files (`resident_disc`); one whose image is not on this
        // machine runs on the retail disc and says so.
        let resident = entry.resident_disc.as_deref().map(|name| {
            let dirs = super::resident_disc::resident_disc_dirs();
            let image = super::resident_disc::locate_resident_disc(name, &dirs)
                .with_context(|| format!("{name} not found in {dirs:?}"))?;
            let cache = opts
                .out_dir
                .map(|d| d.join("resident-discs"))
                .context("no report directory to extract into")?;
            let root = super::resident_disc::extract_resident_disc(&image, &cache)?;
            let scus = std::fs::read(root.join("SCUS_942.54"))?;
            let cdname = legaia_prot::cdname::parse(&root.join("CDNAME.TXT"))?;
            anyhow::Ok((root, scus, cdname))
        });
        let mut report = match &resident {
            Some(Ok((root, scus, cdname))) => {
                let opts = RunOptions {
                    extracted: root,
                    ..*opts
                };
                run_one(&opts, &entry, scus, cdname)
            }
            _ => run_one(opts, &entry, &scus, &cdname),
        };
        if let (Some(name), Some(r)) = (&entry.resident_disc, &resident) {
            report.detail.insert(
                "resident_disc".into(),
                match r {
                    Ok(_) => format!("engine ran the capture's own image {name}"),
                    Err(e) => {
                        report.resident_disc_missing = Some(format!("{e:#}"));
                        format!("engine ran the retail disc: {e:#}")
                    }
                },
            );
        }
        out.push(report);
    }
    Ok(out)
}

pub(super) fn run_one(
    opts: &RunOptions<'_>,
    entry: &CorpusEntry,
    scus: &[u8],
    cdname: &legaia_prot::cdname::IndexMap,
) -> StateReport {
    let mut report = StateReport {
        label: entry.label.clone(),
        emulator: entry.emulator.to_string(),
        fingerprint: entry.fingerprint.chars().take(16).collect(),
        scene: String::new(),
        game_mode: 0,
        class: StateClass::Other,
        unseeded: String::new(),
        channels: BTreeMap::new(),
        detail: BTreeMap::new(),
        score: None,
        image: None,
        resident_patch: entry.resident_patch.clone(),
        resident_disc_missing: None,
    };
    let mut retail = match read_retail(entry, scus) {
        Ok(r) => r,
        Err(e) => {
            report.unseeded = format!("unreadable state: {e:#}");
            return report;
        }
    };
    retail.settle_on_loaded_scene(cdname);
    if let Some(pending) = &retail.pending_scene {
        report.detail.insert(
            "pending_scene".into(),
            format!(
                "label reads {pending} but define {} ({}) is loaded; scored as {}",
                retail.loaded_define, retail.scene, retail.scene
            ),
        );
    }
    report.scene = retail.scene.clone();
    report.game_mode = retail.game_mode;
    report.class = retail.class;
    if !retail.class.seedable() {
        report.unseeded = retail.class.unseeded_reason().to_string();
        return report;
    }
    if retail.class == StateClass::Battle {
        if retail.seat_battle_on_region(opts.extracted)
            && let Some(seat) = retail.save.as_ref().and_then(|s| s.ext_v2.field_position)
        {
            report.detail.insert(
                "seat".into(),
                format!(
                    "no position snapshot; seated at {seat:?}, the tile nearest the entry \
                     operand {:?} whose regions read mask {:#X}",
                    retail.entry_operand, retail.region_mask
                ),
            );
        }
        run_battle(opts, entry, &retail, &mut report);
        return report;
    }
    if let Some(Err(why)) = &retail.menu {
        report.unseeded = format!("{MENU_NOT_SEEDABLE}{why}");
        return report;
    }
    let engine = match run_engine_with(opts.extracted, &retail, opts.order) {
        Ok(e) => e,
        Err(e) => {
            report.unseeded = format!("seeding failed: {e:#}");
            return report;
        }
    };
    let image = match (
        opts.engine_exe,
        &retail.frame,
        retail.player,
        retail.seed_save().as_ref(),
    ) {
        (Some(exe), Some(rf), seat, Some(save))
            if matches!(retail.menu, Some(Ok(_)))
                && crate::retail_compare_image::luma(rf)
                    >= crate::retail_compare_image::DARK_LUMA =>
        {
            let Some(Ok(menu)) = &retail.menu else {
                unreachable!()
            };
            menu_image(opts, exe, entry, &retail, menu, seat, save, rf, &mut report)
        }
        (Some(_), Some(rf), _, _)
            if crate::retail_compare_image::luma(rf) < crate::retail_compare_image::DARK_LUMA =>
        {
            report.detail.insert(
                "image".into(),
                "not scored: retail frame is a fade (near-black)".into(),
            );
            None
        }
        (Some(exe), Some(rf), Some([x, _, z]), Some(save)) => {
            // A capture the headless seed reached by its script phase is
            // framed at that phase too: the child runs the same gate and
            // captures the frame it holds, with the deadline as its bound.
            let gate = engine.script.filter(|p| p.met_at.is_some()).and(
                crate::retail_compare_script::displayed_script_gate_from_retail(&retail.scripts),
            );
            let frame = match gate {
                Some(g) => {
                    let mut env = vec![("LEGAIA_SCRIPT_GATE", g.to_env())];
                    // A gate on the player's clip latch is met on the RAM's
                    // cursor, which has no wait to take the lag off: the
                    // child takes the frame `display_lag` ticks back, while
                    // the record it replays already ran.
                    if g.latch.is_some() {
                        env.push((
                            "LEGAIA_CAPTURE_DISPLAY_LAG",
                            retail.scripts.display_lag.to_string(),
                        ));
                    }
                    env.extend(retail.seat_env(opts.out_dir, &entry.label));
                    if let Some(n) = retail.hud_countdown {
                        env.push(("LEGAIA_HUD_COUNTDOWN", n.to_string()));
                    }
                    if let Some(b) = &retail.camera_block {
                        env.push(("LEGAIA_SEAT_CAMERA_BLOCK", camera_block_env(b)));
                    }
                    if let Some([fx, fz]) = retail.seat_focus {
                        env.push(("LEGAIA_SEAT_FOCUS", format!("{fx},{fz}")));
                    }
                    crate::retail_compare_image::engine_frame_with(
                        exe,
                        opts.extracted,
                        &retail.scene,
                        Some((x, z)),
                        &[],
                        &env,
                        legaia_engine_capture::script_gate::SCRIPT_GATE_DEADLINE,
                        opts.out_dir,
                        &entry.label,
                        crate::retail_compare_image::FrameEntry::Resume(save),
                    )
                }
                None => crate::retail_compare_image::engine_frame(
                    exe,
                    opts.extracted,
                    &retail.scene,
                    x,
                    z,
                    opts.out_dir,
                    &entry.label,
                    save,
                    retail.hud_countdown,
                    retail.camera_block.as_ref(),
                    retail.seat_focus,
                    &retail.seat_env(opts.out_dir, &entry.label),
                ),
            };
            match frame {
                Ok(ef) => {
                    let score = crate::retail_compare_image::score(rf, &ef);
                    if let Some(dir) = opts.out_dir {
                        let _ = crate::retail_compare_image::write_side_by_side(
                            &dir.join(format!("{}.png", entry.label)),
                            rf,
                            &ef,
                        );
                    }
                    Some(score)
                }
                Err(e) => {
                    report
                        .detail
                        .insert("image".into(), format!("engine frame failed: {e:#}"));
                    None
                }
            }
        }
        _ => None,
    };
    let (ch, det) = compare(&retail, &engine, image.clone());
    report.detail.extend(det);
    report.score = state_score(&ch);
    report.channels = ch;
    report.image = image;
    report
}

/// The pause-menu frame through `play-window`: the card-load resume and seat
/// a field state takes, then the same pad edges the headless drive presses
/// ([`pause_menu_presses`]) as a `--pad-script`, captured once the drive has
/// settled.
#[allow(clippy::too_many_arguments)]
pub(super) fn menu_image(
    opts: &RunOptions<'_>,
    exe: &Path,
    entry: &CorpusEntry,
    retail: &RetailObs,
    menu: &RetailMenu,
    seat: Option<[i16; 3]>,
    save: &legaia_save::SaveFile,
    rf: &Frame,
    report: &mut StateReport,
) -> Option<ImageScore> {
    let first = crate::retail_compare_image::CAPTURE_TICK;
    let script = pause_menu_presses(menu)
        .iter()
        .map(|(t, b)| format!("{}:{b:?}", first + t))
        .collect::<Vec<_>>()
        .join(",");
    let extra = vec!["--pad-script".to_string(), script];
    match crate::retail_compare_image::engine_frame_with(
        exe,
        opts.extracted,
        &retail.scene,
        seat.map(|[x, _, z]| (x, z)),
        &extra,
        &[],
        first + pause_menu_drive_ticks(menu),
        opts.out_dir,
        &entry.label,
        crate::retail_compare_image::FrameEntry::Resume(save),
    ) {
        Ok(ef) => {
            if let Some(dir) = opts.out_dir {
                let _ = crate::retail_compare_image::write_side_by_side(
                    &dir.join(format!("{}.png", entry.label)),
                    rf,
                    &ef,
                );
            }
            Some(crate::retail_compare_image::score(rf, &ef))
        }
        Err(e) => {
            report
                .detail
                .insert("image".into(), format!("engine frame failed: {e:#}"));
            None
        }
    }
}
