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
//!   [`ModeTraceFrame`] stream;
//! - an optional sparse `[[switch]]` array of [`SessionSwitch`] rows - the
//!   host-side session changes that reach the simulation and that the pad
//!   stream cannot carry: the action-movement switch, the active expansion
//!   pack (by id and [`pack_content_hash`]), and the expansion mode seat,
//!   card menu, battle and map entries a host makes outside the pad.
//!
//! Every addition to the schema is optional and skipped when empty, so a
//! file written before it parses, and serialises back, unchanged.
//!
//! [`ReplayFile::expand_pad_stream`] turns the sparse event list into a
//! dense `Vec<u16>` of length `meta.frames + 1` (frame 0 inclusive) for
//! drivers that prefer a flat array.
//!
//! Pure data + serde here. The engine driver that consumes a [`ReplayFile`]
//! is wired in J2 / J3.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use legaia_engine_core::world::World;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_session::mode_trace::ModeTraceFrame;
use legaia_expansion_format::ExpansionPack;

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
    /// Sparse list of session switches, frame-ascending. A switch at frame
    /// `N` is applied before frame `N` ticks, the same frame its pad event
    /// would take effect on.
    #[serde(default, rename = "switch", skip_serializing_if = "Vec::is_empty")]
    pub switches: Vec<SessionSwitch>,
}

/// One session switch: a host-side change to the running session that
/// reaches the simulation and is not pad input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSwitch {
    /// The frame the change is applied before.
    pub frame: u64,
    /// What changes.
    #[serde(flatten)]
    pub change: SwitchChange,
}

/// What a [`SessionSwitch`] changes. The TOML form names the variant in a
/// `set` key: `set = "action_movement"`, `on = true`.
///
/// The value-setting rows (`action_movement`, `pack`, `no_pack`) are
/// idempotent - applying one that already holds changes nothing - so a
/// recorder may note them by watching the state. The entry rows open
/// something and are recorded where the host makes the call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "set", rename_all = "snake_case")]
pub enum SwitchChange {
    /// `WorldToggles::action_movement`.
    ActionMovement { on: bool },
    /// Activate expansion pack `id`, whose [`pack_content_hash`] is `hash`.
    /// The replay refuses a pack whose content differs.
    Pack { id: String, hash: String },
    /// Deactivate the active expansion pack.
    NoPack,
    /// Open the expansion mode picker (`World::open_pack_mode_picker`).
    ModePicker,
    /// Enter mode instance `id` (`World::enter_pack_mode`).
    Mode { id: String },
    /// Leave the mode seat (`World::exit_pack_mode`).
    LeaveMode,
    /// Open the active pack's card menu (`World::open_card_menu`).
    CardMenu,
    /// Start the active pack's formation `formation`
    /// (`World::start_expansion_battle`).
    Battle { formation: String },
    /// Enter map `map` of the active pack at spawn `spawn`
    /// (`SceneHost::enter_expansion_map`; empty = the map's first spawn).
    Map { map: String, spawn: String },
    /// The run's rules (`World::run_rules`, `legaia_engine_run_rules`):
    /// difficulty, New Game+, ironman, randomizer seed, mode. A value row:
    /// the recorder notes it whenever the world's rules change.
    RunRules {
        rules: legaia_engine_run_rules::rules::RunRules,
    },
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
    /// `legaia_engine_system::rand::psyq_rand_step`.
    pub rng_seed: u32,
    /// Total frame count to drive. The expanded pad stream has length
    /// `frames + 1` (frame 0 + the next `frames` ticks).
    pub frames: u64,
}

