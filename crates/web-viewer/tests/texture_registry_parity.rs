//! Behaviour-preservation guard for the texture-family registry.
//!
//! The ROM patcher's replacement grid used to be three hand-written loops
//! inside the WASM `scan_textures` binding. Those loops are now
//! [`legaia_web_viewer::texture_registry`] tiers. This test carries the
//! **pre-refactor algorithm verbatim** (see [`legacy_rows`]) and asserts the
//! registry emits exactly the same rows, in exactly the same order, for the
//! three families that existed before: same coordinates, same dimensions,
//! same palette counts, same byte lengths, same curated labels.
//!
//! That is the point of the file. A refactor of a scan whose only consumer is
//! a browser grid is otherwise unfalsifiable - "it still lists textures" is
//! not evidence. The legacy loops are deliberately duplicated here rather
//! than called: if the registry drifts, the copy is the thing that disagrees
//! with it.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset, matching the rest of the
//! disc-dependent test suite. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use std::env;
use std::fs;

use legaia_web_viewer::texture_registry::{
    self as reg, Rgba, ScanCtx, TIER_LZS, TIER_RAW, TIER_SAVE_ICON, TIER_SUMMON, TexRow,
};

/// The comparable shape of one grid row.
#[derive(Debug, PartialEq, Eq)]
struct Row {
    tier: &'static str,
    entry: i64,
    section: i64,
    offset: u64,
    width: u32,
    height: u32,
    bpp: u32,
    cluts: usize,
    bytes: usize,
    label: Option<&'static str>,
}

impl From<&TexRow> for Row {
    fn from(r: &TexRow) -> Self {
        Row {
            tier: r.coord.tier,
            entry: r.coord.entry,
            section: r.coord.section,
            offset: r.coord.offset,
            width: r.width,
            height: r.height,
            bpp: r.bpp,
            cluts: r.cluts,
            bytes: r.bytes,
            label: r.label,
        }
    }
}

/// The scan exactly as it was written before the registry existed: the raw
/// TIM catalog loop, then the deep (LZS) catalog loop grouped by hosting
/// entry, then the save-slot portrait loop. Copied, not called.
fn legacy_rows(prot: &[u8], spans: &[(u64, u64, u32)]) -> Vec<Row> {
    let mut out = Vec::new();
    let raw = legaia_asset::tim_catalog::build_from_spans(prot, spans);
    let deep = legaia_asset::tim_deep_catalog::build_from_spans(prot, spans);

    for t in &raw {
        // A catalog row that no longer strict-parses emitted no grid row.
        let Ok(_tim) = legaia_tim::parse_strict(&prot[t.abs_offset as usize..]) else {
            continue;
        };
        out.push(Row {
            tier: "raw",
            entry: t.entry_index.map(|e| e as i64).unwrap_or(-1),
            section: -1,
            offset: t.offset_in_entry,
            width: t.width,
            height: t.height,
            bpp: t.bpp,
            cluts: t.clut_count,
            bytes: t.byte_len,
            label: t.label,
        });
    }

    // Deep tier: decompress each hosting entry once, decode its rows. Every
    // catalog row emitted a grid row here, decode failure or not.
    let mut i = 0usize;
    while i < deep.len() {
        let entry = deep[i].entry_index;
        let end = deep[i..]
            .iter()
            .position(|t| t.entry_index != entry)
            .map(|p| i + p)
            .unwrap_or(deep.len());
        for t in &deep[i..end] {
            out.push(Row {
                tier: "lzs",
                entry: t.entry_index as i64,
                section: t.lzs_section as i64,
                offset: t.offset_in_section,
                width: t.width,
                height: t.height,
                bpp: t.bpp,
                cluts: t.clut_count,
                bytes: t.byte_len,
                label: t.label,
            });
        }
        i = end;
    }

    // Save-slot portraits: fifteen, not sixteen.
    let entry = legaia_asset::save_icon::PROT_ENTRY as u32;
    if let Some(&(off, len, _)) = spans.iter().find(|&&(_, _, idx)| idx == entry)
        && let Some(bytes) = prot.get(off as usize..(off + len) as usize)
        && let Ok(sheet) = legaia_asset::save_icon::parse_entry(bytes)
    {
        for slot in 0..legaia_asset::save_icon::USABLE_TILE_COUNT {
            if sheet.tile_rgba(slot).is_err() {
                continue;
            }
            out.push(Row {
                tier: "save-icon",
                entry: entry as i64,
                section: slot as i64,
                offset: sheet.tile_clut_offset(slot) as u64,
                width: legaia_asset::save_icon::TILE_SIZE as u32,
                height: legaia_asset::save_icon::TILE_SIZE as u32,
                bpp: 4,
                cluts: 1,
                bytes: legaia_asset::save_icon::TILE_BLOCK_BYTES
                    + legaia_asset::save_icon::TILE_CLUT_BYTES,
                label: Some("save-slot portrait"),
            });
        }
    }
    out
}

