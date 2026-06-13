//! Engine determinism + scripted-input replay format (Phase J1).
//!
//! A `.toml` schema (`schema = "j-replay-v1"`) carrying:
//!
//! - a `[meta]` block - scenario binding, RNG seed, total frame count;
//! - a sparse `[[event]]` array of `(frame, pad_mask)` rows - only frames
//!   where the pad bitmask transitions are recorded, so a 600-frame
//!   replay with two button presses is six lines instead of six hundred;
//! - an optional `[[expected]]` block of `(frame, scene_mode,
//!   active_scene)` rows - the regression fixture against the engine's
//!   [`ModeTraceFrame`] stream.
//!
//! [`ReplayFile::expand_pad_stream`] turns the sparse event list into a
//! dense `Vec<u16>` of length `meta.frames + 1` (frame 0 inclusive) for
//! drivers that prefer a flat array.
//!
//! Pure data + serde here. The engine driver that consumes a [`ReplayFile`]
//! is wired in J2 / J3.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::mode_trace_oracle::ModeTraceFrame;

/// Current replay-file schema version. Bumped whenever the layout adds
/// or removes a field; readers reject anything that doesn't match so a
/// stale `.toml` produces a loud error instead of silent drift.
pub const REPLAY_SCHEMA_V1: &str = "j-replay-v1";

/// Top-level replay file. Mirrors the on-disk TOML shape one-to-one;
/// every field is named so the file is human-readable and `git diff`-able.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayFile {
    /// Schema marker, RNG seed, frame count, optional scenario binding.
    pub meta: ReplayMeta,
    /// Sparse list of pad-mask transitions. Frame `N` carries the mask
    /// that becomes effective on that frame and stays in force until the
    /// next event. Frame 0 is implicit `pad = 0` if no event names it.
    #[serde(default, rename = "event", skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<PadEvent>,
    /// Optional expected mode-trace fixture. When present, [`Self::diff`]
    /// compares a recorded trace against this and surfaces the first
    /// diverging row.
    #[serde(default, rename = "expected", skip_serializing_if = "Vec::is_empty")]
    pub expected: Vec<ExpectedFrame>,
}

/// `[meta]` block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayMeta {
    /// Must equal [`REPLAY_SCHEMA_V1`].
    pub schema: String,
    /// Scenario label from [`scripts/scenarios.toml`]. When set, the
    /// scenario's starting state (mednafen `.mc{slot}` save, expected
    /// active scene, etc.) drives boot before replay begins. `None`
    /// means "ad-hoc - boot the default scene".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<String>,
    /// Initial battle/effect-VM RNG seed. The PsyQ PRNG is `seed =
    /// seed * 1103515245 + 12345; (seed >> 16) & 0x7FFF`. See
    /// `legaia_engine_vm::battle_formulas::psyq_rand_step`.
    pub rng_seed: u32,
    /// Total frame count to drive. The expanded pad stream has length
    /// `frames + 1` (frame 0 + the next `frames` ticks).
    pub frames: u64,
}

/// One pad-mask transition.
///
/// `pad` is the PSX pad bitmask. Layout matches
/// [`legaia_engine_core::input::PadButton::mask`]: Cross = `0x4000`,
/// Circle = `0x2000`, Up = `0x0010`, Down = `0x0040`, Left = `0x0080`,
/// Right = `0x0020`, etc. Stored as a plain `u16` so the wire form is
/// `pad = 0x4000` rather than `pad = ["Cross"]`. Future writers can
/// fold human-readable names in if the byte-soup becomes a problem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PadEvent {
    pub frame: u64,
    pub pad: u16,
}

/// One row in the optional expected-trace fixture.
///
/// Subset of [`ModeTraceFrame`] - keeps only the fields both engine
/// and retail emitters populate (`scene_mode` + `active_scene`).
/// `game_mode` is engine-side `None` today so it would always
/// false-mismatch (see [`crate::mode_trace_oracle`] for the asymmetry
/// note).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedFrame {
    pub frame: u64,
    pub scene_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_scene: Option<String>,
}

/// First row on which a recorded engine trace disagrees with the
/// fixture. Surfaced by [`ReplayFile::diff`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayDivergence {
    pub frame: u64,
    pub kind: ReplayDivergenceKind,
    pub expected: ExpectedFrame,
    pub recorded: ModeTraceFrame,
}

/// Which axis disagreed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayDivergenceKind {
    /// `scene_mode` strings disagreed.
    SceneMode,
    /// `active_scene` disagreed (either side resolved a name the other
    /// didn't, or both resolved and they differ).
    ActiveScene,
    /// The expected row references a frame index past the end of the
    /// recorded trace.
    Truncated,
}