/// One pad-mask transition.
///
/// `pad` is the PSX pad bitmask. Layout matches
/// [`legaia_engine_system::input::PadButton::mask`]: Cross = `0x4000`,
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
/// false-mismatch (see `legaia_parity::mode_trace_oracle` for the asymmetry
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
            switches: Vec::new(),
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
        let mut last: Option<u64> = None;
        for sw in &self.switches {
            if sw.frame > self.meta.frames {
                bail!(
                    "replay switch frame {} exceeds meta.frames {}",
                    sw.frame,
                    self.meta.frames
                );
            }
            if let Some(prev) = last
                && sw.frame < prev
            {
                bail!(
                    "replay switches must be frame-ascending; saw {} after {}",
                    sw.frame,
                    prev
                );
            }
            last = Some(sw.frame);
        }
        Ok(())
    }

    /// The switches applied before frame `frame` ticks, in file order.
    pub fn switches_at(&self, frame: u64) -> impl Iterator<Item = &SwitchChange> {
        self.switches
            .iter()
            .filter(move |s| s.frame == frame)
            .map(|s| &s.change)
    }

    /// Append a switch at `frame`. Caller keeps the list frame-ascending.
    pub fn push_switch(&mut self, frame: u64, change: SwitchChange) {
        self.switches.push(SessionSwitch { frame, change });
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
            // `build_engine_mode_trace` carries `frame = NativeSession::frames`,
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

/// The content hash a [`SwitchChange::Pack`] row carries: SHA-256 (lower-case
/// hex) over the pack's canonical file tree ([`ExpansionPack::to_files`]),
/// each file as its path, a NUL, its length (`u64` LE) and its bytes. A pack
/// read from its directory and from its `.lxpack` bundle hash the same; any
/// edit to its content changes the hash.
pub fn pack_content_hash(pack: &ExpansionPack) -> Result<String> {
    let files = pack.to_files().context("serialise the pack to hash it")?;
    let mut h = Sha256::new();
    for (path, bytes) in &files {
        h.update(path.as_bytes());
        h.update([0u8]);
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(bytes);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The `pack` switch row for `pack` as it stands.
pub fn pack_switch(pack: &ExpansionPack) -> Result<SwitchChange> {
    Ok(SwitchChange::Pack {
        id: pack.manifest.id.clone(),
        hash: pack_content_hash(pack)?,
    })
}

/// The expansion packs a replay may activate, each with its content hash.
#[derive(Debug, Clone, Default)]
pub struct PackShelf {
    packs: Vec<(Arc<ExpansionPack>, String)>,
}

impl PackShelf {
    /// An empty shelf: a replay naming any pack fails.
    pub fn new() -> Self {
        Self::default()
    }

    /// Put `pack` on the shelf.
    pub fn add(&mut self, pack: Arc<ExpansionPack>) -> Result<()> {
        let hash = pack_content_hash(&pack)?;
        self.packs.push((pack, hash));
        Ok(())
    }

    /// A shelf of the packs at `paths` (pack directories or `.lxpack`
    /// bundles).
    pub fn from_paths(paths: &[PathBuf]) -> Result<Self> {
        let mut shelf = Self::new();
        for p in paths {
            let files = legaia_expansion_format::read_files(p)?;
            let pack = ExpansionPack::from_files(&files)
                .with_context(|| format!("parse the expansion pack at {}", p.display()))?;
            shelf.add(Arc::new(pack))?;
        }
        Ok(shelf)
    }

    /// The pack with id `id` and content hash `hash`. A pack of that id
    /// whose content differs is refused by name: the replay was recorded
    /// against other content and would not reproduce.
    pub fn resolve(&self, id: &str, hash: &str) -> Result<Arc<ExpansionPack>> {
        let mut seen = Vec::new();
        for (pack, h) in &self.packs {
            if pack.manifest.id == id {
                if h == hash {
                    return Ok(pack.clone());
                }
                seen.push(h.as_str());
            }
        }
        if seen.is_empty() {
            bail!("the replay activates expansion pack '{id}', which no --pack supplies");
        }
        bail!(
            "expansion pack '{id}' differs from the one the replay was recorded with \
             (recorded {hash}, supplied {})",
            seen.join(", ")
        )
    }
}

/// What a replay driver applies [`SwitchChange`]s to: a bare [`World`], or
/// a [`SceneHost`] (which can also enter a pack map and merges a pack's
/// base monsters on activation).
pub trait ReplaySeat {
    /// The world the switches act on.
    fn replay_world(&mut self) -> &mut World;

    /// Activate `pack`.
    fn replay_activate_pack(&mut self, pack: Arc<ExpansionPack>) -> Result<()> {
        self.replay_world()
            .activate_expansion(pack)
            .map_err(|p| anyhow::anyhow!("the expansion pack does not activate: {}", p.join("; ")))
    }

    /// Enter map `map` of the active pack at `spawn`.
    fn replay_enter_map(&mut self, map: &str, spawn: &str) -> Result<()> {
        let _ = spawn;
        bail!("a bare world cannot enter pack map '{map}': the replay needs a scene host")
    }
}

impl ReplaySeat for World {
    fn replay_world(&mut self) -> &mut World {
        self
    }
}

impl ReplaySeat for SceneHost {
    fn replay_world(&mut self) -> &mut World {
        &mut self.world
    }

    fn replay_activate_pack(&mut self, pack: Arc<ExpansionPack>) -> Result<()> {
        self.activate_expansion_pack(pack)?;
        self.merge_expansion_monsters();
        Ok(())
    }

    fn replay_enter_map(&mut self, map: &str, spawn: &str) -> Result<()> {
        self.enter_expansion_map(map, spawn)
    }
}

/// Apply one switch to `seat`. Packs come off `shelf`.
pub fn apply_switch<S: ReplaySeat + ?Sized>(
    seat: &mut S,
    change: &SwitchChange,
    shelf: &PackShelf,
) -> Result<()> {
    match change {
        SwitchChange::ActionMovement { on } => seat.replay_world().toggles.action_movement = *on,
        SwitchChange::Pack { id, hash } => {
            let active = seat.replay_world().expansion.pack.clone();
            if let Some(p) = active
                && p.manifest.id == *id
                && pack_content_hash(&p)? == *hash
            {
                return Ok(());
            }
            let pack = shelf.resolve(id, hash)?;
            seat.replay_activate_pack(pack)?;
        }
        SwitchChange::NoPack => seat.replay_world().deactivate_expansion(),
        SwitchChange::ModePicker => seat.replay_world().open_pack_mode_picker(),
        SwitchChange::Mode { id } => seat
            .replay_world()
            .enter_pack_mode(id)
            .map_err(|e| anyhow::anyhow!("enter mode '{id}': {e}"))?,
        SwitchChange::LeaveMode => seat.replay_world().exit_pack_mode(),
        SwitchChange::CardMenu => {
            if !seat.replay_world().open_card_menu() {
                bail!("the card menu does not open: the active pack has no cards");
            }
        }
        SwitchChange::Battle { formation } => {
            if !seat.replay_world().start_expansion_battle(formation) {
                bail!("expansion formation '{formation}' does not start");
            }
        }
        SwitchChange::Map { map, spawn } => seat.replay_enter_map(map, spawn)?,
        SwitchChange::RunRules { rules } => seat.replay_world().run_rules.rules = rules.clone(),
    }
    Ok(())
}

/// Apply every switch `replay` holds at `frame`, in file order.
pub fn apply_switches_at<S: ReplaySeat + ?Sized>(
    seat: &mut S,
    replay: &ReplayFile,
    frame: u64,
    shelf: &PackShelf,
) -> Result<()> {
    for change in replay.switches_at(frame) {
        apply_switch(seat, change, shelf)
            .with_context(|| format!("replay switch at frame {frame}"))?;
    }
    Ok(())
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
    /// A file in the schema as it stood before `[[switch]]`: it must parse
    /// to the same events and serialise back byte-for-byte.
    const PRE_SWITCH_FILE: &str = r#"[meta]
schema = "j-replay-v1"
scenario = "title_attract"
rng_seed = 305419896
frames = 60

[[event]]
frame = 0
pad = 0

[[event]]
frame = 10
pad = 16384

[[expected]]
frame = 60
scene_mode = "Field"
active_scene = "town01"
"#;

    #[test]
    fn a_file_without_switches_parses_and_writes_back_unchanged() {
        let parsed = ReplayFile::from_toml_str(PRE_SWITCH_FILE).unwrap();
        assert!(parsed.switches.is_empty());
        assert_eq!(parsed.events.len(), 2);
        assert_eq!(parsed.to_toml_string().unwrap(), PRE_SWITCH_FILE);
    }

    #[test]
    fn switches_round_trip_in_every_shape() {
        let mut file = ReplayFile::new(ReplayMeta::new(100));
        file.push_switch(
            0,
            SwitchChange::Pack {
                id: "rebirth".into(),
                hash: "ab".repeat(32),
            },
        );
        file.push_switch(0, SwitchChange::ActionMovement { on: true });
        file.push_switch(5, SwitchChange::ModePicker);
        file.push_switch(
            6,
            SwitchChange::Mode {
                id: "builtin-golf".into(),
            },
        );
        file.push_switch(7, SwitchChange::LeaveMode);
        file.push_switch(8, SwitchChange::CardMenu);
        file.push_switch(
            9,
            SwitchChange::Battle {
                formation: "gauntlet".into(),
            },
        );
        file.push_switch(
            10,
            SwitchChange::Map {
                map: "rebirth_tower_1f".into(),
                spawn: "entrance".into(),
            },
        );
        file.push_switch(11, SwitchChange::NoPack);
        file.push_switch(
            11,
            SwitchChange::RunRules {
                rules: legaia_engine_run_rules::rules::RunRules {
                    difficulty: legaia_engine_run_rules::rules::Difficulty::Hard,
                    randomizer: Some(legaia_engine_run_rules::rules::Randomizer::all("seed")),
                    ..Default::default()
                },
            },
        );
        let text = file.to_toml_string().unwrap();
        assert!(text.contains("set = \"run_rules\""), "{text}");
        assert!(text.contains("set = \"action_movement\""), "{text}");
        assert_eq!(ReplayFile::from_toml_str(&text).unwrap(), file);
        let at0: Vec<_> = file.switches_at(0).collect();
        assert_eq!(at0.len(), 2);
        assert_eq!(file.switches_at(1).count(), 0);
    }

    #[test]
    fn switches_out_of_order_or_past_the_end_are_rejected() {
        let mut file = ReplayFile::new(ReplayMeta::new(10));
        file.push_switch(5, SwitchChange::LeaveMode);
        file.push_switch(3, SwitchChange::LeaveMode);
        let msg = format!("{:#}", file.validate().unwrap_err());
        assert!(msg.contains("frame-ascending"), "got: {msg}");
        let mut file = ReplayFile::new(ReplayMeta::new(10));
        file.push_switch(11, SwitchChange::LeaveMode);
        let msg = format!("{:#}", file.validate().unwrap_err());
        assert!(msg.contains("exceeds meta.frames"), "got: {msg}");
    }

    fn tiny_pack(name: &str) -> ExpansionPack {
        let mut files = legaia_expansion_format::PackFiles::new();
        files.insert(
            "pack.toml".into(),
            format!("id = \"tiny\"\nname = \"{name}\"\nversion = \"0.1.0\"\n").into_bytes(),
        );
        ExpansionPack::from_files(&files).unwrap()
    }

    #[test]
    fn the_content_hash_follows_content_not_container() {
        let pack = tiny_pack("Tiny");
        let h = pack_content_hash(&pack).unwrap();
        assert_eq!(h.len(), 64);
        let bundle =
            legaia_expansion_format::bundle::write_bundle(&pack.to_files().unwrap()).unwrap();
        let from_bundle = ExpansionPack::from_bundle(&bundle).unwrap();
        assert_eq!(pack_content_hash(&from_bundle).unwrap(), h);
        assert_ne!(pack_content_hash(&tiny_pack("Tiny 2")).unwrap(), h);
    }

    #[test]
    fn the_shelf_refuses_a_pack_whose_content_moved() {
        let mut shelf = PackShelf::new();
        shelf.add(Arc::new(tiny_pack("Tiny"))).unwrap();
        let good = pack_content_hash(&tiny_pack("Tiny")).unwrap();
        assert!(shelf.resolve("tiny", &good).is_ok());
        let moved = pack_content_hash(&tiny_pack("Tiny 2")).unwrap();
        let msg = format!("{:#}", shelf.resolve("tiny", &moved).unwrap_err());
        assert!(msg.contains("differs"), "got: {msg}");
        let msg = format!("{:#}", shelf.resolve("other", &good).unwrap_err());
        assert!(msg.contains("no --pack"), "got: {msg}");
    }

    #[test]
    fn switches_apply_to_a_bare_world() {
        let mut shelf = PackShelf::new();
        shelf.add(Arc::new(tiny_pack("Tiny"))).unwrap();
        let mut world = World::new();
        apply_switch(
            &mut world,
            &SwitchChange::ActionMovement { on: true },
            &shelf,
        )
        .unwrap();
        assert!(world.toggles.action_movement);
        let row = pack_switch(&tiny_pack("Tiny")).unwrap();
        apply_switch(&mut world, &row, &shelf).unwrap();
        assert_eq!(world.expansion.pack.as_ref().unwrap().manifest.id, "tiny");
        // Re-applying the active pack is a no-op, not a re-activation.
        let before = Arc::as_ptr(world.expansion.pack.as_ref().unwrap());
        apply_switch(&mut world, &row, &shelf).unwrap();
        assert_eq!(Arc::as_ptr(world.expansion.pack.as_ref().unwrap()), before);
        let map = SwitchChange::Map {
            map: "m".into(),
            spawn: String::new(),
        };
        assert!(apply_switch(&mut world, &map, &shelf).is_err());
        apply_switch(&mut world, &SwitchChange::NoPack, &shelf).unwrap();
        assert!(world.expansion.pack.is_none());
    }
}
