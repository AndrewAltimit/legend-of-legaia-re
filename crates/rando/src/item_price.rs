//! Item shop-price edits + the shop-sellable item pool.
//!
//! A town shop's price for an item is the `u16` at `+2` of that item's record
//! in the `SCUS_942.54` item table (`legaia_asset::item_names::price_slot`).
//! Quest / key / story / found-only items have price **0**, which is what makes
//! `> 0` a clean "this item is something a shop sells" predicate — the shop
//! randomizer draws its `Random` fill from that [`sellable_pool`], so no quest
//! item can ever be put up for sale.
//!
//! A handful of genuinely-equippable items are normally **only found in chests**
//! and so ship with price `0` — they'd appear free if a randomized shop stocked
//! them. [`CHEST_EQUIPMENT_PRICES`] gives each a shop value (approximated from
//! the nearest priced gear of the same type), and [`price_patches`] emits the
//! same-size `SCUS_942.54` edits to install them. Once priced they are both
//! non-free and part of [`sellable_pool`].

use anyhow::{Context, Result};
use legaia_asset::item_names;
use legaia_asset::item_names::ItemNameTable;
use std::collections::BTreeSet;

/// Chest-found equipment that ships with shop price `0`, paired with the value
/// to give it (gold). Approximated from the nearest priced item of the same
/// type: the Ra-Seru weapons + Astral Sword sit at the top weapon tier
/// (Great Axe, ~55000); the Ra-Seru armor/head/shoes at the top armor tier
/// (War God Plate / Triumph Boots / Battle Robe, ~28800–35000). These are the
/// only `0`-price items that are real gear rather than quest/key items.
pub const CHEST_EQUIPMENT_PRICES: &[(u8, u16)] = &[
    // weapons (atk 96–100) — Great Axe tier
    (0x1b, 55000), // Ra-Seru Blade
    (0x1f, 55000), // Ra-Seru Fangs
    (0x21, 55000), // Ra-Seru Club
    (0xba, 55000), // Astral Sword
    // head gear
    (0x38, 31000), // Ra-Seru Seal
    (0x3e, 35000), // Ra-Seru Plume
    (0x42, 31000), // Ra-Seru Helmet
    // body armor / shoes — War God Plate tier
    (0x4b, 28800), // Ra-Seru Armor
    (0x51, 28800), // Ra-Seru Robe
    (0x57, 28800), // Ra-Seru Plate
    (0x5f, 28800), // Ra-Seru Boots
    (0x64, 28800), // Ra-Seru Shoes
    (0x69, 28800), // Ra-Seru Thongs
];

/// Same-size `SCUS_942.54` price edits for [`CHEST_EQUIPMENT_PRICES`]: each is
/// `(file_offset, u16_le_bytes)` for the record's `+2` price field, emitted only
/// where the current value differs from the target (so re-applying is a no-op).
/// Errors only if `scus` isn't a PSX-EXE / the table is out of range.
pub fn price_patches(scus: &[u8]) -> Result<Vec<(usize, [u8; 2])>> {
    let mut out = Vec::new();
    for &(id, price) in CHEST_EQUIPMENT_PRICES {
        let (off, cur) = item_names::price_slot(scus, id)
            .with_context(|| format!("price slot for item 0x{id:02x} out of range"))?;
        if cur != price {
            out.push((off, price.to_le_bytes()));
        }
    }
    Ok(out)
}

/// The shop-sellable item pool: every id whose item-table price is `> 0`. This
/// is exactly the set of items the game prices for sale — consumables, buyable
/// weapons/armor/accessories, and (once [`price_patches`] is applied) the
/// chest-found equipment — and it excludes every quest / key / story item
/// (which all have price `0`). Pass the `SCUS_942.54` bytes *after* applying the
/// price patches so the priced equipment is included.
pub fn sellable_pool(scus: &[u8]) -> Result<Vec<u8>> {
    // Require a parseable item table (id 1's price slot resolves).
    item_names::price_slot(scus, 1).context("SCUS_942.54 item table absent")?;
    let mut pool = Vec::new();
    for id in 1..=u8::MAX {
        if item_names::item_price(scus, id).is_some_and(|p| p > 0) {
            pool.push(id);
        }
    }
    Ok(pool)
}

/// The quest / key / story item ids on the disc: every **named** item the
/// item-table prices as unsellable (price `0`), minus the handful of genuinely
/// equippable "chest-found" gear pieces ([`CHEST_EQUIPMENT_PRICES`]) that ship
/// price-0 only because they're never sold.
///
/// Price-0 is the game's own marker for "this is not a thing a shop trades"
/// (see [`sellable_pool`]); for items it means a key/story/tool item — the door
/// keys, the garden-quest tools, the egg/talisman/book collectibles, the
/// letters and diaries, the fishing rods, the casino cards, and the internal
/// Ra-Seru weapon-state template entries. None of these belong in the chest
/// randomizer's pool: moving one out of its scripted chest can soft-lock
/// progression, and dropping one into an unrelated chest is nonsensical. The
/// chest randomizer treats this set as static (kept in place, dropped from the
/// random-fill pool), so a quest item never moves and never spawns elsewhere.
///
/// The chest-found **equipment** is excluded so it stays randomizable — it is
/// real gear, not a quest item. Buyable tools (priced > 0, e.g. the Silver
/// Compass a shop sells) are likewise not protected: only genuinely unsellable
/// quest items are. This is exactly the set
/// [`crate::items::default_static_chest_items`] uses by default.
///
/// Errors only if `scus` isn't a PSX-EXE / the item table is out of range.
pub fn quest_item_ids(scus: &[u8]) -> Result<Vec<u8>> {
    let table = ItemNameTable::from_scus(scus)
        .context("SCUS_942.54 is not a PSX-EXE / item table absent")?;
    let exceptions: BTreeSet<u8> = CHEST_EQUIPMENT_PRICES.iter().map(|&(id, _)| id).collect();
    let mut out = Vec::new();
    for id in 1..=u8::MAX {
        if exceptions.contains(&id) {
            continue;
        }
        // A real item (named slot) the table prices as unsellable.
        if table.name(id).is_some() && item_names::item_price(scus, id).is_some_and(|p| p == 0) {
            out.push(id);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_cover_thirteen_distinct_chest_equipment_ids() {
        let mut ids: Vec<u8> = CHEST_EQUIPMENT_PRICES.iter().map(|&(id, _)| id).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "no duplicate ids");
        assert_eq!(n, 13, "the 13 reviewed chest-equipment items");
        for &(_, p) in CHEST_EQUIPMENT_PRICES {
            assert!(p > 0, "price is a non-zero u16");
        }
    }
}
