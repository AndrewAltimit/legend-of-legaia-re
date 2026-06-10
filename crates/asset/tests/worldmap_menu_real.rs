//! Decode the real world-map menu placement + landmark-name tables out of
//! `extracted/SCUS_942.54` and check their invariants. Skips and passes when
//! the executable isn't on disk (the disc-gated skip pattern).
//!
//! The module's own unit tests exercise a synthetic PS-X EXE fixture; this
//! oracle pins the parse against the retail executable. The decisive
//! cross-table tie is that each placement's `scene_id` is the destination
//! scene's PROT bundle index: the three kingdom bundles (Drake 85, Sebucus
//! 244, Karisto 391 — `docs/formats/world-map-overlay.md`) all appear as
//! destinations, so a drift in either the placement table base or the kingdom
//! bundle indexing would break this test.

use legaia_asset::worldmap_menu::{self, NAME_COUNT, PLACEMENT_MAX};
use std::path::PathBuf;

fn scus_bytes() -> Option<Vec<u8>> {
    let ws = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()
        .map(PathBuf::from)?;
    let path = ws.join("extracted").join("SCUS_942.54");
    if !path.is_file() {
        eprintln!("extracted/SCUS_942.54 missing - skipping");
        return None;
    }
    Some(std::fs::read(&path).expect("read SCUS_942.54"))
}

#[test]
fn decodes_landmark_names_or_skips() {
    let Some(scus) = scus_bytes() else { return };
    let menu = worldmap_menu::parse_scus(&scus).expect("parse worldmap menu");

    // The name table is a fixed-size block (16 landmarks).
    assert_eq!(menu.names.len(), NAME_COUNT);

    // Ground-truth endpoints: the opening town and the late-game capital that
    // has no on-map placement record (it is only reached in cutscenes).
    assert_eq!(menu.names.first().map(String::as_str), Some("Rim Elm"));
    assert_eq!(menu.names.get(0x0E).map(String::as_str), Some("Conkram"));

    // Every name is non-empty printable ASCII (no truncation / garbage).
    for (i, name) in menu.names.iter().enumerate() {
        assert!(!name.is_empty(), "name {i} is empty");
        assert!(
            name.bytes().all(|b| b.is_ascii_graphic() || b == b' '),
            "name {i} = {name:?} has non-printable bytes"
        );
    }
}

#[test]
fn placement_table_walks_and_resolves_or_skips() {
    let Some(scus) = scus_bytes() else { return };
    let menu = worldmap_menu::parse_scus(&scus).expect("parse worldmap menu");

    // The walk terminated at the `0xFF` sentinel well before the clamp.
    assert!(!menu.placements.is_empty(), "no placement records walked");
    assert!(
        menu.placements.len() < PLACEMENT_MAX,
        "placement walk hit the {PLACEMENT_MAX} clamp - terminator not found"
    );

    // Indices are dense from zero (the walker emits its own running index).
    for (i, rec) in menu.placements.iter().enumerate() {
        assert_eq!(rec.index as usize, i, "placement index gap at {i}");
        // Every placement names a valid landmark.
        assert!(
            (rec.name_idx as usize) < menu.names.len(),
            "placement {i} name_idx {} out of range",
            rec.name_idx
        );
        // A placement always loads a real destination scene.
        assert_ne!(rec.scene_id, 0, "placement {i} has null scene_id");
    }

    // Conkram (name index 0x0E) is the one landmark with no placement record.
    assert!(
        menu.placements.iter().all(|p| p.name_idx != 0x0E),
        "Conkram should have no placement record"
    );
}

#[test]
fn destinations_tie_to_kingdom_bundles_or_skips() {
    let Some(scus) = scus_bytes() else { return };
    let menu = worldmap_menu::parse_scus(&scus).expect("parse worldmap menu");

    // Each `scene_id` is the destination's PROT bundle index. The three
    // kingdom bundles named in docs/formats/world-map-overlay.md must all be
    // reachable from the world-map menu.
    let dests: std::collections::BTreeSet<u16> =
        menu.placements.iter().map(|p| p.scene_id).collect();
    for kingdom in [85u16, 244, 391] {
        assert!(
            dests.contains(&kingdom),
            "kingdom bundle PROT {kingdom} not among world-map destinations {dests:?}"
        );
    }
}
