//! Disc-gated regression test: assert the `scene_v12_table` detector finds
//! the documented 97-entry cluster in the real PROT corpus, and that every
//! detected entry carries a valid event-script prescript at offset 0x800.
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.
//!
//! What this catches:
//!  - The detector regresses (matches < 95 entries → header parser broke).
//!  - A future code change accidentally widens the detector (> 100 hits).
//!  - The `N = 4*param + 22` algebra breaks (= the runtime fixup-slot
//!    layout has shifted).
//!  - The event-script prescript at offset 0x800 stops parsing (= the
//!    cross-format integration with `scene_event_scripts` is broken).

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

    let mut hits = 0usize;
    let mut with_scripts = 0usize;
    let mut frame_opener_50pct = 0usize;
    let mut min_n = u16::MAX;
    let mut max_n = u16::MIN;
    let mut min_param = u16::MAX;
    let mut max_param = u16::MIN;
    let mut params: std::collections::BTreeSet<u16> = std::collections::BTreeSet::new();
    let mut max_scripts = 0usize;

    for path in &entries {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if let Some(t) = detect(&bytes) {
            hits += 1;
            min_n = min_n.min(t.n);
            max_n = max_n.max(t.n);
            min_param = min_param.min(t.param);
            max_param = max_param.max(t.param);
            params.insert(t.param);

            // Algebraic tie - every retail entry satisfies it.
            assert_eq!(
                t.n,
                4 * t.param + 22,
                "n/param algebra broken at {}: n={}, param={}",
                path.display(),
                t.n,
                t.param
            );
            // Inline records vec length matches param.
            assert_eq!(
                t.records.len(),
                t.param as usize,
                "inline records vec length mismatch at {}",
                path.display()
            );
            // All records carry the 0x01 flag in retail (no exceptions
            // across the 97-entry corpus - if this flips a runtime
            // behaviour just changed).
            for (i, rec) in t.records.iter().enumerate() {
                assert_eq!(
                    rec.flag,
                    0x01,
                    "{}: record[{}].flag was {:#x}, expected 0x01",
                    path.display(),
                    i,
                    rec.flag
                );
            }

            if !t.scripts.is_empty() {
                with_scripts += 1;
                max_scripts = max_scripts.max(t.scripts.len());
            }
            if t.frame_opener_rate() >= 0.50 {
                frame_opener_50pct += 1;
            }
        }
    }

    eprintln!(
        "[v12] {} matches across {} PROT entries",
        hits,
        entries.len()
    );
    eprintln!("[v12] n range:     {min_n}..={max_n}");
    eprintln!(
        "[v12] param range: {min_param}..={max_param} ({} unique)",
        params.len()
    );
    eprintln!(
        "[v12] valid prescript at +0x800: {with_scripts} entries (max {max_scripts} scripts)"
    );
    eprintln!("[v12] frame-opener-rate >= 50%:  {frame_opener_50pct} entries");

    assert!(
        (95..=100).contains(&hits),
        "scene_v12 detector matched {hits} entries; expected ~97 per docs"
    );
    // Every match should have a valid prescript at offset 0x800.
    // Every retail v12 entry carries a parseable prescript at +0x800.
    assert_eq!(
        with_scripts, hits,
        "{with_scripts}/{hits} entries had a valid prescript at +0x800; expected all of them",
    );
    // Documented frame-opener rate: 75/97 ≥ 50%.
    assert!(
        (70..=85).contains(&frame_opener_50pct),
        "frame-opener-rate >= 50% in {frame_opener_50pct} entries; expected ~75",
    );

    // Observed floor: `0724_noaru.BIN` has N=22, param=0 (empty inline
    // records). All other entries hit N >= 26 (param >= 1).
    assert!(min_n >= 22, "min n {min_n} below observed floor");
    assert!(max_n <= 800, "max n {max_n} above observed ceiling");
    assert!(
        max_param <= 200,
        "max param {max_param} above observed ceiling"
    );
}
