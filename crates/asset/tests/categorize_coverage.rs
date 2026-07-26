//! Disc-gated coverage test: how much of PROT.DAT falls into a known format
//! class.
//!
//! An entry's size is the sector gap to its successor (`toc[p+3] - toc[p+2]`,
//! retail's own `FUN_8003E68C`) - so **the footprint is the whole entry**, and
//! classifying it is the invariant: every PROT byte should land in a named
//! class. Threshold: >= 99 %.
//!
//! The superseded `toc[p+5] - toc[p+3] + 4` window is measured alongside it as a
//! **contrast, not a second flavour of the same number**. That expression
//! measures the two entries *after* `p`, so its window neither starts nor ends
//! where entry `p` does: it over-reads into the neighbours for most of the
//! archive and truncates the entry for the rest, which is why its coverage is
//! the lower of the two and why its total is not a superset of the footprint's.
//! It is logged, never asserted. (The pre-correction reading of this file had
//! the two the other way round - it called the over-read "indexed", asserted on
//! it, and printed `extended - indexed` as a trailing-sector count, which
//! underflowed once the footprint became the smaller of the two.)
//!
//! Skips silently when `extracted/PROT.DAT` or `LEGAIA_DISC_BIN` is missing.
//!
//! What the assertion catches:
//!  - A detector regression that pushes Unknown* bytes above the 1 % budget.
//!  - A new PROT format cluster that was missed by all detectors.
//!  - Accidental narrowing of an existing detector that drops its class
//!    into Unknown*.
//!
//! Its sibling `crates/extract/tests/validation_suite.rs` pins the exact
//! per-class entry counts; this one measures bytes.

use legaia_asset::categorize::{Class, classify};
use legaia_prot::archive::Archive;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn extracted_prot_dat() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("extracted/PROT.DAT"),
        PathBuf::from("../../extracted/PROT.DAT"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

#[test]
fn categorize_coverage() {
    let Some(prot_dat) = extracted_prot_dat() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut archive = Archive::open(&prot_dat).expect("open PROT.DAT");

    let mut footprint_class_bytes: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut span_class_bytes: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut total_footprint = 0u64;
    let mut total_span = 0u64;
    let mut entry_buf = Vec::new();

    let entries = archive.entries.clone();
    for entry in &entries {
        // The entry's real footprint: the sector gap to its successor.
        archive
            .read_entry(entry, &mut entry_buf)
            .expect("read entry footprint");
        let footprint_report = classify(&entry_buf);
        *footprint_class_bytes
            .entry(footprint_report.class.name())
            .or_insert(0) += entry_buf.len() as u64;
        total_footprint += entry_buf.len() as u64;

        // The same classification over the superseded `toc[p+5] - toc[p+3] +
        // 4` window, which measures entry `p`'s two successors rather than
        // `p` - kept as the contrast the coverage numbers are read against.
        archive
            .read_entry_declared_span(entry, &mut entry_buf)
            .expect("read entry declared span");
        let span_report = classify(&entry_buf);
        *span_class_bytes
            .entry(span_report.class.name())
            .or_insert(0) += entry_buf.len() as u64;
        total_span += entry_buf.len() as u64;
    }

    // Signed, because neither total contains the other: the superseded window
    // over-reads most entries and truncates the rest, so which side is larger
    // is a property of the archive, not an invariant to lean on.
    eprintln!(
        "[categorize] {} entries, {} footprint bytes, {} superseded-window bytes ({:+})",
        entries.len(),
        total_footprint,
        total_span,
        total_span as i64 - total_footprint as i64,
    );

    let unknown_names = [
        Class::UnknownHighEntropy.name(),
        Class::UnknownLowEntropy.name(),
        Class::UnknownOther.name(),
    ];

    eprintln!("[categorize] footprint breakdown:");
    for (name, bytes) in &footprint_class_bytes {
        let pct = (*bytes as f64 / total_footprint as f64) * 100.0;
        eprintln!("[categorize]   {name}: {bytes} bytes ({pct:.2}%)");
    }
    let footprint_unknown: u64 = unknown_names
        .iter()
        .filter_map(|n| footprint_class_bytes.get(*n).copied())
        .sum();
    let footprint_coverage = 1.0 - (footprint_unknown as f64 / total_footprint as f64);
    eprintln!(
        "[categorize] footprint coverage         {:.1}% ({} known / {} total)",
        footprint_coverage * 100.0,
        total_footprint - footprint_unknown,
        total_footprint,
    );

    eprintln!("[categorize] superseded-window breakdown:");
    for (name, bytes) in &span_class_bytes {
        let pct = (*bytes as f64 / total_span as f64) * 100.0;
        eprintln!("[categorize]   {name}: {bytes} bytes ({pct:.2}%)");
    }
    let span_unknown: u64 = unknown_names
        .iter()
        .filter_map(|n| span_class_bytes.get(*n).copied())
        .sum();
    let span_coverage = 1.0 - (span_unknown as f64 / total_span as f64);
    eprintln!(
        "[categorize] superseded-window coverage {:.1}% ({} known / {} total)  [informational]",
        span_coverage * 100.0,
        total_span - span_unknown,
        total_span,
    );

    assert!(
        footprint_coverage >= 0.99,
        "categorize coverage {:.1}% < 99% over the entry footprints (Unknown*: {} / {} bytes)",
        footprint_coverage * 100.0,
        footprint_unknown,
        total_footprint,
    );

    // The contrast has to stay a contrast: if the superseded window ever scored
    // as well as the footprint, the two readers would have converged and this
    // file's second half would be measuring nothing.
    assert!(
        span_coverage < footprint_coverage,
        "the superseded `toc[p+5] - toc[p+3] + 4` window scored {:.1}%, at or above the \
         footprint's {:.1}% - the two readers are no longer distinguishable, so either \
         the correction was undone or this contrast is dead weight",
        span_coverage * 100.0,
        footprint_coverage * 100.0,
    );
}
