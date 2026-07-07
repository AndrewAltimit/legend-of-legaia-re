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

/// Position law + `.LZS` sibling: every detected `scene_v12_table` (the
/// per-scene `DATA\FIELD\<scene>.PCH` walk-on trigger sidecar) sits at raw
/// TOC index `define + 1` (= extraction `define - 1`), and the entry at raw
/// `define + 3` (= extraction `define + 1`) is the scene's `.LZS`
/// `scene_asset_table` bundle - the entry the transition streamer
/// `FUN_80021934` stages at `_DAT_8007B85C` (state 2 streams
/// `DAT_8007B768 + 3` by index; state 4 the same file by name).
/// See `docs/formats/scene-v12-table.md` and
/// `docs/subsystems/asset-loader.md`.
#[test]
fn scene_v12_position_law_and_lzs_sibling() {
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let cdname_path = prot.parent().unwrap().join("CDNAME.TXT");
    let Ok(defines) = legaia_prot::cdname::parse(&cdname_path) else {
        eprintln!("[skip] CDNAME.TXT missing next to extracted/PROT/");
        return;
    };

    // Extraction index -> file path.
    let mut by_idx: BTreeMap<u32, PathBuf> = BTreeMap::new();
    for entry in std::fs::read_dir(&prot).unwrap().flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !name.ends_with(".BIN") || name.len() < 4 {
            continue;
        }
        if let Ok(idx) = name[..4].parse::<u32>() {
            by_idx.insert(idx, path);
        }
    }

    // 1. Every v12 in the corpus sits at raw `define + 1`, i.e. its
    //    extraction index `p` satisfies `p + 1 in defines` (raw = p + 2).
    let mut v12_at = Vec::new();
    for (&idx, path) in &by_idx {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if detect(&bytes).is_some() {
            assert!(
                defines.contains_key(&(idx + 1)),
                "v12 at extraction {idx} is not at raw define+1 (no define {})",
                idx + 1
            );
            v12_at.push(idx);
        }
    }
    eprintln!(
        "[v12-law] {} v12 entries, all at raw define+1",
        v12_at.len()
    );
    assert!((95..=100).contains(&v12_at.len()));

    // 2. Kingdom scenes + town01: define-1 is the v12, define+1 is a
    //    scene_asset_table head with legal dispatcher types (the raw
    //    base+3 `.LZS` slot the transition streamer loads).
    const LEGAL_TYPES: [u8; 14] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0xA, 0xB, 0xF, 0x14];
    for scene in ["town01", "map01", "map02", "map03"] {
        let define = *defines
            .iter()
            .find(|(_, v)| v.as_str() == scene)
            .map(|(k, _)| k)
            .unwrap_or_else(|| panic!("{scene} missing from CDNAME"));
        let v12_bytes = std::fs::read(&by_idx[&(define - 1)]).unwrap();
        let table = detect(&v12_bytes)
            .unwrap_or_else(|| panic!("{scene}: extraction define-1 is not a v12"));
        let bundle = std::fs::read(&by_idx[&(define + 1)]).unwrap();
        let count = u32::from_le_bytes(bundle[0..4].try_into().unwrap());
        assert!(
            (1..=16).contains(&count),
            "{scene}: base+3 count word {count} not a descriptor table"
        );
        let mut types = Vec::new();
        for i in 0..count as usize {
            let ts = u32::from_le_bytes(bundle[4 + 8 * i + 4..4 + 8 * i + 8].try_into().unwrap());
            let ty = (ts >> 24) as u8;
            assert!(
                LEGAL_TYPES.contains(&ty),
                "{scene}: base+3 descriptor[{i}] type {ty:#x} not a dispatcher type"
            );
            types.push(ty);
        }
        assert!(
            types.contains(&3),
            "{scene}: base+3 bundle carries no type-3 (MAN) slot"
        );
        eprintln!(
            "[v12-law] {scene}: v12 param={} at ext {}, base+3 bundle count={count} types={types:?}",
            table.param,
            define - 1
        );
    }

    // 3. Content anchors pinning the scene attribution (not the naive
    //    filename labels): town01's v12 carries the opening walk-on
    //    trigger record (tile 0x1D, 0x5B -> P2[3]); map01's carries a
    //    record spawning P2[38] (0x26), the world-map fly-in.
    let town01 = *defines
        .iter()
        .find(|(_, v)| v.as_str() == "town01")
        .unwrap()
        .0;
    let t = detect(&std::fs::read(&by_idx[&(town01 - 1)]).unwrap()).unwrap();
    assert!(
        t.records
            .iter()
            .any(|r| (r.b0, r.b1, r.b2) == (0x1D, 0x5B, 0x03)),
        "town01 v12 lacks the (0x1D, 0x5B, P2[3]) opening trigger"
    );
    let map01 = *defines
        .iter()
        .find(|(_, v)| v.as_str() == "map01")
        .unwrap()
        .0;
    let m = detect(&std::fs::read(&by_idx[&(map01 - 1)]).unwrap()).unwrap();
    assert!(
        m.records.iter().any(|r| r.b2 == 0x26),
        "map01 v12 lacks a P2[38] (fly-in) trigger record"
    );
}
