//! Disc-gated cross-validation of `characters.toml` against the battle
//! overlay's element tables (PROT 0898, `legaia_game_tables::element_affinity`):
//! each character's `ra_seru_element` is its entry in the per-character
//! element table (`0x801F5480`), and its `affinity_strong` / `affinity_weak`
//! lists are the elements whose cell in the affinity matrix (`0x801F53E8`,
//! `matrix[attacker][defender]`) down that element's column is below / above
//! 100.
//!
//! Skips silently when `extracted/PROT/0898_xxx_dat.BIN` is missing.

use std::path::PathBuf;

use legaia_game_tables::element_affinity::{self, Element};
use legaia_gamedata::Database;

#[test]
fn curated_character_affinities_match_the_disc() {
    let ws = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Ok(prot) = std::fs::read(ws.join("extracted/PROT/0898_xxx_dat.BIN")) else {
        eprintln!("[skip] extracted/PROT/0898_xxx_dat.BIN missing");
        return;
    };
    let table = element_affinity::parse(&prot).expect("affinity tables");
    let db = Database::load();
    assert_eq!(db.characters().len(), 3);
    for (i, c) in db.characters().iter().enumerate() {
        let own = table
            .character_element(i as u8 + 1)
            .expect("character element");
        let name = |e: u8| Element::from_id(e).expect("element id").name().to_string();
        assert_eq!(c.ra_seru_element, name(own), "{} element", c.name);
        let column = |pred: fn(u8) -> bool| -> Vec<String> {
            let mut v: Vec<String> = (0..element_affinity::ELEMENT_COUNT as u8)
                .filter(|&a| pred(table.affinity_pct(a, own).unwrap()))
                .map(name)
                .collect();
            v.sort();
            v
        };
        let sorted = |v: &[String]| {
            let mut v = v.to_vec();
            v.sort();
            v
        };
        assert_eq!(
            sorted(&c.affinity_strong),
            column(|p| p < 100),
            "{} strong",
            c.name
        );
        assert_eq!(
            sorted(&c.affinity_weak),
            column(|p| p > 100),
            "{} weak",
            c.name
        );
    }
}
