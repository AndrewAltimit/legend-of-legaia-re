//! Disc-gated VRAM oracle (Phase E1) - **static-mask** parity.
//!
//! The retail VRAM in a save state is a live snapshot: a large fraction of the
//! texpage region is *dynamic / residual* state (animation frames, battle
//! leftovers, scroll position). Comparing two snapshots of the **same scene**
//! (town01 pre- vs post-battle) shows ~40% of the primary texture band differs
//! between them. So a stateless engine pre-pass can never be byte-exact against
//! a single snapshot; that earlier assertion was testing something physically
//! unachievable.
//!
//! This oracle instead asserts against the **static mask** - the pixels that
//! are identical across every same-scene snapshot, i.e. the scene's genuine
//! static VRAM. For each scene with >= 2 captures, it:
//!
//!   1. Builds the engine-side 1 MiB VRAM via the field-mode DMA-every-TIM
//!      pre-pass (the retail field loader uploads every scene TIM, not just
//!      the render-targeted subset).
//!   2. Lifts each snapshot's runtime VRAM via [`legaia_mednafen::PsxGpu`] and
//!      computes the static mask (words equal across all snapshots).
//!   3. Asserts the engine never uploads a **wrong** texel on a static pixel
//!      in the texpage region (`y >= 256`), excluding the runtime-managed
//!      NPC / character CLUT band (see [`NPC_CLUT_BAND_ROWS`]). The engine may
//!      be incomplete (it doesn't yet assemble every boot-resident texture),
//!      but where it uploads on a static pixel it must match retail exactly.
//!
//! Skip-pass cases (CLAUDE.md disc-gated convention):
//!   - `LEGAIA_DISC_BIN` unset.
//!   - `scripts/scenarios.toml` missing.
//!   - No scene has >= 2 qualifying captures (both `expected_active_scene`
//!     and a matching save on disk) to build a static mask from.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_shell::vram_oracle::{
    build_engine_vram_bytes_prepass, compute_static_mask, first_static_upload_divergence,
    load_runtime_vram_from_save,
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

    // Group qualifying captures by scene so we can build a static mask from
    // all snapshots of the same scene.
    let mut by_scene: BTreeMap<String, Vec<(String, PathBuf)>> = BTreeMap::new();
    for scn in &manifest.scenarios {
        let Some(scene_name) = scn.expected_active_scene.as_deref() else {
            continue;
        };
        let Ok(save_path) = manifest.mednafen_save_path(scn, library_dir().as_deref()) else {
            continue;
        };
        if !save_path.exists() {
            continue;
        }
        by_scene
            .entry(scene_name.to_owned())
            .or_default()
            .push((scn.label.clone(), save_path));
    }

    // Only scenes with >= 2 captures yield a usable static mask.
    let multi: Vec<(&String, &Vec<(String, PathBuf)>)> =
        by_scene.iter().filter(|(_, v)| v.len() >= 2).collect();
    if multi.is_empty() {
        eprintln!(
            "[skip] no scene has >= 2 captures (need a static mask): scenes seen = {:?}",
            by_scene
                .iter()
                .map(|(s, v)| (s.as_str(), v.len()))
                .collect::<Vec<_>>()
        );
        return;
    }

    let mut failures = Vec::new();
    for (scene_name, captures) in &multi {
        let engine_bytes = build_engine_vram_bytes_prepass(scene_name, &extracted, None)
            .unwrap_or_else(|e| panic!("scene {scene_name:?}: build engine VRAM: {e:#}"));
        let runtimes: Vec<Vec<u8>> = captures
            .iter()
            .map(|(label, path)| {
                load_runtime_vram_from_save(path).unwrap_or_else(|e| {
                    panic!("scene {scene_name:?} ({label:?}): load VRAM: {e:#}")
                })
            })
            .collect();
        let refs: Vec<&[u8]> = runtimes.iter().map(|v| v.as_slice()).collect();
        let mask = compute_static_mask(&refs);
        let static_words = mask.iter().filter(|&&b| b).count();
        // Diff the engine against the first snapshot; the static mask guarantees
        // every other snapshot holds the same value at the asserted pixels.
        match first_static_upload_divergence(&engine_bytes, refs[0], &mask) {
            None => {
                eprintln!(
                    "[ok]    scene={scene_name:<10} {} captures, {static_words} static words, engine uploads byte-exact on static mask",
                    captures.len()
                );
            }
            Some(d) => {
                eprintln!(
                    "[DRIFT] scene={scene_name:<10} row={} col={} engine=0x{:04X} runtime=0x{:04X}",
                    d.y, d.x, d.engine_word, d.runtime_word
                );
                failures.push((*scene_name).clone());
            }
        }
    }

    assert!(
        failures.is_empty(),
        "VRAM oracle E1 (static-mask): {}/{} scene(s) had a wrong static upload: {:?}",
        failures.len(),
        multi.len(),
        failures
    );
}
