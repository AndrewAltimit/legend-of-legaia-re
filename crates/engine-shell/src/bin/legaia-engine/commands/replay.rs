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
pub(crate) fn cmd_replay(
    input: &Path,
    out: &Path,
    strict: bool,
    packs: &[std::path::PathBuf],
) -> Result<()> {
    let replay = ReplayFile::from_path(input)?;
    let shelf = legaia_engine_shell::replay::PackShelf::from_paths(packs)?;
    let trace = synthetic_replay_trace(&replay, &shelf)?;
    // One JSON object per frame, the `mode-trace` JSONL shape.
    let mut jsonl = String::new();
    for f in &trace {
        jsonl.push_str(&serde_json::to_string(f).context("serialise a mode-trace frame")?);
        jsonl.push('\n');
    }
    let out_label = if out.as_os_str() == "-" {
        print!("{jsonl}");
        "<stdout>".to_string()
    } else {
        std::fs::write(out, jsonl.as_bytes())
            .with_context(|| format!("write replay trace JSONL to {}", out.display()))?;
        out.display().to_string()
    };
    eprintln!(
        "replay '{}' (frames={}, events={}, switches={}, expected={}) -> {}",
        input.display(),
        replay.meta.frames,
        replay.events.len(),
        replay.switches.len(),
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
///
/// Each frame applies the replay's `[[switch]]` rows for it (packs off
/// `shelf`), then the pad in force, then ticks. A pack map switch needs a
/// scene host and fails here.
fn synthetic_replay_trace(
    replay: &ReplayFile,
    shelf: &legaia_engine_shell::replay::PackShelf,
) -> Result<Vec<ModeTraceFrame>> {
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
    // Tick `i` runs frame `i`: its switches, then its pad.
    for (frame, &pad) in pad_stream.iter().enumerate().take(pad_stream.len() - 1) {
        legaia_engine_shell::replay::apply_switches_at(&mut world, replay, frame as u64, shelf)?;
        world.set_pad(pad);
        let _ = world.tick();
        out.push(synthetic_replay_sample(&world));
    }
    Ok(out)
}

fn synthetic_replay_sample(world: &legaia_engine_core::world::World) -> ModeTraceFrame {
    ModeTraceFrame {
        frame: world.frame,
        game_mode: None,
        game_mode_name: None,
        scene_mode: world.mode.name().to_string(),
        active_scene: None,
    }
}
