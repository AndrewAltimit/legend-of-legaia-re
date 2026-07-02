//! Mode-trace oracle plumbing shared between the `legaia-engine mode-trace`
//! subcommand and the disc-gated `mode_trace_e3` integration test.
//!
//! Mirrors the shape of [`crate::vram_oracle`] but the diff axis is the
//! engine's high-level dispatch state instead of VRAM bytes: per-frame
//! `(game_mode, scene_mode, active_scene)`. The retail side reads a
//! mednafen save state; the engine side ticks a [`BootSession`] and
//! samples each frame.
//!
//! **Asymmetry.** The engine port doesn't drive the full 28-mode
//! game-mode dispatcher (see `legaia_engine_core::mode::ModeDriver`) -
//! [`BootSession::tick`] goes through `SceneHost::tick`, which keeps
//! [`world.mode`](legaia_engine_core::world::SceneMode) up to date but
//! not the master game-mode word `_DAT_8007B83C`. Engine-sampled
//! [`ModeTraceFrame::game_mode`] is therefore `None` for most frames;
//! retail-sampled frames fill it from main RAM directly. The exception
//! is the mode the session models explicitly: while the
//! BootSession-hosted pause menu is open the engine emits `game_mode =
//! 0x17` (`CARD MODE` - the retail menu / memory-card per-frame mode).
//! [`first_mode_trace_divergence`] compares scene_mode + active_scene
//! always, and game_mode whenever both sides emit it.
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
    /// read directly. Engine: `Some(0x17)` while the BootSession-hosted
    /// pause menu is open (the modelled CARD per-frame mode); `None`
    /// otherwise until the port models the rest of the dispatcher.
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
/// Returns `frames + 1` records. No pad input is fed; the engine is
/// effectively idle except for self-driving tick logic (e.g. field-VM
/// scripts that run on their own without controller input). Callers
/// that need to thread scripted pad input use
/// [`build_engine_mode_trace_with_inputs`].
pub fn build_engine_mode_trace(
    scene_name: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    frames: u64,
) -> Result<Vec<ModeTraceFrame>> {
    build_engine_mode_trace_with_inputs(scene_name, extracted_root, disc, frames, &[])
}

/// Variant of [`build_engine_mode_trace`] that drives the
/// [`BootSession`] with a scripted pad timeline. `pad_stream[i]` is
/// installed onto `World.input` just before tick `i+1`; frame `0` is
/// sampled pre-tick with an all-zeros pad. `frames + 1` records are
/// returned, same shape as the no-input variant.
///
/// If `pad_stream` is shorter than `frames`, the tail is held at zero.
/// If longer, only the first `frames` entries are consumed.
///
/// `World::tick` consumes `World.input` for the modes whose consumer
/// lives in the engine tick today: the field-VM dialog-advance poll
/// (`SceneMode::Field`) and the world-map controller
/// (`SceneMode::WorldMap`). Modes whose input consumer is still
/// host-side (battle command selection, field locomotion) evolve as
/// they would with no input until those consumers move into the tick;
/// the pad threading here keeps the contract end-to-end so each new
/// consumer starts asserting real behavior the moment it lands.
pub fn build_engine_mode_trace_with_inputs(
    scene_name: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    frames: u64,
    pad_stream: &[u16],
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
    for i in 0..frames {
        let pad = pad_stream.get(i as usize).copied().unwrap_or(0);
        session.host.world.set_pad(pad);
        let _ = session.tick()?;
        out.push(sample_engine_frame(&session));
    }
    Ok(out)
}

/// Variant of [`build_engine_mode_trace_with_inputs`] that drops the
/// session into a *live field scene* before sampling - it calls
/// [`BootSession::enter_field_live`] (run record 0, install the encounter
/// table, arm the live loop) so the engine reaches `SceneMode::Field` the way
/// the windowed host does, instead of sitting in `Title` like the plain
/// `load_scene` path.
///
/// This is the v0.1 oracle's engine driver: from a cold boot it reaches
/// Field/town01 immediately, which converges against the retail mode-trace
/// snapshot of a field-phase save (`game_mode 0x03`). The scripted-encounter
/// Battle leg (which needs the scene's story state seeded so the trigger is
/// armed) is a separate milestone; this builder stops at Field.
pub fn build_engine_mode_trace_field_live(
    scene_name: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    frames: u64,
    pad_stream: &[u16],
) -> Result<Vec<ModeTraceFrame>> {
    let cfg = BootConfig {
        scene: scene_name.to_string(),
        enable_audio: false,
    };
    let mut session = match disc {
        Some(p) => BootSession::open_disc(p, &cfg)?,
        None => BootSession::open(extracted_root, &cfg)?,
    };
    session.enter_field_live(
        scene_name,
        &crate::boot::FieldLiveOpts {
            live_loop: true,
            ..Default::default()
        },
    )?;
    let mut out = Vec::with_capacity((frames as usize).saturating_add(1));
    out.push(sample_engine_frame(&session));
    for i in 0..frames {
        let pad = pad_stream.get(i as usize).copied().unwrap_or(0);
        session.host.world.set_pad(pad);
        let _ = session.tick()?;
        out.push(sample_engine_frame(&session));
    }
    Ok(out)
}

