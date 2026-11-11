//! Disc-gated cross-validation of curated shop prices against the static
//! `SCUS_942.54` item-property table.
//!
//! Each item's 12-byte property record (`DAT_80074368`, the table
//! `legaia_game_tables::item_names` reads name pointers from at `+4`) carries the
//! shop price as the `u16` at `+2` - the field the in-game buy/sell UI reads
//! (verified there against a live shop). It is the authoritative price, so it
//! makes a clean oracle for the curated `gamedata` prices mined from
//! walkthroughs.
//!
//! Every curated item, weapon, armour piece and accessory must name-join a
//! disc item id (the one exception is [`NO_DISC_ITEM`], a walkthrough entry
//! with no item of its own), and its `price` must equal the disc price, with
//! a missing curated price meaning a disc price of `0`. Consumables also pin
//! the restore amount their `effect` text names against the item-effect
//! table's resolved amount (`legaia_game_tables::item_effect`). This oracle
//! pinned walkthrough errors that were corrected to the disc values (Forest /
//! Magic Amulet 4000→2000, Evil Medallion 9998→9999, Healing Flower 800→400,
//! Honey's unlisted 10000, the "TimeSpace Bomb" and "Light Egg" spellings).
//!
//! Skips silently when `extracted/SCUS_942.54` is missing.

use std::collections::HashMap;
use std::path::PathBuf;

use legaia_game_tables::item_effect::{ItemEffectTable, RestoreAmount};
use legaia_game_tables::item_names::{self, ItemNameTable};
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

/// Curated entries with no disc item of their own: the walkthrough's
/// "Soren Book of Arts" names no row of the SCUS item-name table.
const NO_DISC_ITEM: &[&str] = &["Soren Book of Arts"];

/// `name -> (disc item id, disc shop price)` over all 256 item ids (the first
/// id wins for a repeated name).
fn disc_items(scus: &[u8]) -> HashMap<String, (u8, u16)> {
    let names = ItemNameTable::from_scus(scus).expect("parse item-name table");
    let mut out = HashMap::new();
    for id in 0u8..=u8::MAX {
        let Some(name) = names.name(id) else { continue };
        if name.is_empty() {
            continue;
        }
        let price = item_names::item_price(scus, id).unwrap_or(0);
        out.entry(norm(name)).or_insert((id, price));
    }
    out
}

#[test]
fn curated_prices_match_the_disc() {
    let Some(scus) = scus_bytes() else {
        eprintln!("[skip] extracted/SCUS_942.54 missing");
        return;
    };
    let disc = disc_items(&scus);
    let db = Database::load();

    // (label, name, curated price) over every curated item row.
    let mut rows: Vec<(&str, &str, Option<u32>)> = Vec::new();
    rows.extend(
        db.items()
            .iter()
            .map(|i| ("item", i.name.as_str(), i.price)),
    );
    rows.extend(
        db.weapons()
            .iter()
            .map(|w| ("weapon", w.name.as_str(), w.price)),
    );
    rows.extend(
        db.armor()
            .iter()
            .map(|a| ("armor", a.name.as_str(), a.price)),
    );
    rows.extend(
        db.accessories()
            .iter()
            .map(|a| ("accessory", a.name.as_str(), a.price)),
    );

    let mut matched = 0usize;
    let mut fail: Vec<String> = Vec::new();
    for (label, name, curated) in &rows {
        let Some(&(_, disc_price)) = disc.get(&norm(name)) else {
            if !NO_DISC_ITEM.contains(name) {
                fail.push(format!("[{label}] {name}: no disc item of that name"));
            }
            continue;
        };
        assert!(
            !NO_DISC_ITEM.contains(name),
            "{name} now joins a disc item; drop it from NO_DISC_ITEM"
        );
        matched += 1;
        if curated.unwrap_or(0) != u32::from(disc_price) {
            fail.push(format!(
                "[{label}] {name}: curated={curated:?} disc={disc_price}"
            ));
        }
    }
    assert!(
        fail.is_empty(),
        "curated items disagree with the disc (disc is authoritative): {fail:#?}"
    );
    assert_eq!(matched, rows.len() - NO_DISC_ITEM.len(), "every row joined");
}

#[test]
fn curated_restore_amounts_match_the_disc() {
    let Some(scus) = scus_bytes() else {
        eprintln!("[skip] extracted/SCUS_942.54 missing");
        return;
    };
    let disc = disc_items(&scus);
    let effects = ItemEffectTable::from_scus(&scus).expect("parse item-effect table");
    let db = Database::load();
    let mut checked = 0usize;
    for it in db.items().iter().filter(|i| i.category == "consumable") {
        let Some(&(id, _)) = disc.get(&norm(&it.name)) else {
            continue;
        };
        let (amount, stat) = match effects.restore_amount(id) {
            Some(RestoreAmount::Hp(9999)) => {
                assert!(
                    it.effect.contains("maximum HP"),
                    "{}: disc is a full HP restore, curated {:?}",
                    it.name,
                    it.effect
                );
                checked += 1;
                continue;
            }
            Some(RestoreAmount::Hp(n)) => (n, "HP"),
            Some(RestoreAmount::Mp(n)) => (n, "MP"),
            _ => continue,
        };
        let want = format!("Restores {amount} {stat}");
        assert!(
            it.effect.starts_with(&want),
            "{}: disc restores {amount} {stat}, curated {:?}",
            it.name,
            it.effect
        );
        checked += 1;
    }
    assert!(checked >= 8, "only {checked} restore amounts checked");
}
