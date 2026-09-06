//! Disc-gated regression tests over every kingdom bundle's slot 4 - the
//! world-map scene's actor animation bank (asset type `0x05`).
//!
//! `slot4_parses_for_every_kingdom` guards the container: all three kingdom
//! PROT entries (0086, 0245, 0392 - `kingdom_bundle::BUNDLE_ENTRIES`, NOT the
//! `0085` / `0244` / `0391` prescript entries the superseded over-reading
//! entry size started in) carry a valid 7-asset table, slot 4 LZS-decodes
//! cleanly at its declared size, and the payload parses with marker `0x080C`
//! and body sizes fitting `8 + part_count * frame_count * 8 + 8`. Its
//! `top_down_lines` / `wireframe_segments_3d` assertions are **byte-view**
//! guards - those helpers plot raw `i16` slices that straddle the entries'
//! packed nibble boundaries, and are not geometry.
//!
//! `slot4_entries_decode_as_rigid_transforms` guards the decoded model.
//!
//! Both skip silently when `LEGAIA_DISC_BIN` is unset or `extracted/PROT/`
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
        (kingdom_bundle::BUNDLE_ENTRIES[0], "Drake", 15, 2000),
        (kingdom_bundle::BUNDLE_ENTRIES[1], "Sebucus", 16, 1500),
        (kingdom_bundle::BUNDLE_ENTRIES[2], "Karisto", 16, 1500),
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

        // The 3D segment emitter uses the same row-major group-polyline
        // topology, so it yields the same segment count as the (X, Z)-
        // projected RowMajor path while keeping all three raw `i16` slices.
        // Both are byte-inspection plots, not geometry (the slices straddle
        // the entries' packed nibble boundaries); the equal-count + non-
        // degenerate checks below are a byte-view regression guard only.
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
            "{label}: every 3D segment's middle slice is 0 (byte view degenerate)"
        );

        // Sanity-bound the byte-view extent (again: field slices, not
        // coordinates - the wide spans come from adjacent packed fields
        // landing in one `i16`).
        let (xmin, zmin, xmax, zmax) =
            world_map_overlay::xz_bounds(&parsed).expect("xz_bounds present");
        assert!(xmin < xmax, "{label}: degenerate X bounds");
        assert!(zmin < zmax, "{label}: degenerate Z bounds");
        let dx = i32::from(xmax) - i32::from(xmin);
        let dz = i32::from(zmax) - i32::from(zmin);
        // Every kingdom's payload reaches these spans in the byte view; a
        // regression that shifted the record stride would collapse them.
        assert!(
            dx > 20_000 && dz > 50_000,
            "{label}: bounds too small ({xmin}..{xmax}, {zmin}..{zmax})"
        );
    }
}

/// Every kingdom's slot 4 satisfies the animation-clip model decoded from
/// `FUN_800204F8` / `FUN_8001B964` / `FUN_8001BE80`:
///
///  - the header's frame-count high byte is 0 and its `rate` is 1 / 2 / 4;
///  - `flags` carries no bit but bit 0, and setting it implies `rate >= 2`
///    (the divisor is only consulted on the interpolating path);
///  - the 8-byte trailer is present and the entry count is exactly
///    `part_count * frame_count`;
///  - byte 4's high nibble - the one field the runtime decoder never reads -
///    is zero in every entry;
///  - rotation angles are byte-scaled, so every one is a multiple of 16 and
///    lands in `0..=0xFF0`, and translations stay inside the 12-bit signed
///    range they are packed in.
///
/// Skips silently when `LEGAIA_DISC_BIN` is unset or `extracted/PROT/` is
/// missing.
#[test]
fn slot4_entries_decode_as_rigid_transforms() {
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let kingdoms: &[(u32, &str)] = &[
        (kingdom_bundle::BUNDLE_ENTRIES[0], "Drake"),
        (kingdom_bundle::BUNDLE_ENTRIES[1], "Sebucus"),
        (kingdom_bundle::BUNDLE_ENTRIES[2], "Karisto"),
    ];

    let mut checked_entries = 0usize;
    let mut moving_paths = 0usize;
    for &(base, label) in kingdoms {
        let path = find_kingdom(&prot, base)
            .unwrap_or_else(|| panic!("{label}: no PROT entry {base:04} in {prot:?}"));
        let buf = std::fs::read(&path).unwrap();
        let bundle = kingdom_bundle::parse(&buf)
            .unwrap_or_else(|| panic!("{label}: kingdom_bundle::parse failed for {path:?}"));
        let decoded = bundle.slots[4]
            .decoded
            .as_ref()
            .unwrap_or_else(|e| panic!("{label}: slot 4 LZS decode failed: {e}"));
        let parsed = world_map_overlay::parse(decoded)
            .unwrap_or_else(|e| panic!("{label}: slot 4 parse failed: {e}"));

        for b in &parsed.bodies {
            let idx = b.index;
            assert_eq!(b.flag_b, 0, "{label}: body {idx} frame-count high byte set");
            assert_eq!(
                b.frame_count(),
                b.count_b as usize,
                "{label}: body {idx} frame count disagrees with the low byte"
            );
            assert!(
                matches!(b.subframe_divisor(), 1 | 2 | 4),
                "{label}: body {idx} rate {}",
                b.subframe_divisor()
            );
            assert_eq!(b.flag_a & !1, 0, "{label}: body {idx} unknown flag bits");
            if b.interpolates() {
                assert!(
                    b.subframe_divisor() >= 2,
                    "{label}: body {idx} interpolates with divisor 1"
                );
            }
            assert_eq!(
                b.records.len(),
                b.part_count() * b.frame_count(),
                "{label}: body {idx} entry count mismatch"
            );

            for f in 0..b.frame_count() {
                for p in 0..b.part_count() {
                    let e = b
                        .entry(f, p)
                        .unwrap_or_else(|| panic!("{label}: body {idx} missing entry ({f}, {p})"));
                    let t = e.transform();
                    checked_entries += 1;
                    assert_eq!(
                        t.reserved, 0,
                        "{label}: body {idx} entry ({f}, {p}) reserved nibble {}",
                        t.reserved
                    );
                    for (name, r) in [("rx", t.rx), ("ry", t.ry), ("rz", t.rz)] {
                        assert_eq!(r & 0xF, 0, "{label}: body {idx} {name} {r} not byte-scaled");
                        assert!(
                            (0..=0xFF0).contains(&r),
                            "{label}: body {idx} {name} {r} out of byte-scaled range"
                        );
                    }
                    for (name, v) in [("tx", t.tx), ("ty", t.ty), ("tz", t.tz)] {
                        assert!(
                            (-2048..=2047).contains(&v),
                            "{label}: body {idx} {name} {v} outside the 12-bit signed range"
                        );
                    }
                }
            }
        }

        // The decoded translation paths are the geometric reading that is
        // actually in the bytes: at least some object must move.
        let paths = world_map_overlay::translation_path_segments(&parsed);
        assert!(
            !paths.is_empty(),
            "{label}: no object translates over any clip"
        );
        moving_paths += paths.len();
    }

    assert!(
        checked_entries > 8_000,
        "only {checked_entries} entries seen"
    );
    assert!(moving_paths > 100, "only {moving_paths} path segments");
    eprintln!("[ok] {checked_entries} entries, {moving_paths} translation segments");
}
