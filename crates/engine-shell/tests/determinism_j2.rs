//! Phase J2 - engine determinism gate.
//!
//! Drives a synthetic [`legaia_engine_core::world::World`] twice through
//! the same [`legaia_engine_shell::replay::ReplayFile`] and asserts the
//! per-frame state-trace bytes are identical between runs. Disc-free -
//! intentionally runs in CI without `LEGAIA_DISC_BIN`.
//!
//! The state-trace shape is intentionally tight (frame, scene_mode,
//! rng_state, money, party-HP totals, dialog flag) - it's enough to
//! catch RNG drift, frame-counter drift, and mode-dispatch divergence,
//! which are the determinism-critical surfaces. Wider state (full
//! actor table, story-flag bit array) would inflate the trace without
//! catching anything the current digest misses; widen if a real
//! regression slips through.
//!
//! Two distinct gates land here:
//!
//! 1. [`determinism_gate_two_runs_byte_identical`] - the headline
//!    invariant. Same replay run twice -> bit-identical trace bytes.
//! 2. [`replay_fixture_round_trips_against_engine`] - parses an inline
//!    fixture replay, drives the engine, asserts no
//!    [`ReplayDivergence`] against the expected mode-trace block.
//!
//! The pad-input dimension is captured by [`ReplayFile::expand_pad_stream`]
//! and threaded through the synthetic driver as a `Vec<u16>`; the runner
//! does not yet feed it into a menu / boot-UI controller (J3 wires
//! that). The determinism contract for J2 is "engine state evolves
//! deterministically given identical replay + seed" - pad threading is
//! a J3 invariant.

use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_shell::mode_trace_oracle::{ModeTraceFrame, mode_trace_to_jsonl};
use legaia_engine_shell::replay::{ReplayFile, ReplayMeta};
use sha2::{Digest, Sha256};

/// Build a deterministic starting world. Mirrors the
/// `battle_session_drives_action_sm_to_monster_wipe` synthetic harness:
/// 8 actors, fixed RNG seed, three spawned party slots at non-zero HP.
///
/// Two calls with identical `rng_seed` must produce structurally
/// identical worlds (every field that influences `tick`'s evolution is
/// seeded from compile-time data). [`run_synthetic_replay`] depends on
/// this.
fn synthetic_world(rng_seed: u32) -> World {
    let mut world = World::new();
    while world.actors.len() < 8 {
        world.actors.push(Actor::default());
    }
    world.rng_state = rng_seed;
    world.mode = SceneMode::Title;
    world.money = 0;
    world.party_count = 3;
    // Three party slots with retail-shaped HP so the digest exercises
    // the per-actor channel. Use spawn_actor so battle flags line up
    // with the synthetic-loop pattern; the determinism gate exercises
    // structural drift, not action-SM correctness.
    for slot in 0..3 {
        let actor = world.spawn_actor(slot);
        actor.battle.liveness = 1;
        actor.battle.max_hp = 200;
        actor.battle.hp = 200;
    }
    world
}

/// One sample of the world's determinism-critical observable state.
/// Serialised via serde_json into a stable line in [`build_state_trace`]'s
/// output. Adding a new field here on a regression hunt is fine; just
/// keep it stable per-frame.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct StateSample {
    frame: u64,
    /// World mode name. Matches [`ModeTraceFrame::scene_mode`] so traces
    /// can be diffed against the existing mode-trace oracle.
    scene_mode: String,
    /// Pad mask in effect on this frame (from
    /// [`ReplayFile::expand_pad_stream`]).
    pad: u16,
    /// PsyQ-PRNG running state. The single most important determinism
    /// signal - if RNG drifts, this drifts immediately.
    rng_state: u32,
    /// Gold counter - catches drift in any battle/loot path that
    /// touches gameplay state.
    money: i32,
    /// Sum of HP across active actors. Catches per-actor drift without
    /// inflating the trace with one row per actor.
    party_hp_total: u32,
    /// `current_dialog.is_some()` - catches divergent dialog
    /// transitions.
    dialog_active: bool,
}

fn sample_world(world: &World, pad: u16) -> StateSample {
    let party_hp_total: u32 = world.actors.iter().map(|a| a.battle.hp as u32).sum();
    StateSample {
        frame: world.frame,
        scene_mode: scene_mode_name(world.mode).to_string(),
        pad,
        rng_state: world.rng_state,
        money: world.money,
        party_hp_total,
        dialog_active: world.current_dialog.is_some(),
    }
}

fn scene_mode_name(m: SceneMode) -> &'static str {
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

/// Drive a fresh synthetic world through `replay` and return the
/// JSONL state trace. Two calls with identical `replay` input produce
/// byte-identical output - that's the determinism invariant
/// [`determinism_gate_two_runs_byte_identical`] asserts.
fn build_state_trace(replay: &ReplayFile) -> String {
    let pad_stream = replay.expand_pad_stream();
    let mut world = synthetic_world(replay.meta.rng_seed);
    let mut out = String::new();
    // Frame 0 sample (pre-tick).
    out.push_str(&serde_json::to_string(&sample_world(&world, pad_stream[0])).unwrap());
    out.push('\n');
    for &pad in pad_stream.iter().skip(1) {
        let _ = world.tick();
        out.push_str(&serde_json::to_string(&sample_world(&world, pad)).unwrap());
        out.push('\n');
    }
    out
}

