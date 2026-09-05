//! Disc-gated: the world-overview page's `pack_mesh_*` accessors hand it the
//! **hybrid** landmark mesh - the textured prims followed by the untextured
//! `F*`/`G*` vertex-colour prims, with the `[r, g, b, flag]` stream saying
//! which is which - so the page draws whole landmarks. Rim Elm (Drake slot
//! 29) is the worked case: its hut walls are textured quads and its four
//! roofs are 24 gouraud triangles that a textured-only build dropped,
//! leaving open rings on the overworld.
//!
//! The counts are disc invariants of the kingdom slot-1 TMD packs (one
//! emitted corner per prim vertex: 3 per triangle, 4 per quad). Skipped
//! (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::LegaiaViewer;

fn loaded() -> Option<LegaiaViewer> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = match std::fs::read(&disc) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[skip] {disc}: {e}");
            return None;
        }
    };
    let mut v = LegaiaViewer::new_headless();
    v.load_disc(bytes).ok()?;
    Some(v)
}

/// `(kingdom PROT base, label, pack slot, textured corners, untextured corners)`.
///
/// - Drake 29 (Rim Elm): 6 FT3 + 35 FT4 + 17 GT4 textured; 24 G3 roofs.
/// - Drake 32 (the multi-roof building cluster): 11 GT3 + 48 GT4; 24 G3.
/// - Sebucus 6: 4 FT3 + 111 FT4 + 23 GT4; 16 F4 + 4 G4.
/// - Karisto 8 (the Uru Mais temple, placed at cell `(36, 75)`): no textured
///   prim at all - 50 G3 + 30 G4. A textured-only build had nothing to upload,
///   so the overview drew no landmark there.
const CASES: &[(u32, &str, u32, usize, usize)] = &[
    (85, "map01", 29, 226, 72),
    (85, "map01", 32, 225, 72),
    (244, "map02", 6, 548, 80),
    (391, "map03", 8, 0, 270),
];

#[test]
fn pack_accessors_carry_the_untextured_half_flagged() {
    let Some(mut viewer) = loaded() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping world-map pack hybrid test");
        return;
    };
    for &(prot_base, label, slot, textured, untextured) in CASES {
        viewer
            .set_scene_kingdom(prot_base)
            .unwrap_or_else(|e| panic!("{label}: set_scene_kingdom: {e:?}"));
        viewer
            .pack_mesh(slot)
            .unwrap_or_else(|e| panic!("{label} slot {slot}: pack_mesh: {e:?}"));
        let positions = viewer.pack_mesh_positions();
        let flat = viewer.pack_mesh_flat_rgba();
        let indices = viewer.pack_mesh_indices();
        let total = textured + untextured;
        assert_eq!(
            positions.len(),
            total * 3,
            "{label} slot {slot}: corner count"
        );
        assert_eq!(
            flat.len(),
            total * 4,
            "{label} slot {slot}: flat stream length"
        );
        assert_eq!(
            viewer.pack_mesh_uvs().len(),
            total * 2,
            "{label} slot {slot}: uv stream length"
        );
        assert_eq!(
            viewer.pack_mesh_cba_tsb().len(),
            total * 2,
            "{label} slot {slot}: cba/tsb stream length"
        );
        // Textured prefix flagged 255, untextured tail flagged 0 - the
        // shader's `a_flat_rgba.a < 0.5` fill branch keys off exactly this.
        let flags: Vec<u8> = flat.chunks_exact(4).map(|c| c[3]).collect();
        assert!(
            flags[..textured].iter().all(|&f| f == 255),
            "{label} slot {slot}: textured prefix must be flagged 255"
        );
        assert!(
            flags[textured..].iter().all(|&f| f == 0),
            "{label} slot {slot}: untextured tail must be flagged 0"
        );
        assert_eq!(
            flags.iter().filter(|&&f| f == 0).count(),
            untextured,
            "{label} slot {slot}: untextured corner count"
        );
        // Every corner is indexed (nothing uploaded that never draws).
        let max_index = indices.iter().copied().max().unwrap_or(0) as usize;
        assert_eq!(max_index + 1, total, "{label} slot {slot}: index range");
    }
}