impl ReplayFile {
    /// Construct a minimal replay file with no events and no fixture.
    /// Convenience for tests + the record subcommand's initial state.
    pub fn new(meta: ReplayMeta) -> Self {
        Self {
            meta,
            events: Vec::new(),
            expected: Vec::new(),
        }
    }

    /// Parse a replay file from a TOML string. Rejects unknown schema
    /// strings; rejects events with `frame > meta.frames`; rejects
    /// out-of-order events (writers MUST emit `frame` ascending so
    /// readers can stream without a sort).
    pub fn from_toml_str(s: &str) -> Result<Self> {
        let file: ReplayFile = toml::from_str(s).context("parse replay TOML")?;
        file.validate()?;
        Ok(file)
    }

    /// Load + parse a replay file from disk.
    pub fn from_path(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read replay file {}", path.display()))?;
        Self::from_toml_str(&text)
    }

    /// Serialise to a TOML string. Always emits the schema marker so the
    /// round-trip is closed.
    pub fn to_toml_string(&self) -> Result<String> {
        // toml's serializer fails only on internal errors (no custom
        // serialize impls in this module) so unwrap-context is fine.
        toml::to_string_pretty(self).context("serialise ReplayFile to TOML")
    }

    /// Write the replay to disk.
    pub fn write_to(&self, path: &Path) -> Result<()> {
        let text = self.to_toml_string()?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create replay parent dir {}", parent.display()))?;
        }
        std::fs::write(path, text)
            .with_context(|| format!("write replay file {}", path.display()))?;
        Ok(())
    }

    /// Sanity check the contents. Called automatically by parse paths;
    /// the record subcommand calls it before writing.
    pub fn validate(&self) -> Result<()> {
        if self.meta.schema != REPLAY_SCHEMA_V1 {
            bail!(
                "replay schema mismatch: file declares {:?}, reader supports {:?}",
                self.meta.schema,
                REPLAY_SCHEMA_V1
            );
        }
        let mut last: Option<u64> = None;
        for ev in &self.events {
            if ev.frame > self.meta.frames {
                bail!(
                    "replay event frame {} exceeds meta.frames {}",
                    ev.frame,
                    self.meta.frames
                );
            }
            if let Some(prev) = last
                && ev.frame < prev
            {
                bail!(
                    "replay events must be frame-ascending; saw {} after {}",
                    ev.frame,
                    prev
                );
            }
            last = Some(ev.frame);
        }
        for ex in &self.expected {
            if ex.frame > self.meta.frames {
                bail!(
                    "replay expected-frame {} exceeds meta.frames {}",
                    ex.frame,
                    self.meta.frames
                );
            }
        }
        Ok(())
    }

    /// Sparse event list -> dense `Vec<u16>` of length `meta.frames + 1`.
    /// Each slot is the pad mask in effect on that frame. Frame 0 is
    /// implicit `0x0000` unless an event names frame 0 explicitly.
    pub fn expand_pad_stream(&self) -> Vec<u16> {
        let len = (self.meta.frames as usize).saturating_add(1);
        let mut out = vec![0u16; len];
        let mut held: u16 = 0;
        let mut idx = 0usize;
        for ev in &self.events {
            let upto = (ev.frame as usize).min(len);
            // Fill [idx..upto) with the previously-held mask.
            for slot in &mut out[idx..upto] {
                *slot = held;
            }
            held = ev.pad;
            idx = upto;
        }
        // Tail: fill the rest with the last-held mask.
        for slot in &mut out[idx..] {
            *slot = held;
        }
        out
    }

    /// Compare a recorded engine trace against the optional expected
    /// fixture. Returns `Ok(None)` when no fixture is present or when
    /// every expected row matches the corresponding recorded row.
    ///
    /// Matching rule per axis:
    /// - `scene_mode` must match exactly.
    /// - `active_scene`: if expected has `Some`, recorded must match it.
    ///   `None` on expected is "don't care" (regression-fixture authors
    ///   leave it off when the row only constrains the scene mode).
    pub fn diff(&self, recorded: &[ModeTraceFrame]) -> Option<ReplayDivergence> {
        for ex in &self.expected {
            // Pick the recorded row whose `frame` equals the expected
            // frame. The recorded trace from
            // `build_engine_mode_trace` carries `frame = BootSession::frames`,
            // monotonically increasing from 0, with one row per tick - so
            // index = frame for the common path. Fall back to a linear
            // search for replays that started with a non-zero frame.
            let row_by_idx = recorded.get(ex.frame as usize);
            let row = match row_by_idx {
                Some(r) if r.frame == ex.frame => Some(r),
                _ => recorded.iter().find(|r| r.frame == ex.frame),
            };
            let Some(rec) = row else {
                return Some(ReplayDivergence {
                    frame: ex.frame,
                    kind: ReplayDivergenceKind::Truncated,
                    expected: ex.clone(),
                    recorded: ModeTraceFrame {
                        frame: ex.frame,
                        game_mode: None,
                        game_mode_name: None,
                        scene_mode: String::new(),
                        active_scene: None,
                    },
                });
            };
            if rec.scene_mode != ex.scene_mode {
                return Some(ReplayDivergence {
                    frame: ex.frame,
                    kind: ReplayDivergenceKind::SceneMode,
                    expected: ex.clone(),
                    recorded: rec.clone(),
                });
            }
            if let Some(want) = ex.active_scene.as_deref()
                && rec.active_scene.as_deref() != Some(want)
            {
                return Some(ReplayDivergence {
                    frame: ex.frame,
                    kind: ReplayDivergenceKind::ActiveScene,
                    expected: ex.clone(),
                    recorded: rec.clone(),
                });
            }
        }
        None
    }

    /// Convenience: append `(frame, pad)` to the event list. Caller is
    /// responsible for emitting in frame-ascending order. The record
    /// subcommand uses this; tests use it for fixture construction.
    pub fn push_event(&mut self, frame: u64, pad: u16) {
        self.events.push(PadEvent { frame, pad });
    }

    /// Convenience: append `(frame, scene_mode, active_scene)` to the
    /// expected fixture.
    pub fn push_expected(&mut self, frame: u64, scene_mode: &str, active_scene: Option<&str>) {
        self.expected.push(ExpectedFrame {
            frame,
            scene_mode: scene_mode.to_string(),
            active_scene: active_scene.map(str::to_string),
        });
    }
}

