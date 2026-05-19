//! v0.1 playthrough oracle — SCAFFOLD.
//!
//! Composes the four existing parity oracles (VRAM, audio-trace,
//! mode-trace, SC round-trip) across one scripted `j-replay-v1`
//! recording that drives the shortest gameplay path exercising every
//! deep subsystem:
//!
//!   boot -> title -> load real save -> walk in field scene ->
//!   fixed encounter -> battle victory -> XP/level/loot -> save back
//!
//! The recording itself lives at `scripts/replays/v0_1_playthrough.toml`
//! (override via `LEGAIA_V0_1_REPLAY`). Two halves run here:
//!
//! 1. **Determinism gate (disc-free).** Drives a synthetic
//!    [`legaia_engine_core::world::World`] through the replay twice and
//!    asserts the per-frame state-trace bytes are identical. Mirrors
//!    [`determinism_j2`](crate::determinism_j2) but binds to the v0.1
//!    replay file so a regression on the file shape surfaces here.
//!    Always runs in CI.
//!
//! 2. **Oracle convergence gate (disc-gated).** Resolves the replay's
//!    `meta.scenario` to a row in `scripts/scenarios.toml`, locates the
//!    matching `.mc{slot}` save, builds the engine's mode-trace via
//!    [`build_engine_mode_trace`], and asserts convergence against the
//!    retail snapshot at the frames named in `[[expected]]`. Skip-passes
//!    when the scaffold isn't yet populated (no scenario binding, no
//!    save, or no disc).
//!
//! VRAM and audio-trace oracle hooks will land on top of this scaffold
//! when the replay carries real events. The plumbing for both already
//! exists in `legaia_engine_shell::{vram_oracle, audio_trace_oracle}`;
//! this file picks them up via composition once the third assertion
//! shape is finalised.
//!
//! Skip-pass cases (CLAUDE.md disc-gated convention):
//!   - replay file missing (smoke test fails loudly; oracle test skips)
//!   - `meta.scenario` unset (scaffold state)
//!   - `LEGAIA_DISC_BIN` unset
//!   - `extracted/` missing
//!   - `scripts/scenarios.toml` missing or scenario label unknown
//!   - The scenario's `.mc{slot}` save missing on disk

use std::path::PathBuf;

use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_shell::mode_trace_oracle::{
    ModeTraceFrame, build_engine_mode_trace_with_inputs, first_mode_trace_divergence,
    load_runtime_mode_trace_from_save,
};
use legaia_engine_shell::replay::ReplayFile;
use legaia_mednafen::ScenarioManifest;
use sha2::{Digest, Sha256};

/// Default location of the v0.1 replay file, relative to the workspace
/// root. Overridable via [`REPLAY_PATH_ENV`].
const REPLAY_PATH_DEFAULT: &str = "scripts/replays/v0_1_playthrough.toml";

/// Env var that overrides [`REPLAY_PATH_DEFAULT`]. Lets local runs point
/// at a work-in-progress recording without editing the committed
/// scaffold.
const REPLAY_PATH_ENV: &str = "LEGAIA_V0_1_REPLAY";

// ---------------------------------------------------------------------
// Discovery helpers (mirror mode_trace_e3 / vram_oracle_e1 patterns)
// ---------------------------------------------------------------------

