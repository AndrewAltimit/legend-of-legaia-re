//! `j-replay-v1` repro files: writing a finding's reduced replay with its
//! soak header tags, and loading one back.

use super::*;

// ---------------------------------------------------------------------------
// Replay files
// ---------------------------------------------------------------------------

/// The start scene rides in a header comment (`j-replay-v1` has no scene
/// field; `meta.scenario` names a `scripts/scenarios.toml` label and is left
/// unset).
pub(super) const SCENE_TAG: &str = "# soak-scene = ";
pub(super) const SEED_TAG: &str = "# soak-seed = ";
pub(super) const SIG_TAG: &str = "# soak-signature = ";
pub(super) const MIN_DISTINCT_TAG: &str = "# soak-min-distinct = ";
/// The `control_starved` window a replay was recorded under, when it was not
/// the default (`LEGAIA_SOAK_STARVE_FRAMES`): a shorter window is a shorter
/// replay, and the replay must be judged by the window that made it.
pub(super) const STARVE_TAG: &str = "# soak-starve-frames = ";
/// The `battle_endless` threshold a replay was recorded under, when it was
/// not the default (`LEGAIA_SOAK_BATTLE_FRAMES`): a fight the shorter window
/// caught may end before the default one would, so the replay is judged by
/// the window that made it.
pub(super) const BATTLE_FRAMES_TAG: &str = "# soak-battle-frames = ";
/// The session configuration a replay was recorded under: `"host"` (the play
/// hosts' arming, [`Tunables::arming`]) or `"bare"`. A replay without
/// the tag predates the arming and replays bare. Either can carry a
/// `"-retail"` suffix: the run held the hosts' retail mode
/// ([`Arming::retail`]).
pub(super) const ARMING_TAG: &str = "# soak-arming = ";

pub(super) fn replay_text(
    spec: &RunSpec,
    pads: &[u16],
    finding: Option<&Finding>,
    min_distinct: Option<usize>,
    arming: Arming,
) -> String {
    let mut rf = ReplayFile::new(
        ReplayMeta::new(pads.len() as u64)
            .with_rng_seed((run_seed(&spec.scene, spec.seed) >> 16) as u32),
    );
    let mut prev = 0u16;
    for (f, &p) in pads.iter().enumerate() {
        if f == 0 || p != prev {
            rf.push_event(f as u64, p);
            prev = p;
        }
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Soak-harness finding (pad input only - no disc bytes)."
    );
    let _ = writeln!(out, "{SCENE_TAG}{:?}", spec.scene);
    let _ = writeln!(out, "{SEED_TAG}{}", spec.seed);
    if let Some(f) = finding {
        let _ = writeln!(out, "{SIG_TAG}{:?}", f.signature());
        let _ = writeln!(out, "# detail: {}", f.detail.replace('\n', " "));
    }
    if let Some(n) = min_distinct {
        // Minimised under the neutral-pad control: the input that fed the
        // frozen window was dropped, so the detector must not ask for it.
        let _ = writeln!(out, "{MIN_DISTINCT_TAG}{n}");
    }
    if let Some(n) = env_u64("LEGAIA_SOAK_STARVE_FRAMES") {
        let _ = writeln!(out, "{STARVE_TAG}{n}");
    }
    if let Some(n) = env_u64("LEGAIA_SOAK_BATTLE_FRAMES") {
        let _ = writeln!(out, "{BATTLE_FRAMES_TAG}{n}");
    }
    let arming = format!(
        "{}{}",
        if arming.host { "host" } else { "bare" },
        if arming.retail { "-retail" } else { "" }
    );
    let _ = writeln!(out, "{ARMING_TAG}{arming:?}");
    let _ = writeln!(
        out,
        "# reproduce: LEGAIA_SOAK_REPLAY=<this file> cargo test -p legaia-engine-shell \
         --profile release-test --test integration soak_harness::soak_replay -- --nocapture"
    );
    out.push_str(&rf.to_toml_string().expect("serialise replay"));
    out
}

pub(super) struct LoadedReplay {
    pub(super) spec: RunSpec,
    pub(super) pads: Vec<u16>,
    pub(super) signature: Option<String>,
    pub(super) min_distinct: Option<usize>,
    pub(super) starve_frames: Option<u64>,
    pub(super) battle_frames: Option<u64>,
    pub(super) arming: Arming,
}

impl LoadedReplay {
    /// The detector settings this replay was recorded under.
    pub(super) fn tunables(&self) -> Tunables {
        let mut t = Tunables::from_env();
        if let Some(n) = self.min_distinct {
            t.min_distinct = n;
        }
        if let Some(n) = self.starve_frames {
            t.starve_frames = n;
        }
        if let Some(n) = self.battle_frames {
            t.battle_frames = n;
        }
        t.arming = self.arming;
        t
    }
}

pub(super) fn parse_tag(text: &str, tag: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let v = l.strip_prefix(tag)?.trim();
        Some(v.trim_matches('"').to_string())
    })
}

pub(super) fn load_replay(path: &Path) -> LoadedReplay {
    let text = std::fs::read_to_string(path).expect("read replay");
    let rf = ReplayFile::from_toml_str(&text).expect("parse j-replay-v1");
    let scene = parse_tag(&text, SCENE_TAG).expect("replay carries a soak-scene header");
    let seed = parse_tag(&text, SEED_TAG)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut pads = rf.expand_pad_stream();
    pads.truncate(rf.meta.frames as usize);
    LoadedReplay {
        spec: RunSpec {
            scene,
            seed,
            frames: rf.meta.frames,
        },
        pads,
        signature: parse_tag(&text, SIG_TAG),
        min_distinct: parse_tag(&text, MIN_DISTINCT_TAG).and_then(|s| s.parse().ok()),
        starve_frames: parse_tag(&text, STARVE_TAG).and_then(|s| s.parse().ok()),
        battle_frames: parse_tag(&text, BATTLE_FRAMES_TAG).and_then(|s| s.parse().ok()),
        arming: Arming::from_tag(parse_tag(&text, ARMING_TAG).as_deref()),
    }
}

pub(super) fn slug(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    out.truncate(96);
    out
}
