//! The committed ratchet ([`Baseline`]) and the markdown report over a
//! corpus run.

use super::*;

/// The committed ratchet: per-state, per-channel scores. No pixels, no RAM.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Baseline {
    /// Engine ticks between seeding and sampling when the baseline was cut.
    pub settle_ticks: u64,
    /// Label -> channel -> score.
    pub states: BTreeMap<String, BTreeMap<String, f64>>,
    /// Label -> class, for the non-seeded states too (a class change is
    /// itself worth a review).
    pub classes: BTreeMap<String, StateClass>,
}

/// Slack for float round-trips through the JSON baseline.
pub(super) const RATCHET_EPS: f64 = 0.0005;

impl Baseline {
    pub fn from_reports(reports: &[StateReport]) -> Self {
        let mut b = Baseline {
            settle_ticks: SETTLE_TICKS,
            ..Default::default()
        };
        for r in reports {
            b.classes.insert(r.label.clone(), r.class);
            // A patched-disc state run on the retail disc for want of its
            // own image measures the patch's distance, not the engine's:
            // the baseline keeps what its own image scored.
            if !r.channels.is_empty() && r.resident_disc_missing.is_none() {
                b.states.insert(r.label.clone(), r.channels.clone());
            }
        }
        b
    }

    /// Lay this run's measurements over a prior baseline. A state this run
    /// seeded takes this run's channel set: a channel the run no longer
    /// measures on it (a class change, a channel retired) leaves the ratchet
    /// instead of failing every later check as "not measured". The channels
    /// in `unmeasured_by_run` (the image channel on a run without a display)
    /// are carried over, and so is every state outside the run (a
    /// `--filter`) or that the run could not seed.
    pub fn merged_over(self, prior: Option<Baseline>, unmeasured_by_run: &[&str]) -> Self {
        let Some(mut out) = prior else { return self };
        out.settle_ticks = self.settle_ticks;
        out.classes.extend(self.classes);
        for (label, mut chans) in self.states {
            if let Some(old) = out.states.get(&label) {
                for (ch, v) in old {
                    if unmeasured_by_run.contains(&ch.as_str()) {
                        chans.entry(ch.clone()).or_insert(*v);
                    }
                }
            }
            out.states.insert(label, chans);
        }
        out
    }

    /// Every drop against this baseline on a state made on a retail disc.
    /// A channel the run did not measure is skipped when `allow_unmeasured`
    /// names it (the image channel on a run without a display), and is a
    /// failure otherwise. A patched-disc state never fails the ratchet -
    /// see [`Self::patched_drift`].
    pub fn regressions(&self, reports: &[StateReport], allow_unmeasured: &[&str]) -> Vec<String> {
        self.drops(reports, allow_unmeasured, false)
    }

    /// The same drops on the states made on a patched disc
    /// (`resident_patch`). They replay a modified executable, so a drop is
    /// reported for review and is not a regression against retail.
    pub fn patched_drift(&self, reports: &[StateReport], allow_unmeasured: &[&str]) -> Vec<String> {
        self.drops(reports, allow_unmeasured, true)
    }

    /// The baselined states this run measured on the retail disc because
    /// their own patched image was not found ([`StateReport::resident_disc_missing`]),
    /// with the reason. [`Self::drops`] leaves them out: the baseline was cut
    /// on the image, so a comparison would score the patch, and a machine
    /// without the images would read every such state as a drop.
    pub fn fallback_skips(&self, reports: &[StateReport]) -> Vec<String> {
        reports
            .iter()
            .filter(|r| self.states.contains_key(&r.label))
            .filter_map(|r| {
                let why = r.resident_disc_missing.as_deref()?;
                Some(format!(
                    "{}: not compared - its own image was not used ({why}); the engine ran the retail disc",
                    r.label
                ))
            })
            .collect()
    }

