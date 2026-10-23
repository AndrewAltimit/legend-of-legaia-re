//! Equipment classification: recover which item ids are equippable gear
//! (weapons / armor / accessories) from the disc's own item-name table.
//!
//! This is the id pool the **bonus equipment drop** ([`legaia_code_hooks::bonus_drop`])
//! embeds in its injected reward-routine table - the low-chance extra drop picks
//! uniformly from it. A monster has a single drop slot, so equipment is not a
//! data edit of that slot (which would destroy the normal drop) but an additive
//! code hook; this module only supplies the id list.
//!
//! ## Classification (no committed Sony bytes)
//!
//! The retail item id space is one flat 256-entry table shared by consumables,
//! key items, and equipment (see [`legaia_game_tables::item_names`]). Nothing on the
//! disc cleanly flags "this id is a weapon" in a single byte, so we classify by
//! **name**: every weapon / armor / accessory in the curated, public
//! [`legaia_gamedata`] tables is matched (case-insensitively) against the
//! disc's own item-name table to recover its id. The names come from public
//! walkthroughs and ship in the repo; the ids come from the *user's* disc at
//! runtime - no Sony bytes are embedded, and the join double-checks the curated
//! tables against the real executable.

use anyhow::{Context, Result};

use legaia_game_tables::item_names::ItemNameTable;
use legaia_gamedata::Database;

/// One equippable item eligible to be granted as the bonus equipment drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EquipmentItem {
    /// Item id in the shared 256-entry id space (the value the injected reward
    /// routine passes to the inventory-add helper).
    pub id: u8,
    /// Curated gamedata gold price, or `None` for quest-only gear. Retained for
    /// read-only listings; the bonus drop itself picks uniformly by id.
    pub price: Option<u32>,
}

/// Build the equipment pool from a `SCUS_942.54` image: every weapon, armor and
/// accessory in the curated gamedata tables whose name resolves to a real id in
/// the disc's item-name table, deduplicated and sorted by id. Each entry keeps
/// its gamedata price for listings.
///
/// Returns an error only if `scus` isn't a PSX-EXE / the item table is absent.
/// Items the gamedata names but the disc doesn't (or vice versa) are simply
/// absent from the pool - a few character-default weapons and quest items don't
/// match by name, which is harmless for a drop pool.
pub fn equipment_pool(scus: &[u8]) -> Result<Vec<EquipmentItem>> {
    let table = ItemNameTable::from_scus(scus)
        .context("SCUS_942.54 is not a PSX-EXE / item table absent")?;
    // The join lives in `legaia-gamedata`, which the engine's bonus-drop
    // twin calls too, so the two pools cannot drift.
    Ok(Database::load()
        .equipment_pool_by_name(|id| table.name(id).map(str::to_string))
        .into_iter()
        .map(|(id, price)| EquipmentItem { id, price })
        .collect())
}

/// The equipment ids alone (sorted), for the bonus-drop id table.
pub fn equipment_ids(scus: &[u8]) -> Result<Vec<u8>> {
    Ok(equipment_pool(scus)?.iter().map(|e| e.id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equipment_item_is_copy_and_ordered_by_id() {
        let mut v = [
            EquipmentItem {
                id: 0x46,
                price: Some(1),
            },
            EquipmentItem {
                id: 0x22,
                price: None,
            },
        ];
        v.sort_by_key(|e| e.id);
        assert_eq!(v[0].id, 0x22);
        assert_eq!(v[1].id, 0x46);
    }
}
