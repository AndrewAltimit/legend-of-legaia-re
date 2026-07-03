//! Deterministic replay driver (`replay`) + synthetic mode-trace helpers.
//!
//! Mechanical split from `commands.rs` (behavior-preserving).

use super::*;

/// Drive a synthetic [`World`] from a [`ReplayFile`] and write the
/// resulting mode-trace JSONL. This mirrors the J2 determinism-gate
/// harness verbatim - the gate asserts byte-identity across two runs of
/// the same input, so the subcommand is just "the determinism gate's
/// driver, plus JSONL output".
///
/// `--strict` exits non-zero when the recorded trace disagrees with the
/// replay file's `[[expected]]` fixture; without it, divergence is
/// printed to stderr but doesn't fail.
pub(crate) fn cmd_replay(input: &Path, out: &Path, strict: bool) -> Result<()> {
    let replay = ReplayFile::from_path(input)?;
    let trace = synthetic_replay_trace(&replay);
    let jsonl = mode_trace_to_jsonl(&trace);
    let out_label = if out.as_os_str() == "-" {
        print!("{jsonl}");
        "<stdout>".to_string()
    } else {
        std::fs::write(out, jsonl.as_bytes())
            .with_context(|| format!("write replay trace JSONL to {}", out.display()))?;
        out.display().to_string()
    };
    eprintln!(
        "replay '{}' (frames={}, events={}, expected={}) -> {}",
        input.display(),
        replay.meta.frames,
        replay.events.len(),
        replay.expected.len(),
        out_label,
    );
    if let Some(d) = replay.diff(&trace) {
        let msg = format!(
            "[DRIFT] frame={} kind={:?}: expected(scene_mode={}, active_scene={:?}) vs recorded(scene_mode={}, active_scene={:?})",
            d.frame,
            d.kind,
            d.expected.scene_mode,
            d.expected.active_scene,
            d.recorded.scene_mode,
            d.recorded.active_scene,
        );
        if strict {
            anyhow::bail!("{msg}");
        }
        eprintln!("{msg}");
    } else if !replay.expected.is_empty() {
        eprintln!("[ok] recorded trace matches replay [[expected]] fixture");
    }
    Ok(())
}

/// Build the engine-side mode trace by driving a synthetic [`World`]
/// through `replay`'s frame count. Mirrors
/// `crates/engine-shell/tests/determinism_j2.rs::build_mode_trace` so
/// the subcommand's behaviour is the same the determinism gate tests.
fn synthetic_replay_trace(replay: &ReplayFile) -> Vec<ModeTraceFrame> {
    let pad_stream = replay.expand_pad_stream();
    let mut world = legaia_engine_core::world::World::new();
    while world.actors.len() < 8 {
        world
            .actors
            .push(legaia_engine_core::world::Actor::default());
    }
    world.rng_state = replay.meta.rng_seed;
    let mut out = Vec::with_capacity(pad_stream.len());
    out.push(synthetic_replay_sample(&world));
    for _ in pad_stream.iter().skip(1) {
        let _ = world.tick();
        out.push(synthetic_replay_sample(&world));
    }
    out
}

fn synthetic_replay_sample(world: &legaia_engine_core::world::World) -> ModeTraceFrame {
    ModeTraceFrame {
        frame: world.frame,
        game_mode: None,
        game_mode_name: None,
        scene_mode: synthetic_replay_scene_mode_name(world.mode).to_string(),
        active_scene: None,
    }
}

fn synthetic_replay_scene_mode_name(m: legaia_engine_core::world::SceneMode) -> &'static str {
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
        SceneMode::MuscleDome => "MuscleDome",
        SceneMode::Menu => "Menu",
    }
}