    pub(super) fn drops(
        &self,
        reports: &[StateReport],
        allow_unmeasured: &[&str],
        patched: bool,
    ) -> Vec<String> {
        let by_label: BTreeMap<&str, &StateReport> =
            reports.iter().map(|r| (r.label.as_str(), r)).collect();
        let mut out = Vec::new();
        for (label, chans) in &self.states {
            // A state absent from this run is a property of the local
            // library (backups are gitignored and per-machine), not of the
            // engine, so it is reported by the caller rather than failed.
            let Some(r) = by_label.get(label.as_str()) else {
                continue;
            };
            if r.resident_patch.is_some() != patched || r.resident_disc_missing.is_some() {
                continue;
            }
            for (ch, &want) in chans {
                match r.channels.get(ch) {
                    Some(&got) if got + RATCHET_EPS < want => {
                        out.push(format!("{label}.{ch}: {got:.3} < baseline {want:.3}"))
                    }
                    Some(_) => {}
                    None if allow_unmeasured.contains(&ch.as_str()) => {}
                    None => out.push(format!("{label}.{ch}: not measured (baseline {want:.3})")),
                }
            }
        }
        out
    }
}

/// Render the human report as markdown.
pub fn markdown_report(reports: &[StateReport], summary: &CorpusSummary) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(s, "# Retail comparison corpus\n");
    let _ = writeln!(
        s,
        "{} states; {} seeded ({} on a patched disc); {} seed failures; mean state score \
         {:.3} retail, {:.3} patched\n",
        summary.states,
        summary.seeded,
        summary.patched,
        summary.seed_failed,
        summary.mean_state_score,
        summary.patched_mean_state_score
    );
    let missing: Vec<&str> = reports
        .iter()
        .filter(|r| r.resident_disc_missing.is_some())
        .map(|r| r.label.as_str())
        .collect();
    if !missing.is_empty() {
        let _ = writeln!(
            s,
            "**{} patched-disc state(s) ran the retail disc: their own image was not found \
             (`LEGAIA_PATCHED_DISCS`), so they are not compared against the baseline:** {}\n",
            missing.len(),
            missing.join(", ")
        );
    }
    let _ = writeln!(s, "## Channels (mean over seeded retail-disc states)\n");
    let _ = writeln!(s, "| channel | mean | measured |\n|---|---|---|");
    for ch in CHANNELS {
        if let Some((m, n)) = summary.channels.get(*ch) {
            let _ = writeln!(s, "| {ch} | {m:.3} | {n} |");
        }
    }
    if summary.patched > 0 {
        let _ = writeln!(
            s,
            "\n## Channels, patched-disc states (reported, not ratcheted)\n"
        );
        let _ = writeln!(s, "| channel | mean | measured |\n|---|---|---|");
        for ch in CHANNELS {
            if let Some((m, n)) = summary.patched_channels.get(*ch) {
                let _ = writeln!(s, "| {ch} | {m:.3} | {n} |");
            }
        }
    }
    let _ = writeln!(s, "\n## Classes\n");
    for (k, v) in &summary.classes {
        let _ = writeln!(s, "- {k}: {v}");
    }
    let mut ranked: Vec<&StateReport> = reports.iter().filter(|r| r.score.is_some()).collect();
    ranked.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let _ = writeln!(s, "\n## Seeded states, worst first\n");
    for r in ranked {
        let _ = writeln!(
            s,
            "### {} - {:.3} ({} {} 0x{:02X}){}\n",
            r.label,
            r.score.unwrap_or(0.0),
            r.scene,
            r.emulator,
            r.game_mode,
            r.resident_patch
                .as_deref()
                .map(|p| format!(" - patched disc: {p}"))
                .unwrap_or_default()
        );
        if let Some(d) = r.detail.get("resident_disc") {
            let flag = if r.resident_disc_missing.is_some() {
                "**own image missing** - "
            } else {
                ""
            };
            let _ = writeln!(s, "- disc: {flag}{d}");
        }
        for ch in CHANNELS {
            if let Some(v) = r.channels.get(*ch) {
                let d = r.detail.get(*ch).map(String::as_str).unwrap_or("");
                let _ = writeln!(s, "- `{ch}` {v:.3} - {d}");
            }
        }
        if r.channels.contains_key("image") {
            let _ = writeln!(s, "\n![{0}]({0}.png)", r.label);
        }
        let _ = writeln!(s);
    }
    let _ = writeln!(s, "## Not seeded\n");
    for r in reports.iter().filter(|r| r.score.is_none()) {
        let _ = writeln!(
            s,
            "- {} ({} 0x{:02X} {:?}): {}",
            r.label, r.scene, r.game_mode, r.class, r.unseeded
        );
    }
    s
}
