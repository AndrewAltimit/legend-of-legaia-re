//! Mode-trace oracle plumbing shared between the `legaia-engine mode-trace`
//! subcommand and the disc-gated `mode_trace_e3` integration test.
//!
//! Mirrors the shape of [`crate::vram_oracle`] but the diff axis is the
//! engine's high-level dispatch state instead of VRAM bytes: per-frame
//! `(game_mode, scene_mode, active_scene)`. The retail side reads a
//! mednafen save state; the engine side ticks a [`BootSession`] and
//! samples each frame.
//!
//! **Asymmetry.** The engine port doesn't currently drive the 28-mode
//! game-mode dispatcher (see `legaia_engine_core::mode::ModeDriver`) -
//! [`BootSession::tick`] goes through `SceneHost::tick`, which keeps
//! [`world.mode`](legaia_engine_core::world::SceneMode) up to date but
//! not the master game-mode word `_DAT_8007B83C`. So engine-sampled
//! [`ModeTraceFrame::game_mode`] is `None`; retail-sampled frames fill
//! it from main RAM directly. [`first_mode_trace_divergence`] compares
//! only the fields both sides emit (scene_mode + active_scene); a future
//! engine port that models the dispatcher can populate game_mode and the
//! comparison extends automatically.
//!
//! JSONL is the wire format - one record per line, matching the
//! Phase-E3 spec "engine emits JSONL of (frame, game_mode, scene_mode,
//! active_scene, key_globals)".

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::{BootConfig, BootSession};

/// One sample of the engine's (or retail's) high-level dispatch state.
/// Fields that the sampler can't fill - the engine port doesn't model
/// the 28-mode dispatcher today - are left as `None` rather than zeroed
/// so downstream diff tools can tell "didn't observe" from "observed 0".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeTraceFrame {
    /// Frame counter. Engine: wall-clock frame from
    /// [`BootSession::frames`]. Retail snapshot: always 0 (save states
    /// are single-frame).
    pub frame: u64,
    /// `_DAT_8007B83C` byte (an index into the 28-mode table). Retail:
    /// read directly. Engine: `None` until the port models the
    /// dispatcher.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub game_mode: Option<u8>,
    /// Debug name from `mode::TABLE` (e.g. `"MAPDISP MODE"`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub game_mode_name: Option<String>,
    /// [`SceneMode`](legaia_engine_core::world::SceneMode) variant name:
    /// `"Title"` / `"Field"` / `"Battle"` / `"Cutscene"` / `"WorldMap"`.
    /// Both emitters fill this. Retail derives it from `game_mode` via
    /// [`legaia_engine_core::mode::GameMode::scene_mode`].
    pub scene_mode: String,
    /// CDNAME label of the currently-loaded scene, e.g. `"town01"`.
    /// Engine reads from `host.scene.as_ref().map(|s| s.name)`. Retail
    /// reads from the scene-bundle pool at `0x80084540` slot 0 via
    /// [`legaia_engine_core::capture_observations::field_pack_intra_transition::read_pool_slot_name`].
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub active_scene: Option<String>,
}

/// Run a [`BootSession`] on `scene_name`, sampling `(scene_mode,
/// active_scene)` immediately after boot and again after every tick.
/// Returns `frames + 1` records.
pub fn build_engine_mode_trace(
    scene_name: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    frames: u64,
) -> Result<Vec<ModeTraceFrame>> {
    let cfg = BootConfig {
        scene: scene_name.to_string(),
        enable_audio: false,
    };
    let mut session = match disc {
        Some(p) => BootSession::open_disc(p, &cfg)?,
        None => BootSession::open(extracted_root, &cfg)?,
    };
    let mut out = Vec::with_capacity((frames as usize).saturating_add(1));
    out.push(sample_engine_frame(&session));
    for _ in 0..frames {
        let _ = session.tick()?;
        out.push(sample_engine_frame(&session));
    }
    Ok(out)
}

fn sample_engine_frame(session: &BootSession) -> ModeTraceFrame {
    ModeTraceFrame {
        frame: session.frames,
        game_mode: None,
        game_mode_name: None,
        scene_mode: scene_mode_name(session.host.world.mode).to_string(),
        active_scene: session.host.scene.as_ref().map(|s| s.name.clone()),
    }
}

fn scene_mode_name(m: legaia_engine_core::world::SceneMode) -> &'static str {
    use legaia_engine_core::world::SceneMode;
    match m {
        SceneMode::Title => "Title",
        SceneMode::Field => "Field",
        SceneMode::Battle => "Battle",
        SceneMode::Cutscene => "Cutscene",
        SceneMode::WorldMap => "WorldMap",
    }
}

