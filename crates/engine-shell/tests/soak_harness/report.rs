//! The soak report: findings grouped by signature, known-versus-new, and the
//! files written to the output directory.

use super::*;

/// `(rank, signature, [(outcome index, finding index)])`.
pub(super) type SigRow = (u8, String, Vec<(usize, usize)>);

/// Findings an `@<save>` run raises only because it enters a scene by name
/// that the save's story reaches through a door it never opens (see
/// `docs/tooling/soak-harness.md`). A signature here whose every hit comes
/// from an `@<save>` run is listed apart from the findings, with its reason;
/// the same signature from a plain run is still a finding. An entry ending in
/// `*` matches by prefix: `dream` plays one of several vignettes by the
/// save's flags, and each leaves the party on its own tile.
pub(super) const SAVE_ENTRY_ARTIFACTS: &[(&str, &str)] = &[
    (
        "softlock|map01|free-roam:immobile@tile(96,25)",
        "map01 P1[0] walls in Rim Elm's footprint from PRO-04 on; the town0b / town0c / op* exits land inside it",
    ),
    (
        "softlock|korb3|free-roam:immobile@tile(36,26)",
        "korb3's default entry seat is walled once the save's flags gate off the arrival cutscene that moves the party off it",
    ),
    (
        "confined|dream|*",
        "dream's closing record returns the party to the scene it fell asleep in by a flag the sleep sets; entered by name, no branch matches and the record ends on the hidden party",
    ),
    (
        "softlock|dream|free-roam:immobile*",
        "dream's closing record returns the party to the scene it fell asleep in by a flag the sleep sets; entered by name, no branch matches and the record ends on the hidden party",
    ),
    (
        "softlock|other7|helper@0x00d9:op49",
        "other7's field MAN is a draft no retail path runs; its `49 0B` opens the Incense notice, whose close clears _DAT_8007B450 (0x801F1F20), so op 0x49 re-arms on the next frame, as in retail",
    ),
    (
        "timeline_cap|other7|helper@0xd9:49",
        "other7's field MAN is a draft no retail path runs; its `49 0B` Incense notice re-arms itself in retail too, and the anti-hang cap ends it before the softlock window does when the window is longer",
    ),
    (
        "timeline_cap|other7|helper@0x389:49",
        "other7's field MAN is a draft no retail path runs; its `49 0E` parks for good in retail too, and the anti-hang cap ends it before the softlock window does when the window is longer",
    ),
    (
        "softlock|other7|helper@0x0389:op49",
        "other7's field MAN is a draft no retail path runs; its `49 0E` is past op 0x49's sub-op bound, and the Idle arm returns the same PC (0x801E0990), as in retail",
    ),
];

/// Scenes the story only ever enters through another scene's hand-off, never
/// by name: an entry artifact of one of these is an artifact on **every** run
/// that starts there, `@<save>` or not. `dream` is reached from a sleep that
/// sets the return flag its closing record branches on; its vignette trigger
/// fires for the New Game party too (the `0x35B..0x35E` tests fall through to
/// the default record), and that run's closing record ends on the hidden
/// party just as a save's does.
const HAND_OFF_ONLY_SCENES: &[&str] = &["dream"];

/// Scene blocks whose field MAN no retail path runs, so every run that starts
/// in one enters a field the game never plays. `other7` is an earlier
/// revision of `koin3`'s MAN (`docs/formats/cdname.md`); the dance minigame
/// loads the block's map, trigger sidecar and art pack, never its MAN as a
/// field.
const NEVER_RUN_FIELD_SCENES: &[&str] = &["other7"];

/// Whether a hit from the run labelled `label` counts towards an entry
/// artifact on `sig`: an `@<save>` run, or any run that starts in a
/// hand-off-only scene the signature names.
fn artifact_run(sig: &str, label: &str) -> bool {
    if label.contains('@') {
        return true;
    }
    let start = label.split(['+', '@']).next().unwrap_or(label);
    let sig_scene = sig.split('|').nth(1).unwrap_or("");
    (HAND_OFF_ONLY_SCENES.contains(&start) || NEVER_RUN_FIELD_SCENES.contains(&start))
        && start == sig_scene
}

/// The reason `sig` is a known save-entry artifact, if it is one.
fn entry_artifact(sig: &str) -> Option<&'static str> {
    SAVE_ENTRY_ARTIFACTS
        .iter()
        .find(|(s, _)| match s.strip_suffix('*') {
            Some(prefix) => sig.starts_with(prefix),
            None => *s == sig,
        })
        .map(|&(_, why)| why)
}