impl ReplayMeta {
    /// Construct a v1 meta block with no scenario binding. RNG seed
    /// defaults to the engine's canonical `0xDEAD_C0DE` so out-of-the-box
    /// replays match the engine's default battle-session seed.
    pub fn new(frames: u64) -> Self {
        Self {
            schema: REPLAY_SCHEMA_V1.to_string(),
            scenario: None,
            rng_seed: 0xDEAD_C0DE,
            frames,
        }
    }

    /// Bind a scenario label. Returns `self` so the builder shape stays
    /// terse.
    pub fn with_scenario(mut self, label: impl Into<String>) -> Self {
        self.scenario = Some(label.into());
        self
    }

    /// Override the RNG seed.
    pub fn with_rng_seed(mut self, seed: u32) -> Self {
        self.rng_seed = seed;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(frame: u64, scene_mode: &str, active: Option<&str>) -> ModeTraceFrame {
        ModeTraceFrame {
            frame,
            game_mode: None,
            game_mode_name: None,
            scene_mode: scene_mode.to_string(),
            active_scene: active.map(str::to_string),
        }
    }

    #[test]
    fn meta_defaults_carry_canonical_seed_and_schema() {
        let m = ReplayMeta::new(120);
        assert_eq!(m.schema, REPLAY_SCHEMA_V1);
        assert_eq!(m.rng_seed, 0xDEAD_C0DE);
        assert_eq!(m.frames, 120);
        assert!(m.scenario.is_none());
    }

    #[test]
    fn toml_roundtrip_preserves_every_field() {
        let mut file = ReplayFile::new(
            ReplayMeta::new(60)
                .with_scenario("title_attract")
                .with_rng_seed(0x1234_5678),
        );
        file.push_event(0, 0x0000);
        file.push_event(10, 0x4000);
        file.push_event(12, 0x0000);
        file.push_expected(0, "Title", None);
        file.push_expected(60, "Field", Some("town01"));

        let text = file.to_toml_string().unwrap();
        let parsed = ReplayFile::from_toml_str(&text).unwrap();
        assert_eq!(parsed, file);
    }

    #[test]
    fn schema_mismatch_is_rejected_loudly() {
        let bad = r#"
            [meta]
            schema = "j-replay-v999"
            rng_seed = 1
            frames = 10
        "#;
        let err = ReplayFile::from_toml_str(bad).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("schema mismatch"), "got: {msg}");
    }

