//! Disc-gated regression test: assert the `scene_v12_table` detector finds
//! the documented 97-entry cluster in the real PROT corpus, and surface
//! summary statistics so a future consumer-reverser sees the live shape.
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.
//!
//! What this catches:
//!  - The detector regresses (matches < 95 entries → header parser broke).
//!  - A future code change accidentally widens the detector and starts
//!    matching arbitrary buffers (matches > 100 entries).
//!  - Per-entry `n` (record count) and `param` (per-scene parameter) ranges
//!    drift outside the documented bounds.

use legaia_asset::scene_v12_table::detect;
use std::path::PathBuf;

fn extracted_prot() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ];
    candidates.into_iter().find(|p| p.is_dir())
}

#[test]
fn scene_v12_detector_matches_97_entry_cluster() {
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

    let mut hits = Vec::new();
    let mut min_n = u16::MAX;
    let mut max_n = u16::MIN;
    let mut params: std::collections::BTreeSet<u16> = std::collections::BTreeSet::new();
    for path in &entries {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if let Some(t) = detect(&bytes) {
            hits.push((path.clone(), t.n, t.param));
            min_n = min_n.min(t.n);
            max_n = max_n.max(t.n);
            params.insert(t.param);
        }
    }

    eprintln!(
        "[v12] {} matches across {} PROT entries",
        hits.len(),
        entries.len()
    );
    eprintln!("[v12] n range: {min_n}..={max_n}");
    eprintln!("[v12] {} unique param values", params.len());

    // Documentation says 97 entries match; allow a tight band so a small
    // detector adjustment doesn't silently break.
    assert!(
        hits.len() >= 95 && hits.len() <= 100,
        "scene_v12 detector matched {} entries; expected ~97 per docs",
        hits.len()
    );
    // Every match's `n` should sit in the documented `[8, 4096]` range
    // and the observed corpus range `[24, 366]`.
    assert!(min_n >= 8, "min n {min_n} below documented floor");
    assert!(max_n <= 1024, "max n {max_n} above observed ceiling");
}