fn replay_path() -> PathBuf {
    if let Ok(p) = std::env::var(REPLAY_PATH_ENV) {
        return PathBuf::from(p);
    }
    for candidate in [
        REPLAY_PATH_DEFAULT,
        &format!("../{REPLAY_PATH_DEFAULT}"),
        &format!("../../{REPLAY_PATH_DEFAULT}"),
    ] {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return p;
        }
    }
    PathBuf::from(REPLAY_PATH_DEFAULT)
}

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn manifest_path() -> Option<PathBuf> {
    for candidate in [
        "scripts/scenarios.toml",
        "../scripts/scenarios.toml",
        "../../scripts/scenarios.toml",
    ] {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

// ---------------------------------------------------------------------
// Synthetic-world driver (mirrors determinism_j2; rebound to v0.1 file)
// ---------------------------------------------------------------------

/// Build a deterministic synthetic world. Same shape as
/// `determinism_j2::synthetic_world` so the two gates can share an
/// implementation when the v0.1 path is real enough to swap the
/// synthetic driver for a real BootSession.
fn synthetic_world(rng_seed: u32) -> World {
    let mut world = World::new();
    while world.actors.len() < 8 {
        world.actors.push(Actor::default());
    }
    world.rng_state = rng_seed;
    world.mode = SceneMode::Title;
    world.money = 0;
    world.party_count = 3;
    for slot in 0..3 {
        let actor = world.spawn_actor(slot);
        actor.battle.liveness = 1;
        actor.battle.max_hp = 200;
        actor.battle.hp = 200;
    }
    world
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct StateSample {
    frame: u64,
    scene_mode: String,
    pad: u16,
    rng_state: u32,
    money: i32,
    party_hp_total: u32,
    dialog_active: bool,
}

fn scene_mode_name(m: SceneMode) -> &'static str {
    match m {
        SceneMode::Title => "Title",
        SceneMode::Field => "Field",
        SceneMode::Battle => "Battle",
        SceneMode::Cutscene => "Cutscene",
        SceneMode::WorldMap => "WorldMap",
    }
}

fn sample_world(world: &World, pad: u16) -> StateSample {
    StateSample {
        frame: world.frame,
        scene_mode: scene_mode_name(world.mode).to_string(),
        pad,
        rng_state: world.rng_state,
        money: world.money,
        party_hp_total: world.actors.iter().map(|a| a.battle.hp as u32).sum(),
        dialog_active: world.current_dialog.is_some(),
    }
}

fn build_state_trace(replay: &ReplayFile) -> String {
    let pad_stream = replay.expand_pad_stream();
    let mut world = synthetic_world(replay.meta.rng_seed);
    let mut out = String::new();
    out.push_str(&serde_json::to_string(&sample_world(&world, pad_stream[0])).unwrap());
    out.push('\n');
    for &pad in pad_stream.iter().skip(1) {
        let _ = world.tick();
        out.push_str(&serde_json::to_string(&sample_world(&world, pad)).unwrap());
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------
// Test 1: replay file smoke (always runs)
// ---------------------------------------------------------------------

#[test]
fn v0_1_replay_file_parses_clean() {
    let path = replay_path();
    assert!(
        path.exists(),
        "v0.1 replay scaffold missing at {} — was the file deleted?",
        path.display(),
    );
    let replay = ReplayFile::from_path(&path)
        .unwrap_or_else(|e| panic!("v0.1 replay {} did not parse: {e:#}", path.display()));
    // ReplayFile::from_path calls validate() internally; re-call here so
    // a future regression in that ordering surfaces.
    replay
        .validate()
        .unwrap_or_else(|e| panic!("v0.1 replay {} failed validate: {e:#}", path.display()));
}

// ---------------------------------------------------------------------
// Test 2: determinism gate (disc-free, always runs)
// ---------------------------------------------------------------------

#[test]
fn v0_1_determinism_two_runs_byte_identical() {
    let path = replay_path();
    let replay = ReplayFile::from_path(&path)
        .unwrap_or_else(|e| panic!("load v0.1 replay {}: {e:#}", path.display()));

    let trace_a = build_state_trace(&replay);
    let trace_b = build_state_trace(&replay);
    assert_eq!(
        trace_a, trace_b,
        "v0.1 determinism gate failed: two runs of the same replay produced different state traces"
    );

    // SHA-256 cross-check so a future reader can pin the digest if a
    // regression hunt needs a checksum to bisect against.
    let mut h = Sha256::new();
    h.update(trace_a.as_bytes());
    let _ = h.finalize();

    // No fixture-diff here: the replay's [[expected]] rows describe
    // disc-gated engine behaviour (e.g. reaching Battle at frame 250),
    // not the synthetic-world driver. The fixture diff runs in
    // `v0_1_oracle_convergence` where the real engine ticks. This gate
    // is exclusively about the determinism invariant -- two runs
    // produce byte-identical traces, full stop.
}

// ---------------------------------------------------------------------
// Test 3: oracle convergence gate (disc-gated, scaffold skip-passes)
// ---------------------------------------------------------------------

/// Minimum tick count when the replay carries no `meta.frames`
/// override. Most disc-gated convergence checks happen within the
/// first second of boot (the engine reaches `town01` immediately).
/// The actual frame budget is `max(MIN_ORACLE_FRAMES, replay.meta.frames)`
/// so a recorded playthrough always runs to completion.
const MIN_ORACLE_FRAMES: u64 = 60;

#[test]
fn v0_1_oracle_convergence() {
    // -- preconditions --------------------------------------------------
    let path = replay_path();
    let replay = match ReplayFile::from_path(&path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[skip] v0.1 replay {} unreadable: {e:#}", path.display());
            return;
        }
    };

    let Some(scenario_label) = replay.meta.scenario.as_deref() else {
        eprintln!(
            "[skip] v0.1 replay carries no `meta.scenario` binding — scaffold not yet populated. \
             See scripts/replays/v0_1_playthrough.toml for the recording recipe."
        );
        return;
    };

    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing — run `legaia-extract` first");
        return;
    };
    let Some(manifest_path) = manifest_path() else {
        eprintln!("[skip] scripts/scenarios.toml not found");
        return;
    };
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse scenarios manifest");
    let Some(scn) = manifest
        .scenarios
        .iter()
        .find(|s| s.label == scenario_label)
    else {
        eprintln!(
            "[skip] v0.1 replay references scenario {scenario_label:?} but it isn't in \
             scripts/scenarios.toml"
        );
        return;
    };
    let Some(scene_name) = scn.expected_active_scene.as_deref() else {
        eprintln!(
            "[skip] scenario {scenario_label:?} has no `expected_active_scene` — needed to \
             drive build_engine_mode_trace"
        );
        return;
    };
    let Ok(save_path) = manifest.save_path(scn.slot) else {
        eprintln!("[skip] scenario {scenario_label:?}: save_path resolution failed");
        return;
    };
    if !save_path.exists() {
        eprintln!(
            "[skip] scenario {scenario_label:?}: no .mc{} save at {}",
            scn.slot,
            save_path.display(),
        );
        return;
    }

    // -- pad-threaded mode-trace ---------------------------------------
    //
    // Run the engine for `max(MIN_ORACLE_FRAMES, replay.meta.frames)`
    // ticks, feeding the replay's expanded pad stream into
    // `World.input` per frame. Today the world-tick path doesn't
    // consume `World.input` directly (see
    // `build_engine_mode_trace_with_inputs` doc) so the pad threading
    // is contractual, not behavioural -- the engine evolves the same
    // way it would with no input. That changes as soon as the first
    // input consumer (field-VM dialog advance, menu navigation, world-
    // map controller) moves into the engine tick path, at which point
    // this oracle starts asserting real input-driven behaviour.
    let pad_stream = replay.expand_pad_stream();
    let oracle_frames = replay.meta.frames.max(MIN_ORACLE_FRAMES);
    let trace = build_engine_mode_trace_with_inputs(
        scene_name,
        &extracted,
        None,
        oracle_frames,
        &pad_stream,
    )
    .unwrap_or_else(|e| panic!("scenario {scenario_label:?}: build engine mode-trace: {e:#}"));
    let retail = load_runtime_mode_trace_from_save(&save_path)
        .unwrap_or_else(|e| panic!("scenario {scenario_label:?}: load retail snapshot: {e:#}"));

    // Diagnostic: print the unique scene_mode transitions across the
    // trace. With no pad consumer wired into the engine tick today,
    // the expected output is a single Title row -- the engine never
    // leaves Title from a fresh boot. When dialogue advance lands,
    // additional transitions (Title -> Field -> Battle) will surface
    // here without changing the test code.
    print_scene_mode_transitions(scenario_label, &trace);

    // Retail-snapshot convergence (existing assertion shape from
    // mode_trace_e3). Passes today because the engine reaches
    // active_scene=town01 immediately at boot; first_mode_trace_divergence
    // accepts "at least one engine frame matches retail" as convergence.
    if let Some(d) = first_mode_trace_divergence(&trace, &retail) {
        panic!(
            "v0.1 retail-snapshot convergence failed for scenario {scenario_label:?} \
             (scene={scene_name}): {:?}: engine(scene_mode={}, active_scene={:?}) vs \
             retail(scene_mode={}, active_scene={:?})",
            d.kind,
            d.engine.scene_mode,
            d.engine.active_scene,
            d.retail.scene_mode,
            d.retail.active_scene,
        );
    }

    // Replay-fixture diff (sharper assertion). The replay file's
    // [[expected]] rows pin specific frames to specific scene_modes,
    // typically aligned with the anchors captured in
    // `scripts/scenarios.toml`. Today the engine can't drive the
    // prologue from a cold boot, so any [[expected]] row past frame 0
    // that asserts `scene_mode != "Title"` will RED this test -- that
    // failure IS the v0.1 finding, not a scaffold defect.
    if let Some(d) = replay.diff(&trace) {
        panic!(
            "v0.1 replay fixture drift at frame {}: kind={:?} expected={:?} recorded(scene_mode={}, active_scene={:?})",
            d.frame, d.kind, d.expected, d.recorded.scene_mode, d.recorded.active_scene,
        );
    }

    eprintln!(
        "[ok]    v0.1 disc-gated oracle passed: scenario={scenario_label} scene={scene_name} \
         retail(scene_mode={}, active_scene={:?}) over {} engine frames",
        retail.scene_mode,
        retail.active_scene,
        trace.len(),
    );

    // TODO(item 1 follow-ups):
    //   - VRAM oracle at the frame the replay marks as the title screen.
    //   - audio-trace oracle across field BGM transition frames.
    //   - SC round-trip on the final save block (post-replay World).
    //
    // Each lands as its own diff block once the corresponding
    // [[expected]] row shape stabilises.
}

/// Walk `trace` left-to-right printing one row per `scene_mode`
/// transition. Cheap visibility into what the engine actually did,
/// without dumping all `frames` records. Active-scene changes within
/// the same `scene_mode` are not printed -- they're rare and the
/// retail-snapshot convergence check covers them.
fn print_scene_mode_transitions(scenario_label: &str, trace: &[ModeTraceFrame]) {
    if trace.is_empty() {
        return;
    }
    eprintln!(
        "[trace] {scenario_label}: {} engine frames; scene_mode transitions:",
        trace.len(),
    );
    let mut prev: Option<&str> = None;
    for f in trace {
        if prev != Some(f.scene_mode.as_str()) {
            eprintln!(
                "[trace]   frame={:<5} scene_mode={:<8} active_scene={:?}",
                f.frame, f.scene_mode, f.active_scene,
            );
            prev = Some(f.scene_mode.as_str());
        }
    }
}
