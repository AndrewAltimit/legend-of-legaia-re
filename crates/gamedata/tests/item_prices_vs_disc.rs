//! Disc-gated cross-validation of curated shop prices against the static
//! `SCUS_942.54` item-property table.
//!
//! Each item's 12-byte property record (`DAT_80074368`, the table
//! `legaia_asset::item_names` reads name pointers from at `+4`) carries the
//! shop price as the `u16` at `+2` — the field the in-game buy/sell UI reads
//! (verified there against a live shop). It is the authoritative price, so it
//! makes a clean oracle for the curated `gamedata` prices mined from
//! walkthroughs.
//!
//! Across weapons + armor + accessories the disc agrees with the curated tables
//! on every priced item that name-matches (119+), and this oracle pinned three
//! walkthrough errors that were then corrected to the disc values (Forest /
//! Magic Amulet 4000→2000, Evil Medallion 9998→9999). It now asserts **zero**
//! mismatches so a future curated edit that diverges from the disc is caught.
//!
//! Skips silently when `extracted/SCUS_942.54` is missing.

use std::collections::HashMap;
use std::path::PathBuf;

use legaia_asset::item_names::{self, ItemNameTable};
use legaia_gamedata::Database;

fn scus_bytes() -> Option<Vec<u8>> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    std::fs::read(workspace.join("extracted").join("SCUS_942.54")).ok()
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// `name -> disc shop price` over all 256 item ids (price `0` = quest /
/// not-priced; excluded).
fn disc_prices(scus: &[u8]) -> HashMap<String, u16> {
    let names = ItemNameTable::from_scus(scus).expect("parse item-name table");
    let mut out = HashMap::new();
    for id in 0u8..=u8::MAX {
        let Some(name) = names.name(id) else { continue };
        if name.is_empty() {
            continue;
        }
        if let Some(price) = item_names::item_price(scus, id)
            && price != 0
        {
            out.insert(norm(name), price);
        }
    }
    out
}

#[test]
fn curated_shop_prices_match_the_disc() {
    let Some(scus) = scus_bytes() else {
        eprintln!("[skip] extracted/SCUS_942.54 missing");
        return;
    };
    let disc = disc_prices(&scus);
    let db = Database::load();

    // (label, name, curated price) over every priced equippable.
    let mut items: Vec<(&str, String, u32)> = Vec::new();
    for w in db.weapons() {
        if let Some(p) = w.price {
            items.push(("weapon", w.name.clone(), p));
        }
    }
    for a in db.armor() {
        if let Some(p) = a.price {
            items.push(("armor", a.name.clone(), p));
        }
    }
    for a in db.accessories() {
        if let Some(p) = a.price {
            items.push(("accessory", a.name.clone(), p));
        }
    }

    let mut matched = 0usize;
    let mut mismatches: Vec<String> = Vec::new();
    for (label, name, gp) in &items {
        // Name-misses are tolerated (curated names that don't map to a disc id,
        // e.g. Ra-Seru weapon variants the disc enumerates per-tier); only a
        // present-but-different price is a failure.
        let Some(&dp) = disc.get(&norm(name)) else {
            continue;
        };
        if dp as u32 == *gp {
            matched += 1;
        } else {
            mismatches.push(format!("[{label}] {name}: disc={dp} curated={gp}"));
        }
    }

    assert!(
        mismatches.is_empty(),
        "curated prices disagree with the disc (disc is authoritative): {mismatches:#?}"
    );
    // Non-vacuous: the oracle must actually be comparing a healthy set.
    assert!(
        matched >= 119,
        "expected 119+ price cross-checks, only matched {matched}"
    );
}
