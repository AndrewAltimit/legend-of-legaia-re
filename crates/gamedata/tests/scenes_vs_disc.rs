//! Disc-gated cross-validation of `scenes.toml`'s `cdname` column against the
//! disc's own `CDNAME.TXT`: every row's `#define` number is the one the file
//! gives its label, and every label the file defines has a row (two labels
//! the file puts on one block, `music_test` and `music_01`, share the
//! `music_01` row). The banners are pinned separately, by
//! `crates/patcher/tests/place_names_real.rs`.
//!
//! Skips silently when `extracted/CDNAME.TXT` is missing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_gamedata::Database;

#[test]
fn curated_scene_numbers_match_cdname_txt() {
    let ws = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Ok(text) = std::fs::read(ws.join("extracted/CDNAME.TXT")) else {
        eprintln!("[skip] extracted/CDNAME.TXT missing");
        return;
    };
    let text = String::from_utf8_lossy(&text);
    let mut defines: BTreeMap<String, u32> = BTreeMap::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        if it.next() != Some("#define") {
            continue;
        }
        if let (Some(name), Some(n)) = (it.next(), it.next().and_then(|n| n.parse().ok())) {
            defines.entry(name.to_string()).or_insert(n);
        }
    }
    let db = Database::load();
    for s in db.scene_names() {
        assert_eq!(
            defines.get(&s.id),
            Some(&s.cdname),
            "{}: curated #define vs CDNAME.TXT",
            s.id
        );
    }
    for (name, n) in &defines {
        assert!(
            db.scene_names().iter().any(|s| s.cdname == *n),
            "CDNAME.TXT block {name} ({n}) has no scenes.toml row"
        );
    }
}
