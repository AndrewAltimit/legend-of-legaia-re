//! Disc-gated: the kingdom-bundle slot-5 CLUT-walk animation table decodes
//! identically from all three world-map kingdom bundles (PROT 0085 / 0244 /
//! 0391) and carries the pinned entry set - the ocean-head walk plus the
//! seven shoreline/terrain shimmer cells.
//!
//! Also pins the slot-0 park-strip complement: the walker's source strips
//! ship as raw CLUT-block records `park_strips` locates. Drake (map01)
//! carries the full six-row set (498/499/502..505); Sebucus / Karisto ship
//! only rows {501, 503, 505} and inherit the kingdom-invariant rest as VRAM
//! residue from the Drake upload (live-capture verified byte-exact), so
//! every walk source row must be covered by the scene's own strips, its
//! own slot-0 TIM CLUT rows, or the Drake strip set.
//!
//! Skips and passes when `LEGAIA_DISC_BIN` / the extracted kingdom bundles
//! are absent (the workspace disc-gated convention).

use legaia_asset::clut_walk::{self, ClutWalkTable};
use legaia_asset::kingdom_bundle;
use std::path::PathBuf;

fn workspace() -> Option<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()
        .map(PathBuf::from)
}

/// Expected entry shape: `(dest_x, dest_y, holds, src_y, src_xs)`.
/// A single-element `holds` slice means "every frame holds this long".
type Expected = (u16, u16, &'static [u8], u16, &'static [u16]);

const EXPECTED: [Expected; 8] = [
    // Ocean head: 18 steps over the row-505 strip, with the 128/144
    // ping-pong x3 mid-cycle.
    (
        0,
        506,
        &[8],
        505,
        &[
            0, 16, 32, 48, 64, 80, 96, 112, 128, 144, 128, 144, 128, 144, 160, 176, 192, 208,
        ],
    ),
    (0, 508, &[6], 504, &[0, 16, 32, 48]),
    (16, 508, &[8], 504, &[80, 96, 112, 128]),
    // 48-vsync first hold, then 12 per step.
    (
        16,
        506,
        &[48, 12, 12, 12, 12, 12, 12],
        503,
        &[0, 16, 32, 48, 64, 80, 96],
    ),
    (32, 506, &[10], 502, &[0, 16, 32, 48, 64, 80, 96]),
    (32, 509, &[20], 501, &[0, 16, 32, 16]),
    // The script-faded row-498 park cells double as walk sources.
    (32, 508, &[6], 498, &[0, 16, 32, 48]),
    (48, 500, &[6], 498, &[160, 176, 192, 208]),
];

fn assert_table(table: &ClutWalkTable, label: &str) {
    assert_eq!(table.entries.len(), 8, "{label}: entry count");
    for (k, (dest_x, dest_y, holds, src_y, src_xs)) in EXPECTED.iter().enumerate() {
        let e = &table.entries[k];
        assert_eq!(e.kind, 1, "{label}: entry {k} kind");
        assert_eq!(
            (e.dest_x, e.dest_y),
            (*dest_x, *dest_y),
            "{label}: entry {k} dest"
        );
        assert_eq!(
            e.frames.len(),
            src_xs.len(),
            "{label}: entry {k} frame count"
        );
        for (f, frame) in e.frames.iter().enumerate() {
            let hold = if holds.len() == 1 { holds[0] } else { holds[f] };
            assert_eq!(frame.hold_vsyncs, hold, "{label}: entry {k} frame {f} hold");
            assert_eq!(frame.src_x, src_xs[f], "{label}: entry {k} frame {f} src_x");
            assert_eq!(frame.src_y, *src_y, "{label}: entry {k} frame {f} src_y");
        }
    }
    // The on-disc cumulative_size field is a running total of entry bytes
    // (8-byte header + 8 bytes per frame).
    let mut cum = 0u16;
    for (k, e) in table.entries.iter().enumerate() {
        cum += 8 + 8 * e.frames.len() as u16;
        assert_eq!(e.cumulative_size, cum, "entry {k} cumulative_size");
    }
}

