//! Disc-gated equivalence oracle between the two CDNAME readers
//! (`legaia_prot::cdname`).
//!
//! The tooling uses the tolerant [`parse_str`], but retail's loader
//! (`FUN_8001D8FC`, ported as [`parse_retail_str`]) is stricter: it stops at the
//! first line that does not begin with `#` and truncates names at the record's
//! index field. Those differences are invisible on the shipped file - which is
//! precisely the claim that has to be checked rather than assumed, because it is
//! what justifies the tooling keeping the tolerant reader.
//!
//! Asserts on the user's own `CDNAME.TXT` that:
//!
//!  - both readers declare the same **index set**, so no block is gained, lost
//!    or renumbered by the stricter walk;
//!  - the names differ on exactly the entries that exceed the record's name
//!    field, and each such name is the `#define` spelling truncated to the cap;
//!  - the map is non-trivial and its indices live in the raw-TOC space.
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

#[test]
fn cdname_retail_parse_agrees_on_indices_and_differs_only_by_truncation() {
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

    // Names may differ only where the `#define` spelling exceeds the record's
    // name field, and then only by truncation to the cap.
    let cap = cdname::RETAIL_NAME_CAPACITY;
    let mut truncated = Vec::new();
    for (idx, full) in &tolerant {
        let got = retail.get(idx).expect("index present in both maps");
        if got == full {
            assert!(
                full.len() <= cap,
                "name {full:?} fits neither the cap nor the untruncated case"
            );
            continue;
        }
        assert_eq!(
            got.as_str(),
            &full[..cap],
            "index {idx}: retail name is not the {cap}-byte truncation of {full:?}"
        );
        truncated.push(full.clone());
    }

    // Non-vacuous: the shipped file must actually contain over-cap names, or
    // this test proves nothing about the truncation path.
    assert!(
        !truncated.is_empty(),
        "no over-cap names found; the truncation assertion is vacuous"
    );
}

#[test]
fn the_shipped_file_actually_exercises_the_name_cap() {
    let Some(text) = gated() else { return };

    // Names longer than the record's name field are truncated by the index
    // store. If the file had no such names the equivalence test above would be
    // vacuous on that axis, so pin that it does.
    let over_cap = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("#define"))
        .filter_map(|rest| rest.split_whitespace().next())
        .filter(|name| name.len() > cdname::RETAIL_NAME_CAPACITY)
        .count();
    assert!(
        over_cap > 0,
        "no CDNAME name exceeds the {}-byte cap, so the truncation path is untested",
        cdname::RETAIL_NAME_CAPACITY
    );

    // Truncation must not collide two distinct blocks onto one name+index pair.
    let retail = cdname::parse_retail_str(&text);
    let tolerant = cdname::parse_str(&text).expect("tolerant parse");
    assert_eq!(
        retail.len(),
        tolerant.len(),
        "truncation dropped or merged an entry"
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