/// Everything the registry emits, with the pixels it decoded folded into a
/// fingerprint so a whole-disc decode never has to be held in memory at once.
fn registry_rows(
    prot: &[u8],
    spans: &[(u64, u64, u32)],
    want_pixels: bool,
) -> (Vec<Row>, Vec<u64>) {
    let ctx = ScanCtx::new(prot, spans);
    let mut rows = Vec::new();
    let mut pixel_hashes = Vec::new();
    let mut sink = |row: TexRow, rgba: Option<Rgba>| -> Result<(), String> {
        pixel_hashes.push(match &rgba {
            Some(img) => reg::fnv1a64(&img.data),
            None => 0,
        });
        rows.push(Row::from(&row));
        Ok(())
    };
    reg::scan_all(&ctx, want_pixels, &mut sink).expect("scan");
    (rows, pixel_hashes)
}

fn disc() -> Option<Vec<u8>> {
    let path = env::var("LEGAIA_DISC_BIN").ok()?;
    if path.is_empty() {
        return None;
    }
    fs::read(path).ok()
}

fn prot_and_spans(image: &[u8]) -> (Vec<u8>, Vec<(u64, u64, u32)>) {
    let prot = legaia_iso::iso9660::read_file_in_image(image, "PROT.DAT").expect("PROT.DAT");
    let archive = legaia_prot::archive::Archive::from_bytes(prot.clone()).expect("TOC");
    let spans = archive
        .entries
        .iter()
        .map(|e| (e.byte_offset, e.size_bytes, e.index))
        .collect();
    (prot, spans)
}

#[test]
fn registry_reproduces_the_pre_refactor_scan_row_for_row() {
    let Some(image) = disc() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let (prot, spans) = prot_and_spans(&image);
    drop(image);

    let expected = legacy_rows(&prot, &spans);
    let (got_all, _) = registry_rows(&prot, &spans, false);
    // The three original families, in the order they were emitted before.
    let got: Vec<&Row> = got_all
        .iter()
        .filter(|r| matches!(r.tier, TIER_RAW | TIER_LZS | TIER_SAVE_ICON))
        .collect();

    assert_eq!(
        got.len(),
        expected.len(),
        "the registry emits {} rows for the three original families; the \
         pre-refactor scan emitted {}",
        got.len(),
        expected.len()
    );
    assert!(expected.len() > 1000, "sanity: a retail disc has many TIMs");

    for (i, (a, b)) in got.iter().zip(expected.iter()).enumerate() {
        assert_eq!(**a, *b, "row {i} differs from the pre-refactor scan");
    }
}

#[test]
fn the_original_families_still_come_first_and_in_order() {
    let Some(image) = disc() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let (prot, spans) = prot_and_spans(&image);
    drop(image);
    let (rows, _) = registry_rows(&prot, &spans, false);

    // The page pages through this list, so tier order is observable. New
    // families append; they must never interleave with the old ones.
    let mut order: Vec<&str> = Vec::new();
    for r in &rows {
        if order.last() != Some(&r.tier) {
            assert!(
                !order.contains(&r.tier),
                "tier {:?} is emitted in more than one run",
                r.tier
            );
            order.push(r.tier);
        }
    }
    assert_eq!(
        &order[..3],
        &[TIER_RAW, TIER_LZS, TIER_SAVE_ICON],
        "the three original families must stay first, in their original order"
    );
}

