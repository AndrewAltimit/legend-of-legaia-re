//! Disc-gated mode-trace oracle (Phase E3).
//!
//! Sister test to [`vram_oracle_e1`]. For every scenario in
//! `scripts/scenarios.toml` that has BOTH an `expected_active_scene`
//! AND an on-disk `.mc{slot}` mednafen save, this test:
//!
//!   1. Boots a [`BootSession`](legaia_engine_shell::BootSession) on
//!      the resolved scene and ticks it `FRAMES` times, sampling
//!      `(scene_mode, active_scene)` per frame.
//!   2. Lifts a single `(game_mode, scene_mode, active_scene)` snapshot
//!      out of the matching mednafen `.mc{slot}` save.
//!   3. Asserts the engine's trace converges with the retail snapshot:
//!      at least one engine frame must match
//!      `(scene_mode, active_scene)`. The `game_mode` byte is engine-side
//!      `None` today (the port doesn't drive the 28-mode dispatcher),
//!      so it's informational, not assertional.
//!
//! Skip-pass cases (CLAUDE.md disc-gated convention):
//!   - `LEGAIA_DISC_BIN` unset.
//!   - `extracted/` missing.
//!   - `scripts/scenarios.toml` missing.
//!   - No scenario has both `expected_active_scene` and an on-disk
//!     `.mc{slot}` in the resolved `LEGAIA_MEDNAFEN_DIR` /
//!     `~/.mednafen/mcs` directory.
//!
//! Auto-discovery: scenarios opt in by populating
//! `expected_active_scene`. Adding new captures requires no test edits.

use std::path::PathBuf;

use legaia_engine_shell::mode_trace_oracle::{
    build_engine_mode_trace, first_mode_trace_divergence, load_runtime_mode_trace_from_save,
};
use legaia_mednafen::ScenarioManifest;

/// How many engine frames to tick before the comparison. 60 = one
/// retail second; enough for boot-time scene transitions (field-VM
/// FMV-trigger ops fire within a handful of frames) but cheap enough
/// to keep the test fast.
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
fn mode_trace_e3_all_scenarios_converge() {
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

    let mut failures = Vec::new();
    for (label, scene_name, save_path) in &qualifying {
        let trace = build_engine_mode_trace(scene_name, &extracted, None, FRAMES)
            .unwrap_or_else(|e| panic!("scenario {label:?}: build engine mode-trace: {e:#}"));
        let retail = load_runtime_mode_trace_from_save(save_path)
            .unwrap_or_else(|e| panic!("scenario {label:?}: load retail snapshot: {e:#}"));
        match first_mode_trace_divergence(&trace, &retail) {
            None => {
                eprintln!(
                    "[ok]    {label:<32} scene={scene_name:<10} converged scene_mode={} active_scene={:?}",
                    retail.scene_mode, retail.active_scene
                );
            }
            Some(d) => {
                eprintln!(
                    "[DRIFT] {label:<32} scene={scene_name:<10} {:?}: engine(scene_mode={}, active_scene={:?}) vs retail(scene_mode={}, active_scene={:?})",
                    d.kind,
                    d.engine.scene_mode,
                    d.engine.active_scene,
                    d.retail.scene_mode,
                    d.retail.active_scene,
                );
                failures.push(label.clone());
            }
        }
    }

    assert!(
        failures.is_empty(),
        "mode-trace oracle E3: {}/{} scenario(s) failed to converge with retail: {:?}",
        failures.len(),
        qualifying.len(),
        failures
    );
}
