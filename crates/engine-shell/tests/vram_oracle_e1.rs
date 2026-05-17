//! Disc-gated VRAM oracle (Phase E1).
//!
//! For every scenario in `scripts/scenarios.toml` that has BOTH an
//! `expected_active_scene` AND an on-disk `.mc{slot}` mednafen save,
//! this test:
//!
//!   1. Builds the engine-side 1 MiB VRAM via the targeted-upload
//!      pre-pass over the resolved scene's `SceneResources`.
//!   2. Lifts the runtime 1 MiB VRAM out of the matching mednafen save
//!      via [`legaia_mednafen::PsxGpu`].
//!   3. Asserts byte-exact match in the texpage region (`y >= 256`).
//!      The framebuffer half (`y < 256`) is reported but not asserted;
//!      the engine port renders direct-to-wgpu, not to a simulated PSX
//!      framebuffer.
//!
//! Skip-pass cases (CLAUDE.md disc-gated convention):
//!   - `LEGAIA_DISC_BIN` unset.
//!   - `scripts/scenarios.toml` missing.
//!   - No manifest scenario has both `expected_active_scene` and a
//!     matching `.mc` save on disk in the resolved `LEGAIA_MEDNAFEN_DIR`
//!     / `~/.mednafen/mcs` directory.
//!
//! When at least one scenario qualifies, every qualifying scenario is
//! asserted byte-exact - any failure surfaces "row Y col X: engine=0x.."
//! diagnostic output. Discovered scenarios are not opt-in: this is the
//! "self-maintaining" auto-discover mode that catches engine drift as
//! new captures land.

use std::path::PathBuf;

use legaia_engine_shell::vram_oracle::{
    build_engine_vram_bytes_prepass, first_texpage_divergence, load_runtime_vram_from_save,
};
use legaia_mednafen::ScenarioManifest;

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
fn vram_oracle_e1_all_scenarios_byte_exact() {
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
        let engine_bytes = build_engine_vram_bytes_prepass(scene_name, &extracted, None)
            .unwrap_or_else(|e| panic!("scenario {label:?}: build engine VRAM: {e:#}"));
        let runtime_bytes = load_runtime_vram_from_save(save_path)
            .unwrap_or_else(|e| panic!("scenario {label:?}: load runtime VRAM: {e:#}"));
        match first_texpage_divergence(&engine_bytes, &runtime_bytes) {
            None => {
                eprintln!("[ok]    {label:<32} scene={scene_name:<10} byte-exact y>=256");
            }
            Some(d) => {
                eprintln!(
                    "[DRIFT] {label:<32} scene={scene_name:<10} row={} col={} engine=0x{:04X} runtime=0x{:04X}",
                    d.y, d.x, d.engine_word, d.runtime_word
                );
                failures.push(label.clone());
            }
        }
    }

    assert!(
        failures.is_empty(),
        "VRAM oracle E1: {}/{} scenario(s) diverged in texpage region: {:?}",
        failures.len(),
        qualifying.len(),
        failures
    );
}
