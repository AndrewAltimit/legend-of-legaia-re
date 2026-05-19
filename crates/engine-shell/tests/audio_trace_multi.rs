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
    AudioDivergenceKind, engine_trace_from_paths, first_audio_trace_divergence_multi,
    load_runtime_audio_trace_jsonl,
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
                let is_tolerable = matches!(d.kind, AudioDivergenceKind::NoFrameMatched);
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