/// Drive a NEW GAME cold boot through the opening Rim Elm training-fight
/// transition, sampling `(scene_mode, active_scene)` each frame so the
/// resulting trace carries the Field → Battle flip.
///
/// This is the Battle-leg sibling of [`build_engine_mode_trace_field_live`]:
/// where that builder stops at Field, this one mirrors the proven
/// `v0_1_battle_leg_reaches_battle_from_new_game` test path so the v0.1
/// oracle can assert a *literal* `[[expected]]` Battle row in the replay
/// fixture, not just converge against the bracketing retail anchors. The
/// sequence is:
///
/// 1. [`BootSession::begin_new_game`] seeds the opening party (Vahn, the
///    game's pre-Tetsu story state - there is no earlier save).
/// 2. [`BootSession::enter_field_live`] drops into `scene_name` with the live
///    loop armed; the cold boot auto-installs the town's sparring carrier.
/// 3. A real field-interact op on the carrier's slot opens its dialogue; a
///    just-pressed confirm (Cross) advances/dismisses it, which engages the
///    scripted lone-Tetsu encounter and flips Field → Battle.
///
/// Sampled through [`BootSession::tick`] (so the trace is the same shape the
/// other builders emit). The Field → Battle transition is a scene-*mode* flip
/// on the same loaded scene, not a scene change, so `active_scene` stays
/// `scene_name` across the boundary. Returns `frames + 1` records; ticking
/// continues for the full budget after Battle is reached so a downstream
/// `[[expected]]` row can pin any post-transition frame.
pub fn build_engine_mode_trace_new_game_battle_leg(
    scene_name: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    frames: u64,
) -> Result<Vec<ModeTraceFrame>> {
    use legaia_engine_core::input::PadButton;

    let cfg = BootConfig {
        scene: scene_name.to_string(),
        enable_audio: false,
    };
    let mut session = match disc {
        Some(p) => BootSession::open_disc(p, &cfg)?,
        None => BootSession::open(extracted_root, &cfg)?,
    };
    session.begin_new_game();
    session.enter_field_live(
        scene_name,
        &crate::boot::FieldLiveOpts {
            live_loop: true,
            ..Default::default()
        },
    )?;

    // The cold boot installs exactly one scripted-encounter carrier slot
    // (the sparring partner). Drive its dialogue-accept op.
    let slot = {
        let mut slots: Vec<u8> = session
            .host
            .world
            .field_carrier_slots
            .keys()
            .copied()
            .collect();
        slots.sort_unstable();
        slots.first().copied().context(
            "scene installs no scripted-encounter carrier slot (cannot drive the Battle leg)",
        )?
    };

    let mut out = Vec::with_capacity((frames as usize).saturating_add(1));
    out.push(sample_engine_frame(&session));

    // A real field-interact (op 0x3E, op0<100) on the carrier's slot, then the
    // dialog-advance op (0x4C n5). Mirrors the battle-leg test bytecode.
    session
        .host
        .world
        .load_field_script(vec![0x3E, 0x05, slot, 0x4C, 0x54]);
    let cross = PadButton::Cross.mask();
    let down = PadButton::Down.mask();
    for i in 0..frames {
        // Tick 0 opens the dialogue (pad 0). The spar dialogue carries the
        // faithful 4-option picker (`World::carrier_menu`), whose index-2
        // "practice" option is the one that arms the fight - navigate the
        // cursor Down twice (releases in between: the menu keys off
        // just-pressed edges) before the Cross confirm. The transition
        // resolves on the following frames at pad 0.
        let pad = match i {
            1 | 3 => down,
            5 => cross,
            _ => 0,
        };
        session.host.world.set_pad(pad);
        let _ = session.tick()?;
        out.push(sample_engine_frame(&session));
        // Keep ticking after Battle is reached so the trace runs the full
        // budget; the SceneMode stays Battle (no further input flips it).
    }
    Ok(out)
}

