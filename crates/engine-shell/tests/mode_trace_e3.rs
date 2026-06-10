//! Disc-gated mode-trace oracle (Phase E3).
//!
//! Sister test to [`vram_oracle_e1`]. For every scenario in
//! `scripts/scenarios.toml` that has BOTH an `expected_active_scene`
//! AND an on-disk `.mc{slot}` mednafen save, this test:
//!
//!   1. Boots a [`BootSession`](legaia_engine_shell::BootSession) on
//!      the resolved scene and ticks it `FRAMES` times, sampling
//!      `(scene_mode, active_scene)` per frame. `phase = "menu"`
//!      scenarios additionally thread a scripted Start press so the
//!      session opens the field pause menu headlessly
//!      (`BootSession::open_field_menu`, the retail CARD pair).
//!   2. Lifts a single `(game_mode, scene_mode, active_scene)` snapshot
//!      out of the matching mednafen `.mc{slot}` save.
//!   3. Asserts the engine's trace converges with the retail snapshot:
//!      at least one engine frame must match
//!      `(scene_mode, active_scene)` - and `game_mode` whenever both
//!      sides emit it (the engine fills it for the modes it models,
//!      e.g. `0x17` while the pause menu is open; retail menu saves
//!      hold the same byte, so menu scenarios assert full menu-mode
//!      convergence, not just the active scene).
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
    build_engine_mode_trace_field_live, first_mode_trace_divergence,
    load_runtime_mode_trace_from_save, save_ram_fingerprint,
};
use legaia_mednafen::ScenarioManifest;

/// How many engine frames to tick before the comparison. 60 = one
/// retail second; enough for boot-time scene transitions (field-VM
/// FMV-trigger ops fire within a handful of frames) but cheap enough
/// to keep the test fast.
const FRAMES: u64 = 60;

/// PSX pad Start bit (`PadButton::Start`), threaded into the pad stream of
/// `phase = "menu"` scenarios so the session opens the pause menu.
const START_MASK: u16 = legaia_engine_core::input::PadButton::Start as u16;

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
/// field points at.
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
    let library = library_dir();

    // Resolve each qualifying scenario's save, preferring the immutable
    // library backup over the wipe-prone live slot, then drop any whose
    // resolved save no longer matches the catalogued `ram_fingerprint_sha256`
    // (a live `.mc{slot}` that's been overwritten). Trusting a drifted slot
    // would compare the engine against an arbitrary save, so it's skipped
    // rather than failed - the same convention `manage-states.py validate`
    // uses for live-slot drift.
    let mut qualifying = Vec::new();
    let mut drifted = Vec::new();
    for scn in &manifest.scenarios {
        let Some(scene_name) = scn.expected_active_scene.as_deref() else {
            continue;
        };
        let Ok(save_path) = manifest.mednafen_save_path(scn, library.as_deref()) else {
            continue;
        };
        if !save_path.exists() {
            continue;
        }
        // Fingerprint gate: only run scenarios whose resolved save matches the
        // documented RAM fingerprint (when one is recorded).
        if let Some(expected) = scn.ram_fingerprint_sha256.as_deref() {
            match save_ram_fingerprint(&save_path) {
                Ok(actual) if actual.eq_ignore_ascii_case(expected) => {}
                Ok(actual) => {
                    eprintln!(
                        "[drift] {:<32} {} != catalogued {} - save slot overwritten, skipping",
                        scn.label,
                        &actual[..16.min(actual.len())],
                        &expected[..16.min(expected.len())]
                    );
                    drifted.push(scn.label.clone());
                    continue;
                }
                Err(e) => {
                    eprintln!("[drift] {:<32} fingerprint read failed: {e:#}", scn.label);
                    drifted.push(scn.label.clone());
                    continue;
                }
            }
        }
        qualifying.push((
            scn.label.clone(),
            scene_name.to_owned(),
            save_path,
            scn.phase.clone(),
        ));
    }

    if qualifying.is_empty() {
        eprintln!(
            "[skip] no scenarios qualify: need `expected_active_scene` + a fingerprint-matched save (live mcs dir {}; {} drifted slot(s) skipped: {:?})",
            std::env::var("LEGAIA_MEDNAFEN_DIR").unwrap_or_else(|_| "~/.mednafen/mcs".into()),
            drifted.len(),
            drifted,
        );
        return;
    }

    let mut failures = Vec::new();
    for (label, scene_name, save_path, phase) in &qualifying {
        // All `expected_active_scene` scenarios are field scenes, so drive the
        // engine into the field the way the windowed host does (cold boot ->
        // enter_field_live -> Field) rather than letting it sit in Title.
        // `phase = "menu"` captures hold the field PAUSE MENU open, which
        // retail runs under game_mode 0x17 (CARD MODE, 23 — the menu/memory-
        // card overlay's per-frame mode; all six library menu saves read 0x17
        // at 0x8007B83C). BootSession hosts the same field-menu runtime
        // headlessly (Start-edge path in `BootSession::tick` →
        // `open_field_menu`), so these scenarios drive a scripted Start press
        // and must converge on the full menu state: SceneMode::Menu + the
        // same active scene + game_mode 0x17 (compared whenever both sides
        // emit the byte).
        let menu_scenario = phase.as_deref() == Some("menu");
        let pad_stream: &[u16] = if menu_scenario {
            // pad_stream[i] lands before tick i+1: tick 1 idles (pad 0),
            // tick 2 sees the Start edge and opens the menu, the remaining
            // frames hold it open at pad 0.
            &[0, START_MASK]
        } else {
            &[]
        };
        let trace =
            build_engine_mode_trace_field_live(scene_name, &extracted, None, FRAMES, pad_stream)
                .unwrap_or_else(|e| panic!("scenario {label:?}: build engine mode-trace: {e:#}"));
        let retail = load_runtime_mode_trace_from_save(save_path)
            .unwrap_or_else(|e| panic!("scenario {label:?}: load retail snapshot: {e:#}"));
        match first_mode_trace_divergence(&trace, &retail) {
            None => {
                eprintln!(
                    "[ok]    {label:<32} scene={scene_name:<10} converged scene_mode={} active_scene={:?} game_mode={:?}",
                    retail.scene_mode, retail.active_scene, retail.game_mode
                );
            }
            Some(d) => {
                eprintln!(
                    "[DRIFT] {label:<32} scene={scene_name:<10} {:?}: engine(scene_mode={}, active_scene={:?}, game_mode={:?}) vs retail(scene_mode={}, active_scene={:?}, game_mode={:?})",
                    d.kind,
                    d.engine.scene_mode,
                    d.engine.active_scene,
                    d.engine.game_mode,
                    d.retail.scene_mode,
                    d.retail.active_scene,
                    d.retail.game_mode,
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
