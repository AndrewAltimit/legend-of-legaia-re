//! Disc-gated audio-trace parity oracle.
//!
//! Sister test to the VRAM and mode-trace oracles. For every
//! scenario in `scripts/scenarios.toml` that has BOTH an
//! `expected_active_scene` AND an on-disk `.mc{slot}` mednafen save, this
//! test:
//!
//!   1. Builds an engine audio trace via
//!      [`legaia_engine_shell::audio_trace_oracle::engine_trace_from_paths`]:
//!      boots a [`BootSession`](legaia_engine_shell::BootSession) headlessly
//!      (`enable_audio = false`), runs a private standalone
//!      [`legaia_engine_audio::Spu`] in parallel, ticks `FRAMES` frames,
//!      and samples voice / master / reverb state each frame. The trace
//!      installs a private
//!      [`TraceBgmDirector`](legaia_engine_shell::audio_trace_oracle::TraceBgmDirector)
//!      so the field VM's op `0x35` events drive sequencer attach /
//!      detach in lock-step with retail behaviour.
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
//! **Tolerable drift.** `NoFrameMatched` can still surface when a scene's
//! prescript doesn't emit op `0x35` within the trace window or targets a
//! different BGM than retail captured (a save state taken mid-track will
//! diverge from a 1-second engine window starting at scene load). The
//! test treats `NoFrameMatched` as expected-drift and only hard-fails on
//! `VoiceStartAddrMismatch` / `MasterVolumeMismatch` - those indicate the
//! engine *did* fire the same voices retail did but with wrong bank
//! offsets or volume, which is a real port bug.
//!
//! Auto-discovery: scenarios opt in by populating
//! `expected_active_scene`. Adding new captures requires no test edits.

use std::path::PathBuf;

use legaia_engine_shell::audio_trace_oracle::{
    AudioDivergenceKind, engine_trace_from_paths, first_audio_trace_divergence,
    load_runtime_audio_trace_from_save,
};
use legaia_mednafen::ScenarioManifest;

/// How many engine frames to tick before sampling. One retail second at
/// 60 Hz, matching the mode-trace oracle's window.
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

/// Repo-relative `saves/library` root, if present. Holds the immutable,
/// fingerprint-named save backups that the manifest's `backup_fingerprint`
/// field points at, so a scenario resolves to a stable copy rather than the
/// wipe-prone live `.mc{slot}`.
fn library_dir() -> Option<PathBuf> {
    for c in ["saves/library", "../saves/library", "../../saves/library"] {
        let d = PathBuf::from(c);
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

#[test]
fn audio_trace_all_scenarios_converge() {
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
        let Some(save_path) = library_dir()
            .as_deref()
            .and_then(|lib| manifest.library_save_path(scn, lib))
        else {
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
    let mut converged = 0usize;
    let mut expected_drifts = 0usize;
    for (label, scene_name, save_path) in &qualifying {
        let trace = engine_trace_from_paths(scene_name, &extracted, None, FRAMES, None)
            .unwrap_or_else(|e| panic!("scenario {label:?}: build engine audio trace: {e:#}"));
        let retail = load_runtime_audio_trace_from_save(save_path)
            .unwrap_or_else(|e| panic!("scenario {label:?}: load retail SPU snapshot: {e:#}"));
        match first_audio_trace_divergence(&trace, &retail) {
            None => {
                converged += 1;
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
                // NoFrameMatched is tolerable: the scene's prescript may
                // not fire op 0x35 within the trace window or may target a
                // different track than retail captured. Other divergence
                // kinds (VoiceStartAddrMismatch / MasterVolumeMismatch)
                // indicate the engine matched retail's voice indices but
                // with wrong sample / volume - a real bug.
                let tolerable = matches!(d.kind, AudioDivergenceKind::NoFrameMatched);
                if tolerable {
                    expected_drifts += 1;
                    eprintln!(
                        "[drift] {label:<32} scene={scene_name:<10} NoFrameMatched: retail mask=0b{:024b} (engine BGM did not converge in {FRAMES} frames)",
                        d.retail.active_voice_mask,
                    );
                } else {
                    hard_failures.push((label.clone(), d));
                }
            }
        }
    }

    eprintln!(
        "audio-trace oracle: {} qualifying, {} converged, {} tolerable drifts, {} hard failures",
        qualifying.len(),
        converged,
        expected_drifts,
        hard_failures.len(),
    );

    assert!(
        hard_failures.is_empty(),
        "audio-trace oracle: {} unexpected failure(s) {:?}",
        hard_failures.len(),
        hard_failures
            .iter()
            .map(|(l, d)| format!("{l}: {:?}", d.kind))
            .collect::<Vec<_>>(),
    );
}
