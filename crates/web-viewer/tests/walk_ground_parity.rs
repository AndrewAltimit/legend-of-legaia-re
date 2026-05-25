//! Verify the world-overview viewer's walk-view continent ground heightfield
//! (`build_walk_ground`, built from raw PROT.DAT bytes) is byte-identical to the
//! native engine's authoritative `Scene::walk_heightfield` for every world-map
//! kingdom. This is the parity guarantee that the static-site WebGL viewer
//! renders the same terrain the engine does.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset, matching the rest of the
//! disc-dependent test suite. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_web_viewer::build_walk_ground;
use legaia_web_viewer::disc::{extract_cdname_txt, extract_prot_dat, parse_prot_toc};
use std::env;
use std::fs;

/// (PROT base, CDNAME scene) for the three world-map kingdoms.
const KINGDOMS: &[(u32, &str)] = &[(85, "map01"), (244, "map02"), (391, "map03")];

#[test]
fn walk_ground_matches_engine_heightfield_for_every_kingdom() {
    let Some(disc_path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping walk-ground parity test");
        return;
    };
    let disc = fs::read(&disc_path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let cdname = extract_cdname_txt(&disc).expect("CDNAME.TXT extraction");
    let entries = parse_prot_toc(&prot).expect("PROT TOC parse");

    let index =
        ProtIndex::from_bytes(prot.clone(), Some(&cdname)).expect("ProtIndex from in-memory PROT");

    for &(prot_base, scene_name) in KINGDOMS {
        // Viewer side: build straight from raw PROT bytes (the WASM path).
        let viewer = build_walk_ground(&prot, &entries, prot_base)
            .unwrap_or_else(|| panic!("{scene_name}: viewer build_walk_ground returned None"));

        // Engine side: the authoritative resolver via CDNAME + ProtIndex.
        let scene = Scene::load(&index, scene_name).expect("scene load");
        let engine = scene
            .walk_heightfield(&index)
            .expect("walk_heightfield")
            .unwrap_or_else(|| panic!("{scene_name}: engine walk_heightfield returned None"));

        assert_eq!(
            viewer.quad_count(),
            engine.quad_count(),
            "{scene_name}: quad count mismatch"
        );
        assert!(
            viewer.quad_count() > 1000,
            "{scene_name}: implausibly small heightfield ({} quads)",
            viewer.quad_count()
        );
        assert_eq!(
            viewer.positions, engine.positions,
            "{scene_name}: positions mismatch"
        );
        assert_eq!(viewer.uvs, engine.uvs, "{scene_name}: per-cell UV mismatch");
        assert_eq!(
            viewer.cba_tsb, engine.cba_tsb,
            "{scene_name}: per-cell [clut, tpage] mismatch"
        );
        assert_eq!(
            viewer.indices, engine.indices,
            "{scene_name}: index buffer mismatch"
        );

        // The terrain must be genuinely multi-page (grass + at least one other
        // page) for the viewer to be drawing real terrain types, not a flood.
        let mut pages: Vec<u16> = viewer.cba_tsb.iter().map(|ct| ct[1]).collect();
        pages.sort_unstable();
        pages.dedup();
        assert!(
            pages.len() >= 3,
            "{scene_name}: expected >=3 terrain pages, got {pages:?}"
        );

        eprintln!(
            "{scene_name}: {} quads, {} terrain pages {:?} (viewer == engine)",
            viewer.quad_count(),
            pages.len(),
            pages,
        );
    }
}
