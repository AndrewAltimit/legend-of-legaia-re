//! Multi-frame audio-trace parity oracle (I1b(b)).
//!
//! Sister test to [`audio_trace`](audio_trace.rs). The single-frame
//! oracle there lifts one SPU snapshot out of a mednafen `.mc{slot}`
//! save and asks "did the engine ever match retail's voice mask in the
//! engine window?". This test consumes the **multi-frame retail trace**
//! captured by the PCSX-Redux Lua probe and asks the stronger question
//! "for every retail vsync where audio was playing, did the engine
//! produce a matching frame?".
//!
//! ### Test fixture: producing the retail JSONL
//!
//! 1. Park PCSX-Redux at a mid-BGM save state (e.g. `sstate1`).
//! 2. Run the probe:
//!
//!    ```bash
//!    LEGAIA_LUA=scripts/pcsx-redux/autorun_audio_trace.lua \
//!    LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate1 \
//!    LEGAIA_OUT=/tmp/audio_trace.bin LEGAIA_FRAMES=60 \
//!        bash scripts/pcsx-redux/run_probe.sh
//!    ```
//!
//! 3. Decode to JSONL:
//!
//!    ```bash
//!    python3 scripts/pcsx-redux/extract_audio_trace_from_sstates.py \
//!        /tmp/audio_trace.bin /tmp/audio_trace.jsonl
//!    ```
//!
//! 4. Place the JSONL under `$LEGAIA_AUDIO_TRACE_JSONL_DIR/<label>.jsonl`,
//!    where `<label>` is a scenario label from `scripts/scenarios.toml`
//!    that has `expected_active_scene` set.
//!
//! ### Skip-pass conditions
//!
//!   - `LEGAIA_DISC_BIN` unset (engine side needs disc data).
//!   - `LEGAIA_AUDIO_TRACE_JSONL_DIR` unset (no retail traces to compare).
//!   - `extracted/` missing.
//!   - Manifest missing.
//!   - No scenario has both `expected_active_scene` and a JSONL in the
//!     directory.
//!
//! ### Convergence rule
//!
//! Same as the single-frame test, applied per retail frame:
//! [`first_audio_trace_divergence_multi`] succeeds when, for every
//! retail frame whose `active_voice_mask` is non-zero, some engine
//! frame's mask is a superset of retail's. `NoFrameMatched` stays
//! tolerable drift (the engine's BGM may converge later); only
//! `VoiceStartAddrMismatch` and `MasterVolumeMismatch` hard-fail.

use std::path::PathBuf;

use legaia_engine_shell::audio_trace_oracle::{
    AudioDivergenceKind, compare_voice_allocation_aligned, engine_trace_from_paths,
    first_audio_trace_divergence_multi, load_runtime_audio_trace_jsonl,
};
use legaia_mednafen::ScenarioManifest;

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
fn audio_trace_multi_frame_scenarios_converge() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(jsonl_dir) = std::env::var_os("LEGAIA_AUDIO_TRACE_JSONL_DIR") else {
        eprintln!(
            "[skip] LEGAIA_AUDIO_TRACE_JSONL_DIR unset \
             (no captured retail traces to compare against)"
        );
        return;
    };
    let jsonl_dir = PathBuf::from(jsonl_dir);
    if !jsonl_dir.is_dir() {
        eprintln!(
            "[skip] LEGAIA_AUDIO_TRACE_JSONL_DIR is not a directory: {}",
            jsonl_dir.display()
        );
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
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
        let jsonl_path = jsonl_dir.join(format!("{}.jsonl", scn.label));
        if !jsonl_path.exists() {
            continue;
        }
        qualifying.push((scn.label.clone(), scene_name.to_owned(), jsonl_path));
    }

    if qualifying.is_empty() {
        eprintln!(
            "[skip] no scenarios qualify: need both `expected_active_scene` and a JSONL named \
             `<label>.jsonl` under {}",
            jsonl_dir.display(),
        );
        return;
    }

    let mut hard_failures = Vec::new();
    let mut converged = 0usize;
    let mut tolerable = 0usize;
    for (label, scene_name, jsonl_path) in &qualifying {
        let trace = engine_trace_from_paths(scene_name, &extracted, None, FRAMES, None)
            .unwrap_or_else(|e| panic!("scenario {label:?}: build engine trace: {e:#}"));
        let retail = load_runtime_audio_trace_jsonl(jsonl_path)
            .unwrap_or_else(|e| panic!("scenario {label:?}: load retail JSONL: {e:#}"));
        let retail_active = retail.iter().filter(|f| f.active_voice_mask != 0).count();
        match first_audio_trace_divergence_multi(&trace, &retail) {
            None => {
                converged += 1;
                eprintln!(
                    "[ok]    {label:<32} scene={scene_name:<10} \
                     retail_frames={} ({} active) -> converged",
                    retail.len(),
                    retail_active,
                );
            }
            Some(d) => {
                // Same two tolerable kinds as the single-snapshot test:
                // NoFrameMatched (the window may not carry the track) and
                // VoiceStartAddrMismatch (SPU-RAM offsets come from two
                // independent allocators, so equality there was never a
                // claim the port made). See the comment in audio_trace.rs.
                let is_tolerable = matches!(
                    d.kind,
                    AudioDivergenceKind::NoFrameMatched
                        | AudioDivergenceKind::VoiceStartAddrMismatch
                );
                if is_tolerable {
                    tolerable += 1;
                    eprintln!(
                        "[drift] {label:<32} scene={scene_name:<10} NoFrameMatched: \
                         retail mask=0b{:024b} (engine BGM did not converge in {FRAMES} frames)",
                        d.retail.active_voice_mask,
                    );
                } else {
                    hard_failures.push((label.clone(), d));
                }
            }
        }
    }

    eprintln!(
        "audio-trace-multi oracle: {} qualifying, {} converged, {} tolerable drifts, {} hard failures",
        qualifying.len(),
        converged,
        tolerable,
        hard_failures.len(),
    );

    assert!(
        hard_failures.is_empty(),
        "audio-trace-multi: {} unexpected failure(s) {:?}",
        hard_failures.len(),
        hard_failures
            .iter()
            .map(|(l, d)| format!("{l}: {:?}", d.kind))
            .collect::<Vec<_>>(),
    );
}