/// Lift a single mode-trace sample out of a mednafen `.mc{slot}` save.
/// Reads main RAM via [`legaia_mednafen::SaveState::main_ram`] and
/// pulls:
///
/// - `game_mode` from `_DAT_8007B83C`
///   ([`capture_observations::cutscene_trigger_corpus::GAME_MODE_ADDR`](legaia_engine_core::capture_observations::cutscene_trigger_corpus::GAME_MODE_ADDR)).
/// - `scene_mode` derived from `game_mode` via
///   [`GameMode::scene_mode`](legaia_engine_core::mode::GameMode::scene_mode).
/// - `active_scene` from the scene-bundle pool slot 0 at `0x80084540`.
pub fn load_runtime_mode_trace_from_save(save: &Path) -> Result<ModeTraceFrame> {
    use legaia_engine_core::capture_observations::cutscene_trigger_corpus::read_game_mode;
    use legaia_engine_core::capture_observations::field_pack_intra_transition::read_pool_slot_name;
    use legaia_engine_core::mode::GameMode;
    use legaia_mednafen::SaveState;

    let state = SaveState::from_path(save)
        .with_context(|| format!("load mednafen save {}", save.display()))?;
    let ram = state
        .main_ram()
        .with_context(|| format!("save state {} has no main RAM entry", save.display()))?;
    let game_mode = read_game_mode(ram);
    let resolved = game_mode.and_then(|b| GameMode::from_index(b as usize));
    let game_mode_name = resolved.map(|gm| game_mode_label(gm).to_string());
    let scene_mode = resolved
        .map(|gm| scene_mode_name(gm.scene_mode()).to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    let active_scene = read_pool_slot_name(ram, 0);
    Ok(ModeTraceFrame {
        frame: 0,
        game_mode,
        game_mode_name,
        scene_mode,
        active_scene,
    })
}

fn game_mode_label(gm: legaia_engine_core::mode::GameMode) -> &'static str {
    legaia_engine_core::mode::TABLE[gm.as_index()].name
}

/// Serialise a list of frames as JSON Lines (`\n`-terminated JSON objects,
/// one per frame). Round-trips through [`parse_mode_trace_jsonl`].
pub fn mode_trace_to_jsonl(frames: &[ModeTraceFrame]) -> String {
    let mut out = String::new();
    for f in frames {
        // `to_string` cannot fail for a flat owned struct with primitive
        // + String fields; unwrap is safe here.
        out.push_str(&serde_json::to_string(f).expect("ModeTraceFrame JSON serialise"));
        out.push('\n');
    }
    out
}

/// Parse JSONL emitted by [`mode_trace_to_jsonl`]. Blank lines are
/// skipped so concatenated streams parse cleanly.
pub fn parse_mode_trace_jsonl(s: &str) -> Result<Vec<ModeTraceFrame>> {
    let mut out = Vec::new();
    for (i, line) in s.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let frame: ModeTraceFrame = serde_json::from_str(trimmed)
            .with_context(|| format!("parse JSONL line {}: {trimmed}", i + 1))?;
        out.push(frame);
    }
    Ok(out)
}

/// First field on which `engine` and `retail` disagree. Compares only
/// the fields both emitters populate today (`scene_mode` +
/// `active_scene`); `game_mode` is engine-side `None` so it can't drive
/// a divergence yet. `frame` is informational - retail snapshots are
/// always `frame=0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeTraceDivergence {
    pub kind: DivergenceKind,
    pub engine: ModeTraceFrame,
    pub retail: ModeTraceFrame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DivergenceKind {
    SceneMode,
    ActiveScene,
}

/// Walk `engine` left-to-right; return the first frame where the
/// engine's `(scene_mode, active_scene)` disagrees with `retail`.
/// Returns `None` when at least one engine frame matches.
pub fn first_mode_trace_divergence(
    engine: &[ModeTraceFrame],
    retail: &ModeTraceFrame,
) -> Option<ModeTraceDivergence> {
    if engine.is_empty() {
        return None;
    }
    // Any frame that matches retail counts as convergence - the engine
    // settling later is fine. Surface the *last* frame's divergence if
    // no frame ever matches; that's the actionable mismatch.
    let matches = engine.iter().any(|e| frame_matches(e, retail));
    if matches {
        return None;
    }
    let last = engine.last().unwrap();
    let kind = if last.scene_mode != retail.scene_mode {
        DivergenceKind::SceneMode
    } else {
        DivergenceKind::ActiveScene
    };
    Some(ModeTraceDivergence {
        kind,
        engine: last.clone(),
        retail: retail.clone(),
    })
}

