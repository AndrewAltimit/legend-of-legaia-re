//! Smoke tests that exercise the cheat database files shipped under
//! `data/cheats/`. These run on every CI build; no Sony bytes are
//! needed.

use legaia_cheats::{Category, classify_address, parse_gs_text, parse_mednafen_cht};
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p
}

#[test]
fn gs_text_corpus_parses_cleanly() {
    let path = workspace_root().join("data/cheats/legaia-ntsc-u.gs.txt");
    let text = std::fs::read_to_string(&path).expect("missing legaia-ntsc-u.gs.txt");
    let db = parse_gs_text(&text).expect("parse");
    assert!(
        db.entries.len() >= 200,
        "expected at least 200 entries, got {}",
        db.entries.len()
    );
    // The dedupe path is exercised in the unit tests; here we just
    // confirm the corpus parses without errors.
}

#[test]
fn mednafen_cht_corpus_parses_cleanly() {
    let path = workspace_root().join("data/cheats/legaia-ntsc-u.cht");
    let text = std::fs::read_to_string(&path).expect("missing legaia-ntsc-u.cht");
    let db = parse_mednafen_cht(&text).expect("parse");
    assert!(db.entries.len() >= 40);
    // Sanity: every entry has at least one write.
    for e in &db.entries {
        assert!(
            e.codes.iter().any(|c| c.is_write()),
            "entry `{}` has no writes",
            e.description
        );
    }
}

#[test]
fn classifier_covers_most_corpus_addresses() {
    let path = workspace_root().join("data/cheats/legaia-ntsc-u.cht");
    let text = std::fs::read_to_string(&path).expect("missing legaia-ntsc-u.cht");
    let db = parse_mednafen_cht(&text).expect("parse");
    let mut total = 0usize;
    let mut unknown = 0usize;
    for e in &db.entries {
        for c in e.writes() {
            total += 1;
            if classify_address(c.addr).category == Category::Unknown {
                unknown += 1;
            }
        }
    }
    // We expect at least 80% of the writes to land in a named category.
    let known_pct = (total - unknown) * 100 / total.max(1);
    assert!(
        known_pct >= 80,
        "only {known_pct}% of {total} writes classified ({unknown} unknown)"
    );
}

#[test]
fn both_formats_describe_same_per_character_offsets() {
    let gs_text =
        std::fs::read_to_string(workspace_root().join("data/cheats/legaia-ntsc-u.gs.txt")).unwrap();
    let cht_text =
        std::fs::read_to_string(workspace_root().join("data/cheats/legaia-ntsc-u.cht")).unwrap();
    let mut gs = parse_gs_text(&gs_text).unwrap();
    gs.dedupe_identical();
    let cht = parse_mednafen_cht(&cht_text).unwrap();

    fn offsets(db: &legaia_cheats::Database) -> std::collections::BTreeSet<u32> {
        let mut out = std::collections::BTreeSet::new();
        for e in &db.entries {
            for c in e.writes() {
                if let Some((base, _)) = legaia_cheats::CHAR_RECORD_BASES
                    .iter()
                    .find(|(b, _)| c.addr >= *b && c.addr < *b + 0x414)
                {
                    out.insert(c.addr - base);
                }
            }
        }
        out
    }

    let gs_offsets = offsets(&gs);
    let cht_offsets = offsets(&cht);

    // The Mednafen file is hand-curated and may be a strict subset
    // of the GameShark dump (which has dupes for every character).
    // We assert the cht offsets are a SUBSET of the gs offsets, so
    // both views agree on what's true.
    let missing_in_gs: Vec<_> = cht_offsets
        .iter()
        .filter(|o| !gs_offsets.contains(o))
        .collect();
    assert!(
        missing_in_gs.is_empty(),
        "cht has offsets the gs dump doesn't: {missing_in_gs:?}"
    );
}