pub(super) fn write_report(
    src: &Source,
    known: &BTreeSet<String>,
    cfg: &SoakConfig,
    r: &SoakResult,
) {
    let out = out_dir(&cfg.tag);
    let _ = std::fs::create_dir_all(out.join("replays"));
    // Group by signature.
    let mut groups: BTreeMap<String, Vec<(usize, usize)>> = BTreeMap::new();
    for (oi, o) in r.outcomes.iter().enumerate() {
        for (fi, f) in o.findings.iter().enumerate() {
            groups.entry(f.signature()).or_default().push((oi, fi));
        }
    }
    let mut rows: Vec<SigRow> = groups
        .into_iter()
        .map(|(sig, v)| {
            let f = &r.outcomes[v[0].0].findings[v[0].1];
            (f.rank(), sig, v)
        })
        .collect();
    rows.sort_by(|a, b| {
        (a.0, std::cmp::Reverse(a.2.len())).cmp(&(b.0, std::cmp::Reverse(b.2.len())))
    });
    // Known `@<save>` entry artifacts come out of the table.
    let (artifacts, rows): (Vec<SigRow>, Vec<SigRow>) =
        rows.into_iter().partition(|(_, sig, hits)| {
            entry_artifact(sig).is_some()
                && hits
                    .iter()
                    .all(|&(oi, _)| artifact_run(sig, &r.outcomes[oi].spec.scene))
        });

    let frames_total: u64 = r.outcomes.iter().map(|o| o.stats.frames_run).sum();
    let battles: u32 = r.outcomes.iter().map(|o| o.stats.battles).sum();
    let wins: u32 = r.outcomes.iter().map(|o| o.stats.battles_won).sum();
    let wipes = r.outcomes.iter().filter(|o| o.stats.wiped).count();
    let continues: u32 = r.outcomes.iter().map(|o| o.stats.wipe_continues).sum();
    let menus: u32 = r.outcomes.iter().map(|o| o.stats.menu_opens).sum();
    let fmvs: u32 = r.outcomes.iter().map(|o| o.stats.fmvs).sum();
    let enter_fail = r.outcomes.iter().filter(|o| !o.enter_ok).count();
    let saves: u32 = r.outcomes.iter().map(|o| o.stats.save_commits).sum();
    let trips: u32 = r.outcomes.iter().map(|o| o.stats.round_trips).sum();
    let shops: u32 = r.outcomes.iter().map(|o| o.stats.shop_opens).sum();
    let menu_rows: BTreeSet<&String> = r
        .outcomes
        .iter()
        .flat_map(|o| o.stats.menu_rows.iter())
        .collect();
    let mut modes: BTreeMap<String, usize> = BTreeMap::new();
    for o in &r.outcomes {
        for m in &o.stats.modes {
            *modes.entry(m.clone()).or_default() += 1;
        }
    }

    let mut md = String::new();
    let _ = writeln!(md, "# Soak report `{}`\n", cfg.tag);
    let _ = writeln!(
        md,
        "- runs: {} ({} scenes x {} seeds x {} frames), {} frames ticked, wall {:.0}s, {} jobs",
        r.outcomes.len(),
        cfg.scenes.len(),
        cfg.seeds.len(),
        cfg.frames,
        frames_total,
        r.wall.as_secs_f32(),
        cfg.jobs
    );
    let _ = writeln!(
        md,
        "- battles entered {battles}, left to field {wins}, party wipes {wipes} (continued from the card {continues}), menu opens {menus}, FMVs skipped {fmvs}, entry failures {enter_fail}"
    );
    let _ = writeln!(
        md,
        "- save-screen commits {saves}, save / load round trips {trips}, shops opened {shops}"
    );
    let talks: u32 = r.outcomes.iter().map(|o| o.stats.talk_visits).sum();
    let seats: u32 = r.outcomes.iter().map(|o| o.stats.talk_seats).sum();
    let opens: u32 = r.outcomes.iter().map(|o| o.stats.talk_opens).sum();
    let props: u32 = r.outcomes.iter().map(|o| o.stats.prop_seats).sum();
    let _ = writeln!(
        md,
        "- talk visits {talks}, seated at a target {seats} ({props} at a prop), conversations opened {opens}"
    );
    let _ = writeln!(md, "- pause-menu sub-screens entered: {menu_rows:?}");
    let _ = writeln!(md, "- runs that reached each mode: {modes:?}");
    let mut mode_frames: BTreeMap<&String, u64> = BTreeMap::new();
    for (m, n) in r.outcomes.iter().flat_map(|o| o.stats.mode_frames.iter()) {
        *mode_frames.entry(m).or_default() += n;
    }
    let mut surfaces: BTreeMap<&String, u32> = BTreeMap::new();
    for name in r
        .outcomes
        .iter()
        .flat_map(|o| o.stats.battle_surfaces.iter())
    {
        *surfaces.entry(name).or_default() += 1;
    }
    let _ = writeln!(md, "- runs that reached each battle surface: {surfaces:?}");
    let revisits: u32 = r.outcomes.iter().map(|o| o.stats.minigame_revisits).sum();
    let _ = writeln!(
        md,
        "- frames ticked in each mode: {mode_frames:?}; minigame re-entries {revisits}"
    );
    let moved = r
        .outcomes
        .iter()
        .filter(|o| !o.stats.scenes_entered.is_empty())
        .count();
    let reached: BTreeSet<&String> = r
        .outcomes
        .iter()
        .flat_map(|o| o.stats.scenes_entered.iter())
        .collect();
    let _ = writeln!(
        md,
        "- runs that changed scene {moved}; distinct scenes entered by transition {}",
        reached.len()
    );
    let _ = writeln!(md, "- distinct finding signatures: {}", rows.len());
    for (_, sig, hits) in &artifacts {
        let why = entry_artifact(sig).unwrap_or("");
        let _ = writeln!(
            md,
            "- known entry artifact (not a finding): `{}` x{} - {why}",
            sig.replace('|', "/"),
            hits.len()
        );
    }
    md.push('\n');
    let _ = writeln!(
        md,
        "| rank | detector | scene | location | hits | repro | first (start scene / seed / frame) | detail |"
    );
    let _ = writeln!(md, "|---|---|---|---|---|---|---|---|");
    let max_confirm = env_u64("LEGAIA_SOAK_CONFIRM_MAX").unwrap_or(40) as usize;
    let mut confirmed = 0usize;
    let mut json = String::from("[\n");
    for (rank, sig, hits) in &rows {
        let (oi, fi) = hits[0];
        let o = &r.outcomes[oi];
        let f = &o.findings[fi];
        let (repro, pads, relaxed) = if cfg.confirm && confirmed < max_confirm {
            confirmed += 1;
            let (hit, pads, relaxed) = confirm(src, known, o, f);
            (if hit { "yes" } else { "NO" }.to_string(), pads, relaxed)
        } else {
            (
                "-".to_string(),
                o.pads[..((f.frame + 1) as usize).min(o.pads.len())].to_vec(),
                None,
            )
        };
        let file = out
            .join("replays")
            .join(format!("{}.replay.toml", slug(sig)));
        let _ = std::fs::write(
            &file,
            replay_text(&o.spec, &pads, Some(f), relaxed, o.arming),
        );
        let detail: String = f.detail.chars().take(220).collect();
        let _ = writeln!(
            md,
            "| {} | {} | {} | `{}` | {} | {} | {} / {} / {} | {} |",
            rank,
            f.detector,
            f.scene,
            f.location.replace('|', "/"),
            hits.len(),
            repro,
            o.spec.scene,
            o.spec.seed,
            f.frame,
            detail.replace('|', "/")
        );
        let starts: BTreeSet<&str> = hits
            .iter()
            .map(|&(oi, _)| r.outcomes[oi].spec.scene.as_str())
            .collect();
        // A JSON array: `{:?}` on a set prints `{..}`, which is not JSON.
        let starts: Vec<&str> = starts.into_iter().collect();
        let _ = writeln!(
            json,
            "  {{\"signature\": {:?}, \"rank\": {}, \"hits\": {}, \"repro\": {:?}, \"start_scenes\": {:?}, \"first\": {{\"scene\": {:?}, \"seed\": {}, \"frame\": {}}}, \"mode\": {:?}, \"detail\": {:?}, \"replay\": {:?}}},",
            sig,
            rank,
            hits.len(),
            repro,
            starts,
            o.spec.scene,
            o.spec.seed,
            f.frame,
            f.mode,
            f.detail,
            file.display().to_string()
        );
    }
    if json.ends_with(",\n") {
        json.truncate(json.len() - 2);
        json.push('\n');
    }
    json.push_str("]\n");
    let _ = writeln!(md, "\nReplays: `{}`", out.join("replays").display());
    let _ = std::fs::write(out.join("report.md"), &md);
    let _ = std::fs::write(out.join("findings.json"), &json);
    eprintln!("{md}");
    eprintln!(
        "[soak] report written to {}",
        out.join("report.md").display()
    );
}
