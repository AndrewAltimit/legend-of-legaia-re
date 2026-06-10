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
    NPC_CLUT_BAND_ROWS, TEXPAGE_Y_START, VRAM_HEIGHT, VRAM_WIDTH, build_engine_vram_bytes_prepass,
    clear_world_map_clut_cycle_rows, compute_static_mask, first_static_upload_divergence,
    load_runtime_vram_from_save, refine_mask_with_shared_band,
};
use legaia_mednafen::ScenarioManifest;

/// A scene only yields a trustworthy static mask when its captures are
/// genuinely different same-scene states. If more than this fraction of the
/// non-zero texpage band is identical across the captures, they are too similar
/// (e.g. a before/after pair seconds apart) and the mask sweeps shared residual
/// in; such a scene is skipped. town01's pre/post-battle pair sits at ~60%.
const MAX_STATIC_FRACTION: f64 = 0.80;

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

    // The shared effect-texture band (befect_data) is one global disc source
    // resident across every field scene, with a few history-dependent pixels
    // (battle entry re-uploads the disc bytes over a boot-resident variant).
    // A per-scene mask misclassifies those as static when a scene's captures
    // share battle history, so pool EVERY capture - across all scenes, the
    // single-capture ones included - as cross-scene dynamism evidence for
    // cells inside the band's rects.
    let all_runtimes: Vec<Vec<u8>> = by_scene
        .values()
        .flatten()
        .map(|(label, path)| {
            load_runtime_vram_from_save(path)
                .unwrap_or_else(|e| panic!("({label:?}): load VRAM: {e:#}"))
        })
        .collect();
    let all_refs: Vec<&[u8]> = all_runtimes.iter().map(|v| v.as_slice()).collect();
    let shared_band_rects = {
        let prot = std::fs::read(extracted.join("PROT.DAT")).expect("read PROT.DAT");
        let cdname =
            std::fs::read_to_string(extracted.join("CDNAME.TXT")).expect("read CDNAME.TXT");
        let index = legaia_engine_core::scene::ProtIndex::from_bytes(prot, Some(&cdname))
            .expect("build ProtIndex");
        legaia_engine_core::scene::effect_texture_image_rects(&index)
            .expect("befect effect-texture rects")
    };

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
        let mut mask = compute_static_mask(&refs);
        refine_mask_with_shared_band(&mut mask, &shared_band_rects, &all_refs);
        // World-map scenes: the walk-view runtime animates segments of the
        // kingdom terrain CLUT rows (palette cycling), so their resident
        // words are animation phase, not static texture.
        if legaia_engine_core::scene::is_world_map_scene(scene_name) {
            clear_world_map_clut_cycle_rows(&mut mask);
        }

        // The static mask is only trustworthy when the captures are genuinely
        // DIFFERENT same-scene states (town01 pre/post-battle differs ~40% of
        // the texpage band -> ~60% static). Two near-simultaneous captures of
        // the same moment (e.g. a before/after-opening-a-chest pair seconds
        // apart) barely differ, so almost the whole texpage region counts as
        // "static", sweeping shared residual (battle leftovers, animation
        // frames) into the mask and falsely flagging the engine for not
        // reproducing it. Skip a scene whose captures don't diverge enough.
        let (mut nonzero, mut static_nonzero) = (0usize, 0usize);
        for y in TEXPAGE_Y_START..VRAM_HEIGHT {
            if NPC_CLUT_BAND_ROWS.contains(&y) {
                continue;
            }
            for x in 0..VRAM_WIDTH {
                let widx = y * VRAM_WIDTH + x;
                let off = widx * 2;
                if u16::from_le_bytes([refs[0][off], refs[0][off + 1]]) != 0 {
                    nonzero += 1;
                    if mask[widx] {
                        static_nonzero += 1;
                    }
                }
            }
        }
        let static_frac = static_nonzero as f64 / nonzero.max(1) as f64;
        if static_frac > MAX_STATIC_FRACTION {
            eprintln!(
                "[skip]  scene={scene_name:<10} captures too similar for a static mask \
                 ({:.1}% of the texpage band identical; need < {:.0}% - use captures \
                 from genuinely different states)",
                static_frac * 100.0,
                MAX_STATIC_FRACTION * 100.0
            );
            continue;
        }

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
