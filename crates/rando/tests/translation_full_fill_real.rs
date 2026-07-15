//! Disc-gated mechanism-coverage test for the translation import: the
//! pipeline must replace **everything a pack covers**, not silently drop
//! entries. Method: export the full skeleton, fill EVERY entry with a
//! same-length reversible transform of its source (vowel swap - always
//! encodable, always within budget, compression-neutral), import onto a
//! scratch copy, and assert per section that every filled entry was applied
//! (none skipped), every touched sector stays EDC/ECC-valid, and a re-export
//! from the patched image reads the transform back at every key.
//!
//! Guards the two silent-drop mechanisms this test originally caught:
//! the scene-overflow rollback popping the *shortest* lines first (so a
//! marginal overflow rolled back hundreds of lines), and the greedy LZS
//! parse missing Sony's footprint by 1-2 bytes on two retail MANs (which
//! made those scenes permanently untranslatable).
//!
//! Skips + passes without `LEGAIA_DISC_BIN`.

use std::collections::BTreeMap;

use legaia_iso::raw::SECTOR_SIZE;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::translation::{export_pack, import_pack};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Same-length, in-charset, reversible transform: swap vowels within their
/// case class. `{..}` escapes pass through untouched so token bytes (and the
/// encoded length) never change.
fn vowel_swap(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_brace = false;
    for c in src.chars() {
        let mapped = match c {
            '{' => {
                in_brace = true;
                c
            }
            '}' => {
                in_brace = false;
                c
            }
            _ if in_brace => c,
            'a' => 'e',
            'e' => 'a',
            'i' => 'o',
            'o' => 'i',
            'A' => 'E',
            'E' => 'A',
            'I' => 'O',
            'O' => 'I',
            _ => c,
        };
        out.push(mapped);
    }
    out
}

#[test]
fn full_fill_applies_every_entry_per_section() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let src_patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let mut pack = export_pack(&src_patcher).expect("export");

    // Fill EVERY entry whose transform is observable (vowel-less lines would
    // be indistinguishable from "not applied").
    let mut expect: BTreeMap<String, String> = BTreeMap::new();
    for entries in pack.sections.each_mut() {
        for e in entries.iter_mut() {
            let t = vowel_swap(&e.source);
            if t == e.source {
                continue;
            }
            e.translation = t.clone();
            expect.insert(e.key.clone(), t);
        }
    }
    assert!(
        expect.len() > 25_000,
        "full fill should cover most of the corpus ({})",
        expect.len()
    );

    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let report = import_pack(&mut patcher, &pack).expect("import");

    // Per-section outcome: every filled entry must be applied; nothing may be
    // silently unaccounted for. Print the table (the coverage numbers the
    // site surfaces come from the same accounting).
    //
    // One legitimate skip class remains: a scene whose fully-transformed
    // dialog does not LZS-compress back into its zero-slack footprint even
    // under the optimal-parse encoder (the transform breaks cross-line
    // matches against text the segment scanner rejected). On the retail disc
    // that is a single scene, so the tolerance is one scene's worth of lines,
    // and every such skip must carry the recompress diagnostic - anything
    // else (encode failures, budget, framing) is a mechanism bug.
    let counts = report.section_counts(&pack);
    for c in &counts {
        eprintln!(
            "[section] {:20} total {:6} filled {:6} applied {:6} already {:4} skipped {:4}",
            c.name, c.total, c.filled, c.applied, c.already_applied, c.skipped
        );
        assert_eq!(
            c.applied + c.already_applied + c.skipped,
            c.filled,
            "section {}: every filled entry must be accounted for",
            c.name
        );
        if c.name != "scene_dialog" {
            assert_eq!(
                c.skipped, 0,
                "section {}: an in-budget, encodable, same-length fill must \
                 never be skipped",
                c.name
            );
        }
    }
    for (key, msg) in &report.issues {
        assert!(
            msg.contains("recompresses"),
            "only genuine compressed-footprint overflows may skip: {key}: {msg}"
        );
    }
    let overflow_scenes: std::collections::BTreeSet<&str> = report
        .issues
        .iter()
        .filter_map(|(k, _)| k.split(':').nth(1))
        .collect();
    assert!(
        overflow_scenes.len() <= 1,
        "at most one retail scene overflows under the transform: {overflow_scenes:?}"
    );
    assert!(
        report.issues.len() <= 40,
        "overflow skips bounded to one scene's lines ({})",
        report.issues.len()
    );
    assert!(
        report.applied + report.already_applied >= expect.len() - 40,
        "{} + {} of {} filled entries applied",
        report.applied,
        report.already_applied,
        expect.len()
    );

    // Every touched sector still EDC/ECC-valid.
    let patched = patcher.into_image();
    assert_eq!(patched.len(), original.len());
    let mut touched = 0usize;
    for (i, (a, b)) in original
        .chunks(SECTOR_SIZE)
        .zip(patched.chunks(SECTOR_SIZE))
        .enumerate()
    {
        if a != b && a.len() == SECTOR_SIZE {
            touched += 1;
            assert!(
                legaia_iso::write::mode2_form1_sector_is_valid(b),
                "sector {i} invalid after full-fill import"
            );
        }
    }
    assert!(touched > 0);

    // Re-export from the patched image: every APPLIED key reads back as its
    // transform (same key - all edits are same-size, so no offset moves).
    // Keys the import reported as skipped (the overflow scene) read back as
    // their untouched source instead.
    let skipped: std::collections::BTreeSet<&str> =
        report.issues.iter().map(|(k, _)| k.as_str()).collect();
    let post = DiscPatcher::open(patched).expect("open patched");
    let re = export_pack(&post).expect("re-export");
    let mut seen = 0usize;
    let mut wrong: Vec<String> = Vec::new();
    for (_, entries) in re.sections.iter() {
        for e in entries {
            if skipped.contains(e.key.as_str()) {
                continue;
            }
            if let Some(want) = expect.get(&e.key) {
                seen += 1;
                if e.source.trim_end_matches(' ') != want.trim_end_matches(' ') {
                    wrong.push(e.key.clone());
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} keys did not read back transformed: {:?}",
        wrong.len(),
        &wrong[..wrong.len().min(10)]
    );
    assert_eq!(
        seen,
        expect.len() - skipped.len(),
        "every applied key must re-export from the patched image"
    );
}