fn sample_engine_frame(session: &BootSession) -> ModeTraceFrame {
    // The engine doesn't run the full 28-mode dispatcher, but the mode the
    // session models explicitly is reported: the BootSession-hosted pause
    // menu runs under the retail CARD per-frame mode (game_mode 0x17,
    // `GameMode::CardMode`).
    let game_mode = session
        .field_menu_is_open()
        .then_some(legaia_engine_core::mode::GameMode::CardMode);
    ModeTraceFrame {
        frame: session.frames,
        game_mode: game_mode.map(|gm| gm.as_index() as u8),
        game_mode_name: game_mode.map(|gm| game_mode_label(gm).to_string()),
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
        SceneMode::Dance => "Dance",
        SceneMode::Fishing => "Fishing",
        SceneMode::SlotMachine => "SlotMachine",
        SceneMode::BakaFighter => "BakaFighter",
        SceneMode::Menu => "Menu",
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

/// Number of leading main-RAM bytes the manifest's `ram_fingerprint_sha256`
/// hashes (the first 64 KiB - the scratch + low-globals window that settles
/// deterministically after a save-state load).
pub const RAM_FINGERPRINT_BYTES: usize = 0x10000;

/// Lowercase-hex SHA-256 of the first [`RAM_FINGERPRINT_BYTES`] of a mednafen
/// save state's main RAM. This is the same digest `scripts/manage-states.py`
/// records as a scenario's `ram_fingerprint_sha256`, computed directly from
/// the save state (no emulator re-run), so a live `.mc{slot}` can be checked
/// against the catalogued fingerprint to detect that it has been overwritten
/// and no longer holds the documented scenario.
pub fn save_ram_fingerprint(save: &Path) -> Result<String> {
    use legaia_mednafen::SaveState;
    use sha2::{Digest, Sha256};

    let state = SaveState::from_path(save)
        .with_context(|| format!("load mednafen save {}", save.display()))?;
    let ram = state
        .main_ram()
        .with_context(|| format!("save state {} has no main RAM entry", save.display()))?;
    let window = ram.get(..RAM_FINGERPRINT_BYTES).unwrap_or(ram);
    let digest = Sha256::digest(window);
    Ok(hex_lower(&digest))
}

fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
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

/// First field on which `engine` and `retail` disagree. Compares
/// `scene_mode` + `active_scene` always, and `game_mode` whenever both
/// emitters populate it (the engine fills it for the modes it models,
/// e.g. `0x17` while the pause menu is open). `frame` is informational -
/// retail snapshots are always `frame=0`.
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
    /// Scene mode + active scene agree but the raw game-mode byte (emitted
    /// by both sides) does not.
    GameMode,
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
    } else if !active_scene_matches(last, retail) {
        DivergenceKind::ActiveScene
    } else {
        DivergenceKind::GameMode
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
    // Compare the raw game-mode byte when both sides observed one (the
    // engine emits it only for the modes it models; retail always does).
    if let (Some(e), Some(r)) = (engine.game_mode, retail.game_mode)
        && e != r
    {
        return false;
    }
    active_scene_matches(engine, retail)
}

/// If retail has a scene name, the engine must have the same one. If retail
/// didn't observe a name (unparseable pool slot), don't penalise the engine
/// for filling it in.
fn active_scene_matches(engine: &ModeTraceFrame, retail: &ModeTraceFrame) -> bool {
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
    fn divergence_surfaces_game_mode_mismatch() {
        // Both sides emit a game-mode byte and disagree while the derived
        // scene mode + scene name agree (e.g. CARD INIT vs CARD MODE).
        let mut e = frame("Menu", Some("town01"));
        e.game_mode = Some(0x17);
        let mut r = frame("Menu", Some("town01"));
        r.game_mode = Some(0x16);
        let d = first_mode_trace_divergence(&[e], &r).unwrap();
        assert_eq!(d.kind, DivergenceKind::GameMode);
    }

    #[test]
    fn game_mode_compared_only_when_both_sides_emit_it() {
        // Engine-side None (an unmodelled mode) must not penalise an
        // otherwise-matching frame against a retail byte.
        let e = frame("Field", Some("town01"));
        let mut r = frame("Field", Some("town01"));
        r.game_mode = Some(0x03);
        assert!(first_mode_trace_divergence(&[e], &r).is_none());
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
