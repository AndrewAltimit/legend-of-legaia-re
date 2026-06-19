//! Disc-gated coverage test.
//!
//! The extracted PROT files include both each entry's TOC-indexed payload
//! AND, for entries with a trailing gap, the unindexed sectors the boot
//! loader reads past the indexed end (carrying trailing-overlay code - see
//! `docs/subsystems/boot.md`). Coverage is reported in two flavors:
//!
//! - **Indexed coverage** (the assertion): classify only each entry's
//!   indexed sub-region. This is the historical invariant - every TOC-
//!   declared PROT byte should fall into a known PROT-format class.
//!   Threshold: >= 99 %.
//! - **Extended coverage** (informational): classify each entry's full
//!   on-disc footprint, including the trailing-overlay sectors. These
//!   bytes are MIPS code that legitimately doesn't fit any PROT-format
//!   detector, so the extended coverage is expected to be lower; it's
//!   logged but not asserted.
//!
//! Skips silently when `extracted/PROT.DAT` or `LEGAIA_DISC_BIN` is missing.
//!
//! What the indexed assertion catches:
//!  - A detector regression that pushes Unknown* bytes above the 1 % budget.
//!  - A new PROT format cluster that was missed by all detectors.
//!  - Accidental narrowing of an existing detector that drops its class
//!    into Unknown*.

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

    let mut indexed_class_bytes: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut extended_class_bytes: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut total_indexed = 0u64;
    let mut total_extended = 0u64;
    let mut entry_buf = Vec::new();

    let entries = archive.entries.clone();
    for entry in &entries {
        // Extended classification (full footprint, including any trailing
        // overlay sectors).
        archive
            .read_entry(entry, &mut entry_buf)
            .expect("read entry extended");
        let ext_report = classify(&entry_buf);
        *extended_class_bytes
            .entry(ext_report.class.name())
            .or_insert(0) += entry_buf.len() as u64;
        total_extended += entry_buf.len() as u64;

        // Indexed classification (TOC-declared payload only).
        archive
            .read_entry_indexed(entry, &mut entry_buf)
            .expect("read entry indexed");
        let ix_report = classify(&entry_buf);
        *indexed_class_bytes
            .entry(ix_report.class.name())
            .or_insert(0) += entry_buf.len() as u64;
        total_indexed += entry_buf.len() as u64;
    }

    eprintln!(
        "[categorize] {} entries, {} indexed bytes, {} extended bytes (+{} trailing)",
        entries.len(),
        total_indexed,
        total_extended,
        total_extended - total_indexed,
    );

    let unknown_names = [
        Class::UnknownHighEntropy.name(),
        Class::UnknownLowEntropy.name(),
        Class::UnknownOther.name(),
    ];

    eprintln!("[categorize] indexed breakdown:");
    for (name, bytes) in &indexed_class_bytes {
        let pct = (*bytes as f64 / total_indexed as f64) * 100.0;
        eprintln!("[categorize]   {name}: {bytes} bytes ({pct:.2}%)");
    }
    let indexed_unknown: u64 = unknown_names
        .iter()
        .filter_map(|n| indexed_class_bytes.get(*n).copied())
        .sum();
    let indexed_coverage = 1.0 - (indexed_unknown as f64 / total_indexed as f64);
    eprintln!(
        "[categorize] indexed coverage  {:.1}% ({} known / {} total)",
        indexed_coverage * 100.0,
        total_indexed - indexed_unknown,
        total_indexed,
    );

    eprintln!("[categorize] extended breakdown:");
    for (name, bytes) in &extended_class_bytes {
        let pct = (*bytes as f64 / total_extended as f64) * 100.0;
        eprintln!("[categorize]   {name}: {bytes} bytes ({pct:.2}%)");
    }
    let extended_unknown: u64 = unknown_names
        .iter()
        .filter_map(|n| extended_class_bytes.get(*n).copied())
        .sum();
    let extended_coverage = 1.0 - (extended_unknown as f64 / total_extended as f64);
    eprintln!(
        "[categorize] extended coverage {:.1}% ({} known / {} total)  [informational]",
        extended_coverage * 100.0,
        total_extended - extended_unknown,
        total_extended,
    );

    assert!(
        indexed_coverage >= 0.99,
        "indexed categorize coverage {:.1}% < 99% (Unknown*: {} / {} bytes)",
        indexed_coverage * 100.0,
        indexed_unknown,
        total_indexed,
    );
}