    #[test]
    fn event_past_frames_count_is_rejected() {
        let mut file = ReplayFile::new(ReplayMeta::new(10));
        file.push_event(11, 0x4000);
        let err = file.validate().unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("exceeds meta.frames"), "got: {msg}");
    }

    #[test]
    fn out_of_order_events_are_rejected() {
        let mut file = ReplayFile::new(ReplayMeta::new(10));
        file.push_event(5, 0x4000);
        file.push_event(3, 0x0000);
        let err = file.validate().unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("frame-ascending"), "got: {msg}");
    }

    #[test]
    fn expand_pad_stream_is_dense_and_holds_between_events() {
        let mut file = ReplayFile::new(ReplayMeta::new(5));
        // Press Cross at frame 1, release at frame 3.
        file.push_event(1, 0x4000);
        file.push_event(3, 0x0000);

        let stream = file.expand_pad_stream();
        // Length = frames + 1 (slots for frames 0..=5).
        assert_eq!(stream.len(), 6);
        assert_eq!(stream, vec![0x0000, 0x4000, 0x4000, 0x0000, 0x0000, 0x0000]);
    }

    #[test]
    fn expand_pad_stream_with_frame_zero_event() {
        let mut file = ReplayFile::new(ReplayMeta::new(3));
        // Holding Cross from frame 0.
        file.push_event(0, 0x4000);
        let stream = file.expand_pad_stream();
        assert_eq!(stream, vec![0x4000, 0x4000, 0x4000, 0x4000]);
    }

    #[test]
    fn expand_pad_stream_with_no_events_is_all_zeros() {
        let file = ReplayFile::new(ReplayMeta::new(3));
        let stream = file.expand_pad_stream();
        assert_eq!(stream, vec![0u16; 4]);
    }

    #[test]
    fn diff_returns_none_when_no_fixture() {
        let file = ReplayFile::new(ReplayMeta::new(3));
        let recorded = vec![rec(0, "Title", None), rec(1, "Title", None)];
        assert!(file.diff(&recorded).is_none());
    }

    #[test]
    fn diff_returns_none_when_fixture_matches() {
        let mut file = ReplayFile::new(ReplayMeta::new(3));
        file.push_expected(0, "Title", None);
        file.push_expected(1, "Title", Some("town01"));
        let recorded = vec![rec(0, "Title", None), rec(1, "Title", Some("town01"))];
        assert!(file.diff(&recorded).is_none());
    }

    #[test]
    fn diff_surfaces_scene_mode_mismatch() {
        let mut file = ReplayFile::new(ReplayMeta::new(3));
        file.push_expected(1, "Field", None);
        let recorded = vec![rec(0, "Title", None), rec(1, "Title", None)];
        let d = file.diff(&recorded).unwrap();
        assert_eq!(d.frame, 1);
        assert_eq!(d.kind, ReplayDivergenceKind::SceneMode);
    }

    #[test]
    fn diff_surfaces_active_scene_mismatch() {
        let mut file = ReplayFile::new(ReplayMeta::new(3));
        file.push_expected(1, "Field", Some("town01"));
        let recorded = vec![rec(0, "Title", None), rec(1, "Field", Some("town02"))];
        let d = file.diff(&recorded).unwrap();
        assert_eq!(d.frame, 1);
        assert_eq!(d.kind, ReplayDivergenceKind::ActiveScene);
    }

    #[test]
    fn diff_treats_expected_none_active_scene_as_dont_care() {
        let mut file = ReplayFile::new(ReplayMeta::new(3));
        file.push_expected(1, "Field", None);
        let recorded = vec![rec(0, "Title", None), rec(1, "Field", Some("town01"))];
        assert!(file.diff(&recorded).is_none());
    }

    #[test]
    fn diff_surfaces_truncation_when_recorded_is_short() {
        let mut file = ReplayFile::new(ReplayMeta::new(5));
        file.push_expected(4, "Field", None);
        let recorded = vec![rec(0, "Title", None)];
        let d = file.diff(&recorded).unwrap();
        assert_eq!(d.frame, 4);
        assert_eq!(d.kind, ReplayDivergenceKind::Truncated);
    }

    #[test]
    fn diff_finds_row_by_frame_field_not_array_index() {
        // Recorder may have skipped some frames; matching is by frame
        // value, not slice index.
        let mut file = ReplayFile::new(ReplayMeta::new(10));
        file.push_expected(5, "Field", Some("town01"));
        let recorded = vec![rec(3, "Title", None), rec(5, "Field", Some("town01"))];
        assert!(file.diff(&recorded).is_none());
    }

    #[test]
    fn write_then_read_disk_roundtrip() {
        let tmp = tempfile::Builder::new()
            .prefix("replay-roundtrip-")
            .suffix(".toml")
            .tempfile()
            .unwrap();
        let mut file = ReplayFile::new(ReplayMeta::new(2).with_rng_seed(1));
        file.push_event(0, 0x4000);
        file.write_to(tmp.path()).unwrap();
        let loaded = ReplayFile::from_path(tmp.path()).unwrap();
        assert_eq!(loaded, file);
    }
}
