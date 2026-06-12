//! Disc-gated PCM-window parity oracle.
//!
//! Third axis of the engine-vs-retail parity stack (after the VRAM and
//! mode-trace oracles). For every scenario in `scripts/scenarios.toml`
//! that has BOTH an `expected_active_scene` AND an on-disk `.mc{slot}`
//! mednafen save, this test:
//!
//!   1. Renders a retail-side reference PCM window via
//!      [`legaia_engine_shell::pcm_oracle::retail_reference_pcm`]: lifts
//!      the save's SPU section through
//!      [`legaia_engine_shell::pcm_oracle::engine_spu_from_retail`] and
//!      ticks the engine mixer for [`SAMPLES_PER_CHANNEL`] stereo
//!      samples.
//!   2. Boots a headless engine in parallel via
//!      [`legaia_engine_shell::pcm_oracle::build_engine_pcm_trace`] for
//!      [`FRAMES`] engine frames, routing field-VM op `0x35` BGM events
//!      through a private [`TraceBgmDirector`]. The trace produces an
//!      interleaved L,R PCM buffer covering the boot window.
//!   3. Compares [`legaia_engine_shell::pcm_oracle::pcm_stats`] between
//!      the two windows. Hard-fails only on a single condition: retail
//!      had audibly non-zero output while the engine produced complete
//!      silence over its entire trace window. Anything else - amplitude
//!      drift, envelope shape divergence, cross-fade timing mismatch -
//!      is treated as tolerable for now, matching the
//!      `NoFrameMatched`-as-soft-drift policy in
//!      `crates/engine-shell/tests/audio_trace.rs`.
//!
//! Skip-pass cases (CLAUDE.md disc-gated convention):
//!   - `LEGAIA_DISC_BIN` unset.
//!   - `extracted/` missing.
//!   - `scripts/scenarios.toml` missing.
//!   - No scenario has both `expected_active_scene` and an on-disk save.
//!
//! Auto-discovery: scenarios opt in by populating
//! `expected_active_scene` *and* having an on-disk `.mc{slot}` save in
//! `~/.mednafen/mcs/` (or `LEGAIA_MEDNAFEN_DIR`).

use std::path::PathBuf;

use legaia_engine_shell::audio_trace_oracle::AudioTraceBuildOptions;
use legaia_engine_shell::pcm_oracle::{
    SPU_SAMPLE_RATE, build_engine_pcm_trace, pcm_stats, retail_reference_pcm,
};
use legaia_mednafen::ScenarioManifest;

/// How many engine frames to tick before sampling. One retail second at
/// 60 Hz, matching the audio-trace oracle's window.
const FRAMES: u64 = 60;

/// How many stereo samples to render in the retail reference window.
/// One second at the SPU's internal 44.1 kHz rate, sized to match the
/// engine's per-frame render output across the [`FRAMES`] window.
const SAMPLES_PER_CHANNEL: usize = SPU_SAMPLE_RATE as usize;

/// Lower bound on retail RMS that we treat as "audibly non-zero". Below
/// this the test soft-passes regardless of engine output (the snapshot
/// was at a quiet moment / mid-fade-out).
const RETAIL_AUDIBLE_RMS: i32 = 256;

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
fn pcm_oracle_all_scenarios_engine_not_silent_when_retail_audible() {
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
    let mut soft_drifts = 0usize;
    let mut retail_quiet = 0usize;

    for (label, scene_name, save_path) in &qualifying {
        let retail_pcm = retail_reference_pcm(save_path, SAMPLES_PER_CHANNEL)
            .unwrap_or_else(|e| panic!("scenario {label:?}: load retail PCM: {e:#}"));
        let retail_stats = pcm_stats(&retail_pcm);

        let opts = AudioTraceBuildOptions {
            scene: scene_name.clone(),
            bgm_id: None,
            us_per_frame: 1_000_000.0 / 60.0,
            frames: FRAMES,
        };
        let engine_trace = build_engine_pcm_trace(&extracted, None, &opts)
            .unwrap_or_else(|e| panic!("scenario {label:?}: build engine PCM trace: {e:#}"));
        let engine_stats = pcm_stats(&engine_trace.pcm);

        // Retail had no audible output → trivially passes regardless of
        // engine. Save state may have been taken in a silent moment.
        if retail_stats.rms < RETAIL_AUDIBLE_RMS {
            retail_quiet += 1;
            eprintln!(
                "[quiet] {label:<32} scene={scene_name:<10} retail_rms={} (below {RETAIL_AUDIBLE_RMS}), engine_rms={}",
                retail_stats.rms, engine_stats.rms,
            );
            continue;
        }

        // Retail audible AND engine silent → real port bug (BGM never
        // started, sample upload failed, voices stuck in Off).
        if engine_stats.rms == 0 {
            hard_failures.push((label.clone(), retail_stats, engine_stats));
            continue;
        }

        // Both sides audible. We don't enforce byte-exact or RMS-within-
        // tolerance yet (mixer divergence + cross-fade alignment make
        // that premature). Report and move on.
        if engine_stats.rms < retail_stats.rms / 8 {
            soft_drifts += 1;
            eprintln!(
                "[drift] {label:<32} scene={scene_name:<10} retail_rms={} engine_rms={} (engine much quieter than retail)",
                retail_stats.rms, engine_stats.rms,
            );
        } else {
            converged += 1;
            eprintln!(
                "[ok]    {label:<32} scene={scene_name:<10} retail_rms={} engine_rms={} peaks=(retail={}, engine={})",
                retail_stats.rms, engine_stats.rms, retail_stats.peak_abs, engine_stats.peak_abs,
            );
        }
    }

    eprintln!(
        "pcm oracle: {} qualifying, {} converged, {} soft drifts, {} retail quiet, {} hard failures",
        qualifying.len(),
        converged,
        soft_drifts,
        retail_quiet,
        hard_failures.len(),
    );

    assert!(
        hard_failures.is_empty(),
        "pcm oracle: {} scenario(s) had retail audible but engine produced complete silence over {FRAMES} frames: {:?}",
        hard_failures.len(),
        hard_failures
            .iter()
            .map(|(l, r, e)| format!("{l}: retail_rms={} engine_rms={}", r.rms, e.rms))
            .collect::<Vec<_>>(),
    );
}
