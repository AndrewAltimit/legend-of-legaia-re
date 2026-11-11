//! Disc-gated cross-validation of `music.toml`'s sound-test columns against
//! the music test's own row table in PROT 0974: 81 records of `0x84` bytes
//! from file offset `0x728`, each a Shift-JIS string `` `<id> <title> ``
//! (a leading backtick, the id, full-width padding, the working title).
//! The curated `index` is the record number, and the record's text is the
//! curated `id` followed by its `debug_title_jp`, compared with full-width
//! forms folded to ASCII and whitespace dropped.
//!
//! Skips silently when `extracted/PROT/0974_other_game.BIN` is missing.

use std::path::PathBuf;

use legaia_gamedata::Database;

const TABLE_OFFSET: usize = 0x728;
const RECORD_LEN: usize = 0x84;
const RECORDS: usize = 81;

/// Fold full-width ASCII forms (`U+FF01..=U+FF5E`) to ASCII and drop every
/// space, ideographic space and tab.
fn fold(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            ' ' | '\t' | '\u{3000}' => None,
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0),
            c => Some(c),
        })
        .collect()
}

#[test]
fn curated_sound_test_rows_match_the_music_test_table() {
    let ws = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Ok(entry) = std::fs::read(ws.join("extracted/PROT/0974_other_game.BIN")) else {
        eprintln!("[skip] extracted/PROT/0974_other_game.BIN missing");
        return;
    };
    let db = Database::load();
    assert_eq!(db.music_tracks().len(), RECORDS);
    let mut fail: Vec<String> = Vec::new();
    for t in db.music_tracks() {
        let o = TABLE_OFFSET + t.index as usize * RECORD_LEN;
        let raw = &entry[o..o + RECORD_LEN];
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        let (text, _, bad) = encoding_rs::SHIFT_JIS.decode(&raw[..end]);
        assert!(!bad, "record {} is not Shift-JIS", t.index);
        let disc = fold(
            text.strip_prefix('`')
                .expect("record opens with a backtick"),
        );
        let curated = fold(&format!(
            "{}{}",
            t.id.as_deref().unwrap_or(""),
            t.debug_title_jp.as_deref().unwrap_or("")
        ));
        if disc != curated {
            fail.push(format!(
                "{} {:?}: curated {curated:?} disc {disc:?}",
                t.index, t.id
            ));
        }
    }
    assert!(
        fail.is_empty(),
        "curated sound-test rows disagree with the disc (disc is authoritative): {fail:#?}"
    );
}
