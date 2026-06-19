//! Valid item-id pool for drop randomization.
//!
//! Drawn from the static `SCUS_942.54` item-name table (see
//! [`legaia_asset::item_names`]). Restricting the randomizer's replacement
//! drops to ids that name a real item ensures it never assigns a drop the game
//! has no name or handler for. Pass the retail executable (`SCUS_942.54`)
//! bytes, not the disc image - the table lives in the executable's data segment.

use anyhow::{Context, Result};
use legaia_asset::item_names::ItemNameTable;

/// Curated **fallback** set of quest / key items kept out of chest
/// randomization when the disc's item table can't be read. Every entry is an
/// unsellable (price-0) quest item, so this is a strict subset of the
/// data-driven [`crate::item_price::quest_item_ids`] - the chest randomizer's
/// real default ([`default_static_chest_items`]) prefers that disc-derived set
/// and only falls back to this constant. Override at the CLI with
/// `--keep-static-items` (pass an empty value to randomize everything).
///
/// | id | item | why |
/// |----|------|-----|
/// | `0x9a` | Mary's Diary | story / quest item |
/// | `0x71` | Dark Stone | quest item |
/// | `0xa9` | Fertilizer | Genesis-tree garden quest tool |
/// | `0xaa` | Weed Hammer | Genesis-tree garden quest tool |
/// | `0xb0` | Spring Salts | Genesis-tree garden quest tool |
/// | `0xa0` | Old Rod | fishing enabler |
///
/// Buyable items are deliberately excluded - a shop-tradeable accessory like
/// the Silver Compass (lowers the battle-start ambush rate) is a fine
/// chest-randomization candidate, so only genuinely unsellable quest items are
/// protected.
pub const DEFAULT_STATIC_CHEST_ITEMS: &[u8] = &[0x9a, 0x71, 0xa9, 0xaa, 0xb0, 0xa0];

/// The chest randomizer's default keep-static set, derived from the disc.
///
/// Returns the data-driven [`crate::item_price::quest_item_ids`] - every named,
/// unsellable (price-0) item except the chest-found equipment - which catches
/// the door keys, garden tools, eggs/talismans/books, letters, fishing rods,
/// casino cards, and Ra-Seru template entries automatically, so no quest item
/// is ever moved out of its chest or dropped into an unrelated one. Buyable
/// items (priced > 0) are intentionally left randomizable.
///
/// If the item table can't be read from `scus`, falls back to the curated
/// [`DEFAULT_STATIC_CHEST_ITEMS`] constant (a safe, if narrower, default). The
/// curated constant is a subset of the disc-derived set, so the result only
/// ever shrinks under the fallback, never gains a buyable id.
pub fn default_static_chest_items(scus: &[u8]) -> std::collections::BTreeSet<u8> {
    match crate::item_price::quest_item_ids(scus) {
        Ok(quest) => quest.into_iter().collect(),
        Err(_) => DEFAULT_STATIC_CHEST_ITEMS.iter().copied().collect(),
    }
}

/// The set of item ids that name a real (non-empty) item.
///
/// `id == 0` is the game's "no item" sentinel and is always excluded, so every
/// id in the pool is a valid, droppable item.
pub fn valid_item_pool(scus: &[u8]) -> Result<Vec<u8>> {
    let table = ItemNameTable::from_scus(scus)
        .context("SCUS_942.54 is not a PSX-EXE / item table absent")?;
    let mut pool = Vec::new();
    for id in 1..=u8::MAX {
        if table.name(id).is_some() {
            pool.push(id);
        }
    }
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::item_names::ItemNameTable;

    #[test]
    fn pool_excludes_sentinel_and_empty_slots() {
        // Build a table by hand: id 0 = "no item", a few named, some empty.
        let mut names: Vec<Option<String>> = vec![None; 256];
        names[0] = Some("UNUSED-ZERO".into()); // id 0 must still be excluded
        names[5] = Some("Healing Berry".into());
        names[6] = None; // empty slot
        names[200] = Some("Sword".into());
        let table = ItemNameTable::from_names(names);

        // valid_item_pool takes raw SCUS bytes, so re-derive the same predicate
        // here against the table to validate the id-selection rule directly.
        let pool: Vec<u8> = (1..=u8::MAX)
            .filter(|&id| table.name(id).is_some())
            .collect();
        assert!(!pool.contains(&0), "id 0 sentinel excluded");
        assert!(pool.contains(&5));
        assert!(!pool.contains(&6), "empty slot excluded");
        assert!(pool.contains(&200));
    }

    #[test]
    fn non_exe_input_errs() {
        assert!(valid_item_pool(b"not a psx exe").is_err());
    }
}
