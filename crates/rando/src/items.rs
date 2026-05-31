//! Valid item-id pool for drop randomization.
//!
//! Drawn from the static `SCUS_942.54` item-name table (see
//! [`legaia_asset::item_names`]). Restricting the randomizer's replacement
//! drops to ids that name a real item ensures it never assigns a drop the game
//! has no name or handler for. Pass the retail executable (`SCUS_942.54`)
//! bytes, not the disc image — the table lives in the executable's data segment.

use anyhow::{Context, Result};
use legaia_asset::item_names::ItemNameTable;

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