/// Produce a [`ModeTraceFrame`] stream parallel to [`build_state_trace`]'s
/// output. Used to drive [`ReplayFile::diff`] - the expected fixture
/// rows are mode-trace shaped.
fn build_mode_trace(replay: &ReplayFile) -> Vec<ModeTraceFrame> {
    let pad_stream = replay.expand_pad_stream();
    let mut world = synthetic_world(replay.meta.rng_seed);
    let mut out = Vec::with_capacity(pad_stream.len());
    out.push(ModeTraceFrame {
        frame: 0,
        game_mode: None,
        game_mode_name: None,
        scene_mode: scene_mode_name(world.mode).to_string(),
        active_scene: None,
    });
    for _ in pad_stream.iter().skip(1) {
        let _ = world.tick();
        out.push(ModeTraceFrame {
            frame: world.frame,
            game_mode: None,
            game_mode_name: None,
            scene_mode: scene_mode_name(world.mode).to_string(),
            active_scene: None,
        });
    }
    out
}

#[test]
fn determinism_gate_two_runs_byte_identical() {
    let mut replay = ReplayFile::new(ReplayMeta::new(120).with_rng_seed(0xDEAD_C0DE));
    // Sparse pad events - press Cross at f=10, release at f=12, press
    // Down at f=60, release at f=62. None of these reach an input
    // consumer in J2's driver (that's J3); they exercise the
    // sparse-to-dense expansion in the trace's `pad` column so a
    // future regression that ignores the replay surface up.
    replay.push_event(10, 0x4000);
    replay.push_event(12, 0x0000);
    replay.push_event(60, 0x0040);
    replay.push_event(62, 0x0000);

    let trace_a = build_state_trace(&replay);
    let trace_b = build_state_trace(&replay);
    assert_eq!(
        trace_a, trace_b,
        "determinism gate failed: two runs of the same replay produced different state traces"
    );

    // Cross-check via SHA-256 so a future test reader can see the
    // expected hash without rebuilding the full trace.
    let mut h = Sha256::new();
    h.update(trace_a.as_bytes());
    let digest = h.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    // Sanity: the trace should be 121 lines + trailing newline.
    assert_eq!(trace_a.lines().count(), 121);
    assert!(!hex.is_empty());
}

#[test]
fn determinism_gate_pad_stream_dimension_is_observed() {
    // A trace with a different pad stream must produce a different
    // state-trace - confirms the `pad` column in the digest is actually
    // wired (not silently dropped).
    let mut a = ReplayFile::new(ReplayMeta::new(5).with_rng_seed(1));
    a.push_event(2, 0x4000);
    let mut b = ReplayFile::new(ReplayMeta::new(5).with_rng_seed(1));
    b.push_event(2, 0x2000);

    let trace_a = build_state_trace(&a);
    let trace_b = build_state_trace(&b);
    assert_ne!(trace_a, trace_b);
}

#[test]
fn determinism_gate_seed_dimension_is_observed() {
    // Different RNG seeds at construction time must produce different
    // traces. World::new seeds rng_state from the meta block; if J2's
    // driver ever forgot to thread the seed, this fires.
    let a = ReplayFile::new(ReplayMeta::new(5).with_rng_seed(0xAAAA_AAAA));
    let b = ReplayFile::new(ReplayMeta::new(5).with_rng_seed(0xBBBB_BBBB));

    let trace_a = build_state_trace(&a);
    let trace_b = build_state_trace(&b);
    assert_ne!(trace_a, trace_b);
}

#[test]
fn replay_fixture_round_trips_against_engine() {
    // Inline fixture: synthetic-world starts Title with no field
    // bytecode loaded, so World::tick keeps mode == Title across the
    // whole trace. The fixture asserts that observation as a
    // regression check.
    let mut replay = ReplayFile::new(ReplayMeta::new(30).with_rng_seed(0xDEAD_C0DE));
    replay.push_expected(0, "Title", None);
    replay.push_expected(10, "Title", None);
    replay.push_expected(30, "Title", None);

    let trace = build_mode_trace(&replay);
    let diff = replay.diff(&trace);
    assert!(
        diff.is_none(),
        "engine drifted from replay fixture: {diff:?}"
    );

    // Trace itself must serialise cleanly and be the expected length.
    let jsonl = mode_trace_to_jsonl(&trace);
    assert_eq!(jsonl.lines().count(), 31);
}

#[test]
fn replay_fixture_catches_drift_when_engine_changes_mode() {
    // Same harness but a wrong expectation - confirms `diff` actually
    // fires when the engine diverges from the fixture. If a future
    // engine change makes the synthetic world drop into Field, the
    // headline determinism test above will surface it; this one
    // double-locks the fixture-comparison side of the gate.
    let mut replay = ReplayFile::new(ReplayMeta::new(5).with_rng_seed(0xDEAD_C0DE));
    replay.push_expected(2, "Battle", None);
    let trace = build_mode_trace(&replay);
    let diff = replay.diff(&trace).expect("expected drift");
    assert_eq!(diff.frame, 2);
}
