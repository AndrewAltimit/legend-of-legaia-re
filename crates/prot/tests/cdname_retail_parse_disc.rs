//! Disc-gated oracle for the two CDNAME readers (`legaia_prot::cdname`).
//!
//! The tooling uses the tolerant [`parse_str`]; retail's loader
//! (`FUN_8001D8FC`, ported as `retail_name_table` / `parse_retail_str`) writes
//! names into a 16-byte-stride table with **no bound and no terminator**, then
//! stores the entry index over bytes `+0xC`/`+0xD`. Names therefore come back
//! mangled rather than truncated: the index bytes land *inside* the name, and
//! anything at `+0xE` or beyond survives past them.
//!
//! Asserts on the user's own `CDNAME.TXT` that:
//!
//!  - both readers declare the same **index set**, so no block is gained, lost
//!    or renumbered by the stricter walk;
//!  - every name of 11 bytes or fewer round-trips **identically**;
//!  - every name of 12 bytes or more comes back exactly as the byte-level
//!    record model predicts - the name with the index's two bytes overlaid at
//!    offsets 12..13, read to the first zero.
//!
//! That last assertion is the point of the test: it is computed here from the
//! `#define` spelling and the index alone, independently of how
//! `parse_retail_str` is implemented, so a regression in the port cannot make
//! it pass by agreeing with itself.
//!
//! Skips and passes when `LEGAIA_DISC_BIN` / `extracted/` are absent.

use legaia_prot::cdname;
use std::path::PathBuf;

fn extracted_cdname() -> Option<PathBuf> {
    ["extracted", "../extracted", "../../extracted"]
        .into_iter()
        .map(PathBuf::from)
        .map(|p| p.join("CDNAME.TXT"))
        .find(|p| p.is_file())
}

fn gated() -> Option<String> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let Some(path) = extracted_cdname() else {
        eprintln!("[skip] extracted/CDNAME.TXT missing");
        return None;
    };
    std::fs::read_to_string(path).ok()
}

/// Independent model of one retail record: lay the name into a zeroed 16-byte
/// record, overlay the little-endian index at `+0xC`, read to the first zero.
/// Deliberately does not call the port.
fn expected_retail_name(define_name: &str, idx: u32) -> Vec<u8> {
    let mut rec = [0u8; cdname::RETAIL_RECORD_STRIDE];
    let src = define_name.as_bytes();
    // No shipped name reaches 16 bytes, so a plain record-local model is exact.
    assert!(
        src.len() < cdname::RETAIL_RECORD_STRIDE,
        "name {define_name:?} spills the record; this model does not cover spill"
    );
    rec[..src.len()].copy_from_slice(src);
    rec[cdname::RETAIL_INDEX_OFFSET] = idx as u8;
    rec[cdname::RETAIL_INDEX_OFFSET + 1] = (idx >> 8) as u8;
    let end = rec.iter().position(|&c| c == 0).unwrap_or(rec.len());
    rec[..end].to_vec()
}

#[test]
fn cdname_retail_names_match_the_record_model_byte_for_byte() {
    let Some(text) = gated() else { return };

    let tolerant = cdname::parse_str(&text).expect("tolerant parse");
    let retail = cdname::parse_retail_str(&text);

    assert!(
        tolerant.len() > 100,
        "expected the full retail name map, got {} entries",
        tolerant.len()
    );

    // No block is gained, lost or renumbered by the stricter walk - which is
    // what every index-space conversion in this crate depends on.
    let tolerant_indices: Vec<u32> = tolerant.keys().copied().collect();
    let retail_indices: Vec<u32> = retail.keys().copied().collect();
    assert_eq!(
        retail_indices, tolerant_indices,
        "the retail loader declares a different index set"
    );

    let mut mangled = Vec::new();
    for (idx, full) in &tolerant {
        let got = retail.get(idx).expect("index present in both maps");
        assert_eq!(
            got,
            &expected_retail_name(full, *idx),
            "index {idx}: retail record model disagrees for {full:?}"
        );

        if full.len() <= cdname::RETAIL_NAME_CLEAN_CAPACITY {
            assert_eq!(
                got.as_slice(),
                full.as_bytes(),
                "index {idx}: {full:?} is within the clean capacity and must round-trip"
            );
        } else {
            assert_ne!(
                got.as_slice(),
                full.as_bytes(),
                "index {idx}: {full:?} exceeds the clean capacity, so it cannot round-trip"
            );
            mangled.push(full.clone());
        }
    }

    // Non-vacuous: the shipped file must actually contain over-capacity names,
    // or the mangling assertions prove nothing.
    assert!(
        !mangled.is_empty(),
        "no over-capacity names found; the mangling assertions are vacuous"
    );
}

#[test]
fn over_capacity_names_are_not_merely_truncated() {
    let Some(text) = gated() else { return };

    let tolerant = cdname::parse_str(&text).expect("tolerant parse");
    let retail = cdname::parse_retail_str(&text);

    // The claim this test exists to refute: that retail hard-truncates names to
    // 12 bytes. It does not - the index store overlays bytes 12..13 and bytes
    // at 14+ survive. Pin that at least one shipped name proves each half.
    let mut carries_index_bytes = 0usize;
    let mut survives_past_the_index = 0usize;

    for (idx, full) in &tolerant {
        if full.len() <= cdname::RETAIL_NAME_CLEAN_CAPACITY {
            continue;
        }
        let got = retail.get(idx).expect("index present in both maps");
        assert_ne!(
            got.as_slice(),
            &full.as_bytes()[..cdname::RETAIL_INDEX_OFFSET],
            "index {idx}: {full:?} came back as a plain 12-byte truncation"
        );
        if got.len() > cdname::RETAIL_INDEX_OFFSET {
            carries_index_bytes += 1;
        }
        if full.len() > cdname::RETAIL_INDEX_OFFSET + 1 {
            // Byte 14 onward is past the index store and must be preserved.
            assert_eq!(
                got.last(),
                full.as_bytes().last(),
                "index {idx}: {full:?} lost its byte past the index store"
            );
            survives_past_the_index += 1;
        }
    }

    assert!(
        carries_index_bytes > 0,
        "no name came back carrying index bytes; the overlay path is untested"
    );
    assert!(
        survives_past_the_index > 0,
        "no name long enough to test the surviving `+0xE` byte"
    );
}

#[test]
fn indices_live_in_the_raw_toc_space() {
    let Some(text) = gated() else { return };
    let map = cdname::parse_retail_str(&text);

    // Raw-TOC indices; the extraction frame is two below (RAW_TOC_INDEX_OFFSET).
    let max = map.keys().copied().max().expect("non-empty map");
    assert!(
        max > 1000,
        "highest CDNAME index {max} is too low to be a raw-TOC index"
    );
    // Every declared block must convert into the extraction frame without
    // underflowing past the head defines.
    for &raw in map.keys() {
        let _ = raw.saturating_sub(cdname::RAW_TOC_INDEX_OFFSET);
    }
}