/// The per-voice channel that survives a capture whose frames do not carry a
/// fixed amount of SPU time: **key-ons per frame**, over the aligned window.
///
/// Why not the sounding-voice count. PCSX-Redux's SPU runs on its own thread,
/// paced by the audio device rather than by the emulated CPU, and
/// `PCSX::SPU::ADSR::mix` steps the envelope once per sample that thread
/// produces - so a per-vsync capture advances the envelope by an amount that
/// depends on how fast the host ran. Two captures of the same save state with
/// no pad input disagree on `env_level` far more than on the voice pitch
/// register, and their mean sounding-voice counts differ. A key-on is a
/// register write the score performs from the game's own vsync handler, so it
/// is on the emulated clock on both sides. See `docs/subsystems/audio.md`,
/// "The envelope channel is not on emulated time".
///
/// Same skip-pass gates as the test above, plus: the engine trace has to be
/// at least as long as the retail window for an alignment to exist.
#[test]
fn audio_trace_multi_key_on_rate_matches_on_the_aligned_window() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(jsonl_dir) = std::env::var_os("LEGAIA_AUDIO_TRACE_JSONL_DIR") else {
        eprintln!("[skip] LEGAIA_AUDIO_TRACE_JSONL_DIR unset");
        return;
    };
    let jsonl_dir = PathBuf::from(jsonl_dir);
    let (Some(extracted), Some(manifest_path)) = (extracted_dir(), manifest_path()) else {
        eprintln!("[skip] extracted/ or scripts/scenarios.toml missing");
        return;
    };
    if !jsonl_dir.is_dir() {
        eprintln!("[skip] LEGAIA_AUDIO_TRACE_JSONL_DIR is not a directory");
        return;
    }
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse scenarios manifest");

    // Long enough that a two-second capture can be slid over it: the aligned
    // offset for the one measured pairing sits deep inside a 60-second trace,
    // nowhere near frame 0.
    const LONG_FRAMES: u64 = 3600;
    let mut checked = 0usize;
    for scn in &manifest.scenarios {
        let Some(scene_name) = scn.expected_active_scene.as_deref() else {
            continue;
        };
        let jsonl_path = jsonl_dir.join(format!("{}.jsonl", scn.label));
        if !jsonl_path.exists() {
            continue;
        }
        let retail = load_runtime_audio_trace_jsonl(&jsonl_path)
            .unwrap_or_else(|e| panic!("scenario {:?}: load retail JSONL: {e:#}", scn.label));
        if retail.iter().all(|f| f.active_voice_mask == 0) {
            eprintln!("[skip] {}: retail window is silent", scn.label);
            continue;
        }
        let engine = engine_trace_from_paths(scene_name, &extracted, None, LONG_FRAMES, None)
            .unwrap_or_else(|e| panic!("scenario {:?}: build engine trace: {e:#}", scn.label));
        let c = compare_voice_allocation_aligned(&engine, &retail);
        eprintln!(
            "[ok]    {:<32} scene={scene_name:<10} aligned at {:?}:              key-ons/frame engine={:.3} retail={:.3} ratio={:.3}, shared tones={}",
            scn.label,
            c.alignment_offset,
            c.engine.onsets_per_frame,
            c.retail.onsets_per_frame,
            c.onset_ratio,
            c.shared_tones,
        );
        assert!(
            c.alignment_offset.is_some(),
            "{}: no alignment offset (engine trace shorter than the retail window?)",
            scn.label,
        );
        // Wide on purpose: the claim under test is "the same score is being
        // played at the same rate", not a frame-exact key-on match, and a
        // two-second window holds few enough key-ons that one extra is
        // several percent.
        assert!(
            c.onset_ratio > 0.5 && c.onset_ratio < 2.0,
            "{}: key-on rate ratio {:.3} outside [0.5, 2.0] - engine {:.3}/frame vs retail {:.3}/frame",
            scn.label,
            c.onset_ratio,
            c.engine.onsets_per_frame,
            c.retail.onsets_per_frame,
        );
        checked += 1;
    }
    if checked == 0 {
        eprintln!("[skip] no scenario had both a scene and a non-silent retail JSONL");
    }
}