#[test]
fn clut_walk_table_identical_across_kingdoms_with_pinned_entries_or_skip() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(ws) = workspace() else { return };
    let kingdoms = ["0085_map01.BIN", "0244_map02.BIN", "0391_map03.BIN"];
    let prot = ws.join("extracted").join("PROT");
    if !kingdoms.iter().all(|k| prot.join(k).is_file()) {
        eprintln!("[skip] extracted kingdom bundles not present");
        return;
    }

    let mut shared: Option<Vec<u8>> = None;
    let mut drake_strip_rows: Vec<u16> = Vec::new();
    for k in kingdoms {
        let buf = std::fs::read(prot.join(k)).expect("read kingdom bundle");
        let decoded =
            kingdom_bundle::decode_slot(&buf, clut_walk::KINGDOM_SLOT).expect("decode slot 5");
        assert_eq!(decoded.len(), 0x204, "{k}: slot-5 decoded size");
        match &shared {
            None => shared = Some(decoded.clone()),
            Some(a) => assert_eq!(
                &decoded, a,
                "{k}: slot-5 table differs - it should be the shared global table"
            ),
        }
        let table = clut_walk::parse(&decoded).expect("parse CLUT-walk table");
        assert_table(&table, k);

        // The destination-cell fold matches the VRAM parity oracle's
        // world-map exclusion set (engine-shell
        // `vram_oracle::WORLD_MAP_CLUT_CYCLE_CELLS`): rows 506/508 cols
        // 0..48, row 509 cols 32..48, row 500 cols 48..64.
        let mut cells = table.dest_cells();
        cells.sort_by_key(|(y, r)| (*y, r.start));
        assert_eq!(
            cells,
            vec![
                (500, 48..64),
                (506, 0..16),
                (506, 16..32),
                (506, 32..48),
                (508, 0..16),
                (508, 16..32),
                (508, 32..48),
                (509, 32..48),
            ],
            "{k}: destination cells"
        );

        // Slot-0 park-strip complement. Drake ships the full six-record set
        // (rows 498/499/502..505; rows 500/501/508 arrive as plain TIM
        // CLUTs); Sebucus / Karisto ship only {501, 503, 505} and inherit
        // the kingdom-invariant rest as VRAM residue from the Drake upload.
        let slot0 = kingdom_bundle::decode_slot(&buf, 0).expect("decode slot 0");
        let strips = clut_walk::park_strips(&slot0);
        let mut rows: Vec<u16> = strips.iter().map(|s| s.fb_y).collect();
        rows.sort_unstable();
        let expected_rows: Vec<u16> = if k == kingdoms[0] {
            vec![498, 499, 502, 503, 504, 505]
        } else {
            vec![501, 503, 505]
        };
        assert_eq!(rows, expected_rows, "{k}: park-strip rows");
        for s in &strips {
            assert_eq!(
                (s.fb_x, s.w, s.h),
                (0, 256, 1),
                "{k}: strip rect at row {}",
                s.fb_y
            );
            assert_eq!(s.data.len(), 512, "{k}: strip bytes at row {}", s.fb_y);
        }
        if k == kingdoms[0] {
            drake_strip_rows = rows;
        }
        // Every walk source cell must be covered by the scene's own strips,
        // one of its slot-0 TIM CLUT rows, or the Drake strip set (the
        // engine's park order: scene strips first, Drake complement for
        // whatever is left).
        let tim_clut_rows = slot0_tim_clut_rows(&slot0);
        for e in &table.entries {
            for f in &e.frames {
                let covered = strips
                    .iter()
                    .any(|s| s.fb_y == f.src_y && f.src_x + clut_walk::COPY_WIDTH <= s.fb_x + s.w)
                    || tim_clut_rows.contains(&f.src_y)
                    || drake_strip_rows.contains(&f.src_y);
                assert!(
                    covered,
                    "{k}: source cell ({}, {}) not covered by a park strip / TIM CLUT",
                    f.src_x, f.src_y
                );
            }
        }
    }
}

/// Rows on which this slot-0 pack declares an ordinary TIM CLUT (the TIM
/// upload pass parks those; only the non-TIM CLUT-block records need the
/// dedicated strip pass).
fn slot0_tim_clut_rows(slot0: &[u8]) -> Vec<u16> {
    let mut rows = Vec::new();
    let Some(count) = slot0
        .get(0..4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
    else {
        return rows;
    };
    for k in 0..count {
        let Some(woff) = slot0
            .get(4 + k * 4..8 + k * 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
        else {
            break;
        };
        let bo = woff.saturating_mul(4);
        if bo >= slot0.len() {
            continue;
        }
        if let Ok(tim) = legaia_tim::parse(&slot0[bo..])
            && let Some(c) = tim.clut.as_ref()
        {
            rows.push(c.fb_y);
        }
    }
    rows
}
