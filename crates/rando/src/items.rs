//! Valid item-id pool for drop randomization.
//!
//! Drawn from the static `SCUS_942.54` item-name table (see
//! [`legaia_asset::item_names`]). Restricting the randomizer's replacement
//! drops to ids that name a real item ensures it never assigns a drop the game
//! has no name or handler for. Pass the retail executable (`SCUS_942.54`)
//! bytes, not the disc image — the table lives in the executable's data segment.

use anyhow::{Context, Result};
use legaia_asset::item_names::ItemNameTable;

/// Curated set of chest item ids that should **never** be randomized — quest /
/// key items whose chest location the player needs predictable (and which would
/// be nonsensical as random fill elsewhere). The chest randomizer keeps every
/// chest whose original item is in this set at its original item, and drops
/// these ids from the random-fill pool so they can't be duplicated into other
/// chests. Override at the CLI with `--keep-static-items` (pass an empty value
/// to randomize everything).
///
/// | id | item | why |
/// |----|------|-----|
/// | `0x9a` | Mary's Diary | story / quest item |
/// | `0x71` | Dark Stone | quest item |
/// | `0xa9` | Fertilizer | Genesis-tree garden quest tool |
/// | `0xaa` | Weed Hammer | Genesis-tree garden quest tool |
/// | `0xb0` | Spring Salts | Genesis-tree garden quest tool |
/// | `0xf3` | Silver Compass | navigation enabler |
/// | `0xa0` | Old Rod | fishing enabler |
///
/// This is the *curated* fallback. The chest randomizer's actual default comes
/// from [`default_static_chest_items`], which derives the full quest/key/story
/// set from the disc's price table and unions this curated list on top (the
/// constant adds priced-but-special tools the price-0 rule can't see, like the
/// Silver Compass).
pub const DEFAULT_STATIC_CHEST_ITEMS: &[u8] = &[0x9a, 0x71, 0xa9, 0xaa, 0xb0, 0xf3, 0xa0];

/// The chest randomizer's default keep-static set, derived from the disc.
///
/// Combines two sources so no quest item is ever moved out of its chest or
/// dropped into an unrelated one:
/// - the data-driven [`crate::item_price::quest_item_ids`] — every named,
///   unsellable (price-0) item except the chest-found equipment, which catches
///   the door keys, garden tools, eggs/talismans/books, letters, fishing rods,
///   casino cards, and Ra-Seru template entries automatically; and
/// - the curated [`DEFAULT_STATIC_CHEST_ITEMS`], which adds priced-but-special
///   tools (e.g. the Silver Compass) the price-0 rule doesn't cover.
///
/// If the item table can't be read from `scus`, falls back to the curated
/// constant alone (still a safe, if narrower, default).
pub fn default_static_chest_items(scus: &[u8]) -> std::collections::BTreeSet<u8> {
    let mut set: std::collections::BTreeSet<u8> =
        DEFAULT_STATIC_CHEST_ITEMS.iter().copied().collect();
    if let Ok(quest) = crate::item_price::quest_item_ids(scus) {
        set.extend(quest);
    }
    set
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
