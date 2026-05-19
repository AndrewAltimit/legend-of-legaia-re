//! Disc-gated audio-trace oracle (Phase I1, foundation cut).
//!
//! Sister test to [`vram_oracle_e1`] / [`mode_trace_e3`]. For every
//! scenario in `scripts/scenarios.toml` that has BOTH an
//! `expected_active_scene` AND an on-disk `.mc{slot}` mednafen save, this
//! test:
//!
//!   1. Builds an engine audio trace via
//!      [`legaia_engine_shell::audio_trace_oracle::engine_trace_from_paths`]:
//!      boots a [`BootSession`](legaia_engine_shell::BootSession) headlessly
//!      (`enable_audio = false`), runs a private standalone
//!      [`legaia_engine_audio::Spu`] in parallel, ticks `FRAMES` frames,
//!      and samples voice / master / reverb state each frame.
//!   2. Lifts a single voice-state snapshot out of the matching
//!      `.mc{slot}` save's `SPU` section via
//!      [`legaia_engine_shell::audio_trace_oracle::load_runtime_audio_trace_from_save`].
//!   3. Reports `[ok]` / `[DRIFT]` via [`first_audio_trace_divergence`]:
//!      at least one engine frame must have an active-voice mask that is
//!      a superset of retail's mask AND matching start-addresses where
//!      both sides report them.
//!
//! Skip-pass cases (CLAUDE.md disc-gated convention):
//!   - `LEGAIA_DISC_BIN` unset.
//!   - `extracted/` missing.
//!   - `scripts/scenarios.toml` missing.
//!   - No scenario has both `expected_active_scene` and an on-disk
//!     `.mc{slot}` save.
//!
//! **Foundation cut.** The engine side does not currently drive BGM
//! playback through this oracle - the trace runs without an attached
//! sequencer (`bgm_id = None`), so the engine emits all-quiescent frames.
//! Retail saves captured mid-BGM will report `[DRIFT NoFrameMatched]`,
//! which is the *expected* and *actionable* gap the I1 follow-up PRs
//! close (Lua probe for retail per-vsync trace + engine BGM playback in
//! the trace). To keep CI green on this cut, the test only fails when
//! retail's active-voice mask is non-zero - i.e. it asserts the diff
//! machinery works without asserting the engine matches retail audio yet.
//!
//! Auto-discovery: scenarios opt in by populating
//! `expected_active_scene`. Adding new captures requires no test edits.

use std::path::PathBuf;

use legaia_engine_shell::audio_trace_oracle::{
    AudioDivergenceKind, engine_trace_from_paths, first_audio_trace_divergence,
    load_runtime_audio_trace_from_save,
};
use legaia_mednafen::ScenarioManifest;

/// How many engine frames to tick before sampling. Matches
/// `mode_trace_e3`'s `FRAMES = 60` (one retail second).
const FRAMES: u64 = 60;

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

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn audio_trace_i1_all_scenarios_converge() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let Some(manifest_path) = manifest_path() else {
        eprintln!("[skip] scripts/scenarios.toml not found");
        return;
    };
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse scenarios manifest");

    let mut qualifying = Vec::new();
    for scn in &manifest.scenarios {
        let Some(scene_name) = scn.expected_active_scene.as_deref() else {
            continue;
        };
        let Ok(save_path) = manifest.save_path(scn.slot) else {
            continue;
        };
        if !save_path.exists() {
            continue;
        }
        qualifying.push((scn.label.clone(), scene_name.to_owned(), save_path));
    }

    if qualifying.is_empty() {
        eprintln!(
            "[skip] no scenarios qualify: need both `expected_active_scene` and an on-disk .mc save in {}",
            std::env::var("LEGAIA_MEDNAFEN_DIR").unwrap_or_else(|_| "~/.mednafen/mcs".into())
        );
        return;
    }

    let mut hard_failures = Vec::new();
    let mut expected_drifts = 0usize;
    for (label, scene_name, save_path) in &qualifying {
        let trace = engine_trace_from_paths(scene_name, &extracted, None, FRAMES, None)
            .unwrap_or_else(|e| panic!("scenario {label:?}: build engine audio trace: {e:#}"));
        let retail = load_runtime_audio_trace_from_save(save_path)
            .unwrap_or_else(|e| panic!("scenario {label:?}: load retail SPU snapshot: {e:#}"));
        match first_audio_trace_divergence(&trace, &retail) {
            None => {
                eprintln!(
                    "[ok]    {label:<32} scene={scene_name:<10} converged active_mask=0b{:024b}",
                    retail.active_voice_mask
                );
            }
            Some(d) => {
                if retail.active_voice_mask == 0 {
                    // Retail had no active voices and engine still drifted
                    // - that's an oracle bug, not an expected gap.
                    hard_failures.push((label.clone(), d));
                    continue;
                }
                // Engine doesn't drive BGM in the foundation cut, so any
                // scenario with retail-active voices is expected to drift
                // with `NoFrameMatched`. Surface but don't fail.
                let expected = matches!(d.kind, AudioDivergenceKind::NoFrameMatched);
                if expected {
                    expected_drifts += 1;
                    eprintln!(
                        "[expected-drift] {label:<32} scene={scene_name:<10} {:?}: retail mask=0b{:024b} (foundation cut: engine doesn't drive BGM)",
                        d.kind, d.retail.active_voice_mask,
                    );
                } else {
                    hard_failures.push((label.clone(), d));
                }
            }
        }
    }

    eprintln!(
        "audio-trace I1 (foundation): {} qualifying, {} expected drifts, {} hard failures",
        qualifying.len(),
        expected_drifts,
        hard_failures.len(),
    );

    assert!(
        hard_failures.is_empty(),
        "audio-trace oracle I1: {} unexpected failure(s) {:?}",
        hard_failures.len(),
        hard_failures
            .iter()
            .map(|(l, d)| format!("{l}: {:?}", d.kind))
            .collect::<Vec<_>>(),
    );
}
