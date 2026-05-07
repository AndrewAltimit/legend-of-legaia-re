//! Disc-gated coverage test: asserts that >= 99% of PROT bytes classify to a
//! non-Unknown* class using `legaia_asset::categorize::classify`.
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.
//!
//! What this catches:
//!  - A detector regression that pushes Unknown* bytes above the 1% budget.
//!  - A new PROT format cluster that was missed by all detectors.
//!  - Accidental narrowing of an existing detector that drops its class
//!    into Unknown*.

use legaia_asset::categorize::{Class, classify};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn extracted_prot() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ];
    candidates.into_iter().find(|p| p.is_dir())
}

#[test]
fn categorize_coverage() {
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut entries: Vec<PathBuf> = std::fs::read_dir(&prot)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("BIN"))
        .collect();
    entries.sort();

    let mut class_bytes: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut total_bytes = 0u64;

    for path in &entries {
        let Ok(buf) = std::fs::read(path) else {
            continue;
        };
        let report = classify(&buf);
        *class_bytes.entry(report.class.name()).or_insert(0) += buf.len() as u64;
        total_bytes += buf.len() as u64;
    }

    eprintln!(
        "[categorize] {} entries, {} total bytes",
        entries.len(),
        total_bytes
    );
    for (name, bytes) in &class_bytes {
        let pct = if total_bytes > 0 {
            (*bytes as f64 / total_bytes as f64) * 100.0
        } else {
            0.0
        };
        eprintln!("[categorize]   {name}: {bytes} bytes ({pct:.2}%)");
    }

    let unknown_bytes: u64 = [
        Class::UnknownHighEntropy.name(),
        Class::UnknownLowEntropy.name(),
        Class::UnknownOther.name(),
    ]
    .iter()
    .filter_map(|name| class_bytes.get(*name).copied())
    .sum();

    let coverage = if total_bytes > 0 {
        1.0 - (unknown_bytes as f64 / total_bytes as f64)
    } else {
        1.0
    };

    eprintln!(
        "[categorize] coverage {:.1}% ({} / {} bytes known)",
        coverage * 100.0,
        total_bytes - unknown_bytes,
        total_bytes
    );

    assert!(
        coverage >= 0.99,
        "categorize coverage {:.1}% < 99% (Unknown*: {} / {} bytes)",
        coverage * 100.0,
        unknown_bytes,
        total_bytes,
    );
}
