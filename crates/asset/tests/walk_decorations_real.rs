//! Disc-gated: the kingdom walk `.MAP` decoration layer is gated on the draw
//! bit (`0x2000`), not the walkable-ground bit (`0x1000`), so the big
//! enterable mountains - whose cells carry a mesh and no ground - are part
//! of it, while the riverbank/system family (record 408, walk bit only) and
//! the placed interactive set stay out.
//!
//! Reads the extracted walk `.MAP`s (the first entry of each kingdom block:
//! extraction `0083` / `0242` / `0389`). Skips and passes when `extracted/`
//! is absent (the workspace disc-gated convention).

use legaia_asset::field_objects::{self, FLAG_PLACED, parse_placements, parse_walk_decorations};
use std::path::PathBuf;

/// The extended on-disc footprint of every field `.MAP` (records + floor grid
/// + object-index grid).
const WALK_FIELD_MAP_LEN: usize = 0x12000;

fn workspace() -> Option<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()
        .map(PathBuf::from)
}

/// The extracted `.BIN` for an extraction index, found by index prefix (the
/// label after the index is a CDNAME name, not part of the coordinate).
fn prot_bin(prot: &std::path::Path, entry: u32) -> Option<PathBuf> {
    let prefix = format!("{entry:04}_");
    std::fs::read_dir(prot).ok()?.flatten().find_map(|e| {
        let p = e.path();
        let name = p.file_name()?.to_str()?;
        (name.starts_with(&prefix) && name.ends_with(".BIN")).then_some(p)
    })
}

/// A big-landmark record the layer must carry: `(record, pack mesh, col, row)`.
type Landmark = (u16, u16, u8, u8);

/// `(walk .MAP extraction index, kingdom, big-landmark records, decoration
/// count)`.
///
/// The counts are disc invariants: every `0x2000` cell whose record is not
/// placed (Drake 304 - 6, Sebucus 275 - 33, Karisto 243 - 25).
const KINGDOMS: &[(u32, &str, &[Landmark], usize)] = &[
    (83, "map01", &[(412, 23, 39, 80), (473, 5, 38, 114)], 298),
    (242, "map02", &[(410, 15, 50, 79), (425, 1, 61, 18)], 242),
    (
        389,
        "map03",
        &[
            (444, 11, 16, 84),
            (445, 12, 93, 70),
            (447, 24, 93, 71),
            (462, 14, 49, 90),
            (463, 36, 98, 57),
            (473, 18, 66, 119),
        ],
        218,
    ),
];

#[test]
fn walk_decorations_carry_the_big_landmarks_and_not_the_riverbanks() {
    let Some(ws) = workspace() else {
        eprintln!("workspace root not found; skipping");
        return;
    };
    let prot = ws.join("extracted").join("PROT");
    if !prot.is_dir() {
        eprintln!("extracted/PROT absent; skipping walk-decoration disc test");
        return;
    }
    for &(entry, kingdom, landmarks, expected_count) in KINGDOMS {
        let Some(path) = prot_bin(&prot, entry) else {
            eprintln!("{kingdom}: extraction entry {entry:04} missing; skipping");
            continue;
        };
        let map = std::fs::read(&path).expect("read walk .MAP");
        assert_eq!(
            map.len(),
            WALK_FIELD_MAP_LEN,
            "{kingdom}: {} is not a 0x12000 walk .MAP",
            path.display()
        );
        let deco = parse_walk_decorations(&map);
        assert_eq!(deco.len(), expected_count, "{kingdom}: decoration count");
        // Disjoint from the placed set, and every stamp is a real pack slot.
        assert!(
            deco.iter().all(|p| p.flags & FLAG_PLACED == 0),
            "{kingdom}: placed leak"
        );
        assert!(
            deco.iter().all(|p| p.pack_index.is_some()),
            "{kingdom}: None mesh"
        );
        let placed = parse_placements(&map);
        for p in &placed {
            assert!(
                !deco.iter().any(|d| d.obj_idx == p.obj_idx),
                "{kingdom}: placed record {} also stamped as a decoration",
                p.obj_idx
            );
        }
        // The riverbank/system family never enters: record 408's cells carry
        // the walk bit only, on every kingdom map.
        assert!(
            !deco.iter().any(|p| p.obj_idx == 408),
            "{kingdom}: record 408 (riverbank) leaked into the decoration layer"
        );
        // Every decoration cell carries the corner block the Y kernel needs.
        assert!(deco.iter().all(|p| p.floor_corner_nibbles.is_some()));
        for &(rec, mesh, col, row) in landmarks {
            let hit = deco
                .iter()
                .find(|p| p.obj_idx == rec && p.col == col && p.row == row)
                .unwrap_or_else(|| {
                    panic!("{kingdom}: big landmark record {rec} at ({col}, {row}) missing")
                });
            assert_eq!(hit.pack_index, Some(mesh), "{kingdom}: record {rec} mesh");
            // The cell carries the draw bit and no walkable ground - the
            // signature that separates these from the tree/prop cells.
            let cell_off =
                field_objects::OBJECT_GRID_OFFSET + (row as usize * 128 + col as usize) * 2;
            let cell = u16::from_le_bytes([map[cell_off], map[cell_off + 1]]);
            assert_ne!(cell & field_objects::CELL_VISIBLE, 0);
            assert_eq!(
                cell & field_objects::CELL_WALK_VISIBLE,
                0,
                "{kingdom}: record {rec}'s cell is walkable ground - not the mountain signature"
            );
        }
        eprintln!(
            "{kingdom}: {} decorations incl. {} big landmarks",
            deco.len(),
            landmarks.len()
        );
    }
}
