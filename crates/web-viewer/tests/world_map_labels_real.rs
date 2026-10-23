//! The world-overview page's top-down map labels each kingdom from the
//! location table trailing the kingdom MAN (`build_world_map_labels`). This
//! holds the two properties the page leans on, for all three kingdoms:
//!
//! - every label of a kingdom sits **on that kingdom's continent** - inside
//!   the XZ bounds of the walk heightfield the same page draws, which is what
//!   says the table's `map << 7` coordinates and the walk frame are one frame;
//! - the three kingdoms partition the table (no record dropped or doubled).
//!
//! It also pins the placed / decoration split the map draws as two dot
//! families. Skipped (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_iso::image::extract_prot_dat;
use legaia_prot::toc_index::parse_prot_toc;
use legaia_web_viewer::{build_walk_ground, build_walk_placements, build_world_map_labels};
use std::env;
use std::fs;

const KINGDOMS: &[(u32, &str)] = &[(85, "map01"), (244, "map02"), (391, "map03")];

#[test]
fn world_map_labels_sit_on_their_kingdom() {
    let Some(disc_path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset; skipping world-map label test");
        return;
    };
    let disc = fs::read(&disc_path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let entries = parse_prot_toc(&prot).expect("PROT TOC parse");

    let mut total = 0usize;
    let mut seen = std::collections::BTreeSet::new();
    for &(prot_base, scene) in KINGDOMS {
        let labels = build_world_map_labels(&prot, &entries, prot_base)
            .unwrap_or_else(|| panic!("{scene}: no world-map label table"));
        assert!(!labels.is_empty(), "{scene}: no labels for this region");

        let ground = build_walk_ground(&prot, &entries, prot_base)
            .unwrap_or_else(|| panic!("{scene}: no walk heightfield"));
        let (mut x0, mut x1, mut z0, mut z1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for p in &ground.positions {
            x0 = x0.min(p[0]);
            x1 = x1.max(p[0]);
            z0 = z0.min(p[2]);
            z1 = z1.max(p[2]);
        }
        for l in &labels {
            let (x, z) = (l.world_x as f32, l.world_z as f32);
            assert!(
                x >= x0 && x <= x1 && z >= z0 && z <= z1,
                "{scene}: label {} at ({x}, {z}) is off the continent \
                 x {x0}..{x1} z {z0}..{z1}",
                l.index
            );
            assert!(!l.name.is_empty(), "{scene}: label {} unnamed", l.index);
            assert!(seen.insert(l.index), "record {} in two kingdoms", l.index);
        }
        total += labels.len();

        let stamps = build_walk_placements(&prot, &entries, prot_base)
            .unwrap_or_else(|| panic!("{scene}: no walk placements"));
        let placed = stamps.iter().filter(|p| p.placed).count();
        assert!(placed > 0, "{scene}: no placed objects");
        assert!(placed < stamps.len(), "{scene}: no decoration stamps");
        eprintln!(
            "{scene}: {} labels, {placed} placed objects, {} decoration stamps",
            labels.len(),
            stamps.len() - placed
        );
    }
    assert_eq!(
        total, 29,
        "the three kingdoms partition the 29-record table"
    );
    assert!(build_world_map_labels(&prot, &entries, 86).is_none());
}
