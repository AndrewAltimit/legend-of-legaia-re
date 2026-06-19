//! Disc-gated regression test: every kingdom bundle's slot 4 parses as a
//! world-map slot-4 container parse.
//!
//! Asserts the invariants we rely on for the world-overview page
//! visualization:
//!  - All three kingdom PROT entries (0085, 0244, 0391) carry a valid
//!    7-asset table.
//!  - Slot 4 LZS-decodes cleanly with declared size.
//!  - The decoded payload parses with `world_map_overlay::parse` (count
//!    in expected range, marker 0x080C across every body, body sizes
//!    fit `8 + count_a * count_b * 8 + 8`).
//!  - `top_down_lines` produces a non-empty line list for each kingdom.
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset or `extracted/PROT/`
//! is missing.

use legaia_asset::kingdom_bundle;
use legaia_asset::world_map_overlay;
use std::path::PathBuf;

fn extracted_prot() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ];
    candidates.into_iter().find(|p| p.is_dir())
}

fn find_kingdom(prot: &PathBuf, prot_base: u32) -> Option<PathBuf> {
    let prefix = format!("{prot_base:04}_");
    std::fs::read_dir(prot)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.starts_with(&prefix))
        })
}

#[test]
fn slot4_parses_for_every_kingdom() {
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    // (prot_base, label, min expected bodies, min expected line segments)
    let kingdoms: &[(u32, &str, usize, usize)] = &[
        (85, "Drake", 15, 2000),
        (244, "Sebucus", 16, 1500),
        (391, "Karisto", 16, 1500),
    ];

    for &(base, label, min_bodies, min_lines) in kingdoms {
        let path = find_kingdom(&prot, base)
            .unwrap_or_else(|| panic!("{label}: no PROT entry {base:04} in {prot:?}"));
        let buf = std::fs::read(&path).unwrap();
        let bundle = kingdom_bundle::parse(&buf)
            .unwrap_or_else(|| panic!("{label}: kingdom_bundle::parse failed for {path:?}"));
        assert_eq!(bundle.slots.len(), 7, "{label}: expected 7-slot bundle");

        let slot4 = &bundle.slots[4];
        assert_eq!(
            slot4.type_byte, 0x05,
            "{label}: slot 4 type byte should be 0x05 (MOVE)"
        );
        let decoded = slot4
            .decoded
            .as_ref()
            .unwrap_or_else(|e| panic!("{label}: slot 4 LZS decode failed: {e}"));

        let parsed = world_map_overlay::parse(decoded)
            .unwrap_or_else(|e| panic!("{label}: slot 4 parse failed: {e}"));

        assert!(
            parsed.bodies.len() >= min_bodies,
            "{label}: got {} bodies, expected >= {min_bodies}",
            parsed.bodies.len()
        );
        for b in &parsed.bodies {
            assert_eq!(
                b.marker, 0x080C,
                "{label}: body {} bad marker 0x{:04X}",
                b.index, b.marker
            );
            assert_eq!(
                b.records.len(),
                b.count_a as usize * b.count_b as usize,
                "{label}: body {} record count mismatch",
                b.index
            );
            // Observed kind values: 1, 2, 4 across all three kingdoms.
            // Anything else is a parse / detector regression.
            assert!(
                matches!(b.kind, 1 | 2 | 4),
                "{label}: body {} unexpected kind {}",
                b.index,
                b.kind
            );
        }

        let opts = world_map_overlay::WireframeOptions::default();
        let lines = world_map_overlay::top_down_lines(&parsed, &opts);
        assert!(
            lines.len() >= min_lines,
            "{label}: only {} wireframe lines, expected >= {min_lines}",
            lines.len()
        );

        // The 3D segment emitter (the live-engine inspection overlay's
        // geometry source) uses the same row-major group-polyline topology,
        // so it yields the same segment count as the (X, Z)-projected
        // RowMajor path - but preserves the full 3D coordinate, so at least
        // one segment must carry a non-zero Y endpoint (the bodies are
        // object-local 3D meshes, not flat top-down contours).
        let segs = world_map_overlay::wireframe_segments_3d(&parsed, &opts);
        assert_eq!(
            segs.len(),
            lines.len(),
            "{label}: 3D segment count {} != top-down line count {}",
            segs.len(),
            lines.len()
        );
        assert!(
            segs.iter().any(|s| s.a[1] != 0 || s.b[1] != 0),
            "{label}: every 3D segment is Y=0 (expected real elevation)"
        );

        // Sanity-bound the X-Z extent. Drake reaches ±32K via its
        // full-extent kind-4 body; Sebucus / Karisto have similar extents
        // (every kingdom carries at least one such full-span body).
        let (xmin, zmin, xmax, zmax) =
            world_map_overlay::xz_bounds(&parsed).expect("xz_bounds present");
        assert!(xmin < xmax, "{label}: degenerate X bounds");
        assert!(zmin < zmax, "{label}: degenerate Z bounds");
        let dx = i32::from(xmax) - i32::from(xmin);
        let dz = i32::from(zmax) - i32::from(zmin);
        // Each kingdom's slot 4 always includes at least one full-extent
        // kind-4 body which spans the full 16K x-extent; the smallest
        // observed kingdom (Sebucus) covers the half-world with
        // -28160..-3046 X (dx >= 25000) and the full -32K..+32K Z.
        assert!(
            dx > 20_000 && dz > 50_000,
            "{label}: bounds too small ({xmin}..{xmax}, {zmin}..{zmax})"
        );
    }
}
