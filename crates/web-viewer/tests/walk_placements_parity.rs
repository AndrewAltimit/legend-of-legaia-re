//! Verify the world-overview viewer's walk-frame pack-mesh stamps
//! (`build_walk_placements`, built from raw PROT.DAT bytes) match the native
//! engine's authoritative `Scene::walk_object_placements` +
//! `Scene::walk_decoration_placements` + floor-height LUT for every world-map
//! kingdom. This is the parity guarantee that the static-site WebGL viewer
//! draws the slot-1 pack landmarks AND the decoration layer (crossed-quad
//! trees, mountain groups, props) at the same world coordinates (and Y
//! elevation) the engine resolves them to, on top of the continent
//! heightfield (see `walk_ground_parity.rs`).
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset, matching the rest of the
//! disc-dependent test suite. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_web_viewer::build_walk_placements;
use legaia_web_viewer::disc::{extract_cdname_txt, extract_prot_dat, parse_prot_toc};
use std::env;
use std::fs;

/// (PROT base, CDNAME scene) for the three world-map kingdoms.
const KINGDOMS: &[(u32, &str)] = &[(85, "map01"), (244, "map02"), (391, "map03")];

#[test]
fn walk_placements_match_engine_for_every_kingdom() {
    let Some(disc_path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping walk-placement parity test");
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
        let viewer = build_walk_placements(&prot, &entries, prot_base)
            .unwrap_or_else(|| panic!("{scene_name}: build_walk_placements returned None"));

        // Engine side: the authoritative resolver via CDNAME + ProtIndex. Drop
        // the protagonist / NPC placements (pack_index None) the viewer also
        // drops, then resolve world Y from the floor LUT exactly as the native
        // play-window render (`resolve_placement_draws`) does.
        let scene = Scene::load(&index, scene_name).expect("scene load");
        let lut = scene
            .field_floor_height_lut(&index)
            .expect("floor LUT")
            .unwrap_or_else(|| panic!("{scene_name}: engine floor LUT returned None"));
        let mut placements = scene
            .walk_object_placements(&index)
            .expect("walk_object_placements")
            .unwrap_or_else(|| panic!("{scene_name}: engine walk_object_placements returned None"));
        // The decoration layer, in the same order the viewer concatenates it.
        placements.extend(
            scene
                .walk_decoration_placements(&index)
                .expect("walk_decoration_placements")
                .unwrap_or_else(|| {
                    panic!("{scene_name}: engine walk_decoration_placements returned None")
                }),
        );
        let engine: Vec<(u32, i32, i32, i32, u16)> = placements
            .iter()
            .filter_map(|p| {
                let pack_index = p.pack_index?;
                let world_y = match p.floor_nibble {
                    Some(nib) => -(lut[(nib & 0x0F) as usize] as i32) + p.y_off as i32,
                    None => 0,
                };
                Some((pack_index as u32, p.world_x, world_y, p.world_z, p.rot_y))
            })
            .collect();

        assert_eq!(
            viewer.len(),
            engine.len(),
            "{scene_name}: placement count mismatch (viewer {} vs engine {})",
            viewer.len(),
            engine.len()
        );
        assert!(
            !viewer.is_empty(),
            "{scene_name}: no walk-frame placements resolved"
        );
        for (i, (v, e)) in viewer.iter().zip(engine.iter()).enumerate() {
            assert_eq!(
                (v.pack_index, v.world_x, v.world_y, v.world_z, v.rot_y),
                *e,
                "{scene_name}: placement {i} mismatch"
            );
        }

        // The authored-yaw field (object record +0x0A) is what orients the
        // Sebucus island bridges: the retail map02 walk `.MAP` places the
        // same bridge meshes at quarter-turn rotations (0x400 = 90 deg,
        // 0xC00 = 270 deg in the PSX 4096-per-rev space). Pin that the
        // viewer surfaces them, so a regression back to all-zero yaw (every
        // bridge rendered in the same orientation) fails loudly.
        if scene_name == "map02" {
            let quarter_turns = viewer
                .iter()
                .filter(|p| p.rot_y == 0x400 || p.rot_y == 0xC00)
                .count();
            assert!(
                quarter_turns >= 4,
                "map02: expected >= 4 quarter-turn bridge placements, got {quarter_turns}"
            );
        }

        eprintln!(
            "{scene_name}: {} walk-frame landmark + decoration placements (viewer == engine)",
            viewer.len()
        );
    }
}