#[test]
fn row_set_is_independent_of_whether_thumbnails_were_asked_for() {
    let Some(image) = disc() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let (prot, spans) = prot_and_spans(&image);
    drop(image);
    let (without, _) = registry_rows(&prot, &spans, false);
    let (with, hashes) = registry_rows(&prot, &spans, true);
    assert_eq!(without, with, "asking for pixels must not change the rows");
    assert!(
        hashes.iter().filter(|h| **h != 0).count() > 1000,
        "most rows should decode to pixels"
    );
}

#[test]
fn every_row_can_be_read_back_by_its_own_coordinate() {
    let Some(image) = disc() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let (prot, spans) = prot_and_spans(&image);
    drop(image);

    // A coordinate that the grid shows but that cannot be resolved back to
    // the same pixels is a row a change pack could not pin. Sample across
    // every family rather than decoding the whole disc twice.
    let ctx = ScanCtx::new(&prot, &spans);
    let mut checked = std::collections::BTreeMap::<&str, usize>::new();
    let mut seen_per_tier = std::collections::BTreeMap::<&str, usize>::new();
    let mut sink = |row: TexRow, rgba: Option<Rgba>| -> Result<(), String> {
        let n = seen_per_tier.entry(row.coord.tier).or_default();
        *n += 1;
        // Every 97th row of each family - a prime stride, so the sample is
        // not aligned to any entry boundary.
        if *n % 97 != 1 {
            return Ok(());
        }
        let Some(img) = rgba else { return Ok(()) };
        let back = reg::read_row(&ctx, &row.coord)
            .unwrap_or_else(|e| panic!("re-reading {:?} failed: {e}", row.coord));
        assert_eq!(
            (back.w, back.h, reg::fnv1a64(&back.data)),
            (img.w, img.h, reg::fnv1a64(&img.data)),
            "re-reading {:?} by coordinate gave different pixels",
            row.coord
        );
        *checked.entry(row.coord.tier).or_default() += 1;
        Ok(())
    };
    reg::scan_all(&ctx, true, &mut sink).expect("scan");

    for t in reg::tiers() {
        if seen_per_tier.get(t.id).copied().unwrap_or(0) > 0 {
            assert!(
                checked.get(t.id).copied().unwrap_or(0) > 0,
                "family {:?} was listed but never round-tripped",
                t.id
            );
        }
    }
}

#[test]
fn the_summon_family_reaches_textures_no_tim_scan_can() {
    let Some(image) = disc() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let (prot, spans) = prot_and_spans(&image);
    drop(image);
    let (rows, _) = registry_rows(&prot, &spans, false);

    let summon: Vec<&Row> = rows.iter().filter(|r| r.tier == TIER_SUMMON).collect();
    assert!(
        !summon.is_empty(),
        "PROT 893/894 should yield summon texture pages"
    );
    // The whole justification for the tier: both TIM catalogs are blind to
    // these entries, so before this family existed no filter string could
    // reach a single one of them.
    for entry in [893i64, 894] {
        let tim_rows = rows
            .iter()
            .filter(|r| matches!(r.tier, TIER_RAW | TIER_LZS) && r.entry == entry)
            .count();
        assert_eq!(
            tim_rows, 0,
            "PROT {entry} is supposed to hold no TIM at all - if it now does, \
             the summon tier may be duplicating rows the TIM tiers already show"
        );
        assert!(
            summon.iter().any(|r| r.entry == entry),
            "PROT {entry} should contribute summon texture pages"
        );
    }
    for r in &summon {
        assert_eq!(r.bpp, 4);
        assert_eq!(r.height, 256);
        assert!(r.width == 256 || r.width == 512, "width {}", r.width);
    }
}
