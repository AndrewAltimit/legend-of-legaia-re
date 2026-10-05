//! Disc-gated: the world-overview page runs each kingdom's **slot-5
//! CLUT-walk** shimmer (`set_scene_kingdom` installs it through the engine's
//! `ClutWalkAnim`, `kingdom_clut_tick` steps it), so every walker
//! destination cell in the kingdom VRAM holds one of its own source strips -
//! the palette retail's `FUN_8001ada4` case-0xB copy puts there.
//!
//! Without it the page's VRAM held only the slot-0 TIMs: the river /
//! shoreline cells beside the ocean head kept another TIM's palette, their
//! source strips (rows 498 / 502..505) were blank, and the zero CLUT entries
//! drew as transparent texels the ocean backdrop plane showed through -
//! water seeming to slide under the land as the camera turned.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::LegaiaViewer;

fn loaded() -> Option<LegaiaViewer> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut v = LegaiaViewer::new_headless();
    v.load_disc(bytes).ok()?;
    Some(v)
}

fn px(vram: &[u8], x: usize, y: usize) -> u16 {
    let o = (y * 1024 + x) * 2;
    u16::from_le_bytes([vram[o], vram[o + 1]])
}

fn cell(vram: &[u8], x: usize, y: usize) -> Vec<u16> {
    (0..16).map(|i| px(vram, x + i, y)).collect()
}

/// Does `want` appear as a 16-aligned 16-entry window anywhere on `row`?
fn row_has_strip(vram: &[u8], row: usize, want: &[u16]) -> bool {
    (0..1024 / 16).any(|k| cell(vram, k * 16, row) == want)
}

#[test]
fn kingdom_clut_walk_fills_every_walker_cell_from_its_strips() {
    let Some(mut viewer) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    for (prot_base, label) in [(85u32, "map01"), (244, "map02"), (391, "map03")] {
        viewer
            .set_scene_kingdom(prot_base)
            .unwrap_or_else(|e| panic!("{label}: set_scene_kingdom: {e:?}"));
        let entries = viewer.kingdom_clut_walker_entries();
        assert_eq!(entries, 8, "{label}: slot-5 walker entries");
        // The parked source rows are resident before the first step.
        let vram = viewer.pack_vram_bytes();
        for row in [498usize, 502, 503, 504, 505] {
            assert!(
                (0..64).any(|x| px(&vram, x, row) != 0),
                "{label}: source row {row} parked"
            );
        }
        // One overworld game tick (3 vsyncs): every accumulator starts at the
        // retail seed, so all eight walkers fire.
        assert!(viewer.kingdom_clut_tick(3), "{label}: first step writes");
        let vram = viewer.pack_vram_bytes();
        // Ocean head: a row-505 strip, entry 0 opaque black or clear per strip.
        let head = cell(&vram, 0, 506);
        assert!(
            row_has_strip(&vram, 505, &head),
            "{label}: (0,506) = a row-505 strip"
        );
        // The river cell beside it walks row 503.
        let river = cell(&vram, 16, 506);
        assert!(
            row_has_strip(&vram, 503, &river),
            "{label}: (16,506) = a row-503 strip ({river:04x?})"
        );
        for x in [0usize, 16] {
            let c = cell(&vram, x, 508);
            assert!(
                row_has_strip(&vram, 504, &c),
                "{label}: ({x},508) = a row-504 strip"
            );
        }
    }
}
