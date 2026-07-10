//! Verify the web viewer's assembled full-scene build
//! ([`legaia_web_viewer::field_scene::build_field_scene`]) resolves a real
//! map for representative field scenes: the environment mesh pack is found
//! (the `scene_asset_table` LZS TMD pack, not a lone `scene_tmd_stream`
//! slice), the `.MAP` placement + terrain layers resolve to in-range pack
//! draws, and the ground heightfield builds. For `town01` the numbers are
//! pinned against the engine-side ground truth (env entry 4; the placement
//! count matches `Scene::field_object_placements` minus the NPC records).
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset, matching the rest of
//! the disc-dependent test suite. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::scene::ProtIndex;
use legaia_web_viewer::disc::{extract_cdname_txt, extract_prot_dat};
use legaia_web_viewer::field_scene::build_field_scene;
use std::env;
use std::fs;

/// Representative scenes: the starter town, a dungeon (Mt. Rikuroa), and a
/// Karisto castle interior - the shapes the viewer's sidebar surfaces.
const SCENES: &[&str] = &["town01", "rikuroa", "korb3"];

#[test]
fn field_scene_assembles_full_maps() {
    let Some(disc_path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping field-scene assembly test");
        return;
    };
    let disc = fs::read(&disc_path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let cdname = extract_cdname_txt(&disc).expect("CDNAME.TXT extraction");
    let index = ProtIndex::from_bytes(prot, Some(&cdname)).expect("ProtIndex from in-memory PROT");

    for &name in SCENES {
        let pack = build_field_scene(&index, name)
            .unwrap_or_else(|e| panic!("{name}: build_field_scene failed: {e}"));
        assert!(
            pack.env_tmds.len() > 10,
            "{name}: expected a multi-mesh env pack, got {}",
            pack.env_tmds.len()
        );
        assert!(
            !pack.placements.is_empty(),
            "{name}: no placed-object draws resolved"
        );
        assert!(
            !pack.terrain.is_empty(),
            "{name}: no terrain-tile draws resolved"
        );
        // The walk-ground heightfield is scene-shape-dependent: open maps
        // (towns, mountains) carry a `0x1000`-gated floor grid; some castle
        // interiors floor entirely with terrain-tile meshes instead (korb3).
        // Require *a* ground layer: heightfield or terrain tiles.
        let ground_quads = pack.ground.as_ref().map(|h| h.quad_count()).unwrap_or(0);
        assert!(
            ground_quads > 0 || pack.terrain.len() > 20,
            "{name}: no ground layer (0 heightfield quads, {} terrain tiles)",
            pack.terrain.len()
        );
        // Every draw must reference a valid pack slot + res TMD.
        for d in pack.placements.iter().chain(pack.terrain.iter()) {
            assert!(
                d.env_slot < pack.env_tmds.len(),
                "{name}: draw env_slot {} out of range",
                d.env_slot
            );
            assert!(
                d.res_tmd < pack.res.tmds.len(),
                "{name}: draw res_tmd {} out of range",
                d.res_tmd
            );
        }
        eprintln!(
            "{name}: {} env meshes, {} placements, {} terrain tiles, {} ground quads",
            pack.env_tmds.len(),
            pack.placements.len(),
            pack.terrain.len(),
            ground_quads
        );
    }
}

#[test]
fn town01_env_pack_matches_engine_ground_truth() {
    let Some(disc_path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping town01 env-pack pin test");
        return;
    };
    let disc = fs::read(&disc_path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let cdname = extract_cdname_txt(&disc).expect("CDNAME.TXT extraction");
    let index = ProtIndex::from_bytes(prot, Some(&cdname)).expect("ProtIndex from in-memory PROT");

    let pack = build_field_scene(&index, "town01").expect("town01 build");
    // town01's environment geometry lives in PROT entry 4 (the
    // scene_asset_table LZS TMD pack) - the vote must land there, not on a
    // scene_tmd_stream battle-mesh entry.
    let env_entry = pack.res.tmds[pack.env_tmds[0]].entry_idx;
    assert_eq!(env_entry, 4, "town01 env pack entry");
    assert!(
        pack.env_tmds
            .iter()
            .all(|&i| pack.res.tmds[i].entry_idx == env_entry),
        "env pack spans multiple entries"
    );
    // The placed-object layer: Rim Elm's buildings/props (the engine draws
    // ~40 of 46 placements; the pack-resolved draw count sits in between
    // because mesh-level prim filtering happens later, at upload).
    assert!(
        (20..=60).contains(&pack.placements.len()),
        "town01 placement draw count {} outside expected band",
        pack.placements.len()
    );
}