fn frame_matches(engine: &ModeTraceFrame, retail: &ModeTraceFrame) -> bool {
    if engine.scene_mode != retail.scene_mode {
        return false;
    }
    // If retail has a name, the engine must have the same one. If retail
    // didn't observe a name (unparseable pool slot), don't penalise the
    // engine for filling it in.
    match (&engine.active_scene, &retail.active_scene) {
        (_, None) => true,
        (Some(e), Some(r)) => e == r,
        (None, Some(_)) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(scene_mode: &str, active: Option<&str>) -> ModeTraceFrame {
        ModeTraceFrame {
            frame: 0,
            game_mode: None,
            game_mode_name: None,
            scene_mode: scene_mode.to_string(),
            active_scene: active.map(str::to_string),
        }
    }

    #[test]
    fn jsonl_roundtrip_engine_shape() {
        let frames = vec![
            ModeTraceFrame {
                frame: 0,
                game_mode: None,
                game_mode_name: None,
                scene_mode: "Field".into(),
                active_scene: Some("town01".into()),
            },
            ModeTraceFrame {
                frame: 1,
                game_mode: None,
                game_mode_name: None,
                scene_mode: "Field".into(),
                active_scene: Some("town01".into()),
            },
        ];
        let jsonl = mode_trace_to_jsonl(&frames);
        // One record per line, trailing newline.
        assert_eq!(jsonl.lines().count(), 2);
        let round = parse_mode_trace_jsonl(&jsonl).unwrap();
        assert_eq!(frames, round);
    }

    #[test]
    fn jsonl_roundtrip_retail_shape() {
        let f = ModeTraceFrame {
            frame: 0,
            game_mode: Some(0x1A),
            game_mode_name: Some("STR INIT".into()),
            scene_mode: "Cutscene".into(),
            active_scene: Some("map01".into()),
        };
        let jsonl = mode_trace_to_jsonl(std::slice::from_ref(&f));
        let round = parse_mode_trace_jsonl(&jsonl).unwrap();
        assert_eq!(round, vec![f]);
    }

    #[test]
    fn parser_skips_blank_lines() {
        let s = "\n{\"frame\":0,\"scene_mode\":\"Field\"}\n\n";
        let out = parse_mode_trace_jsonl(s).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].scene_mode, "Field");
    }

    #[test]
    fn divergence_returns_none_on_match() {
        let engine = vec![frame("Field", Some("town01"))];
        let retail = frame("Field", Some("town01"));
        assert!(first_mode_trace_divergence(&engine, &retail).is_none());
    }

    #[test]
    fn divergence_returns_none_on_retail_missing_scene_name() {
        // Retail couldn't parse the pool slot; the engine still
        // resolves it. Don't surface that as a divergence.
        let engine = vec![frame("Field", Some("town01"))];
        let retail = frame("Field", None);
        assert!(first_mode_trace_divergence(&engine, &retail).is_none());
    }

    #[test]
    fn divergence_surfaces_scene_mode_mismatch() {
        let engine = vec![frame("Field", Some("map01"))];
        let retail = frame("Cutscene", Some("map01"));
        let d = first_mode_trace_divergence(&engine, &retail).unwrap();
        assert_eq!(d.kind, DivergenceKind::SceneMode);
    }

    #[test]
    fn divergence_surfaces_active_scene_mismatch() {
        let engine = vec![frame("Field", Some("town01"))];
        let retail = frame("Field", Some("town0c"));
        let d = first_mode_trace_divergence(&engine, &retail).unwrap();
        assert_eq!(d.kind, DivergenceKind::ActiveScene);
    }

    #[test]
    fn divergence_returns_none_when_any_engine_frame_matches() {
        // Engine starts in Field, transitions to Cutscene; retail is
        // Cutscene. The match at frame 1 satisfies the oracle.
        let engine = vec![
            frame("Field", Some("map01")),
            frame("Cutscene", Some("map01")),
        ];
        let retail = frame("Cutscene", Some("map01"));
        assert!(first_mode_trace_divergence(&engine, &retail).is_none());
    }

    #[test]
    fn empty_engine_trace_returns_none() {
        let retail = frame("Field", Some("town01"));
        assert!(first_mode_trace_divergence(&[], &retail).is_none());
    }
}
