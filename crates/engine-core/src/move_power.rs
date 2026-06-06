//! Engine-side wrapper over the battle-action **move-power table**
//! ([`legaia_asset::move_power`], runtime VA `0x801F4F5C`, PROT entry 0898).
//!
//! The table is the one true per-move power scalar in the battle system: the
//! arts / physical damage kernel `FUN_801dd0ac` reads its `+0` power for the
//! attacker roll (`rand % ((power >> 2) + 1) + … + power`). The asset crate
//! parses it and the `0x801F4E63` id → index map off the raw overlay bytes;
//! this module pairs the two so a live battle actor's chosen move id
//! (`actor[+0x1df]`, carried on the engine side as a battle move id) resolves
//! straight to its power record.
//!
//! Loaded lazily from PROT entry 0898 by [`crate::scene::SceneHost`] and parked
//! on [`crate::world::World::move_power`]; the monster special-attack damage
//! path consumes it (see `World::enemy_move_predamage`). Disc-free / synthetic
//! battles leave it `None` and keep the placeholder damage path, so no
//! determinism trace changes when the table is absent.

use legaia_asset::move_power::{self, MOVE_ID_INDEX_MAP_LEN, MoveRecord};

/// PROT / CDNAME index of the battle-action overlay holding the table.
pub const BATTLE_ACTION_OVERLAY_PROT_ENTRY: u32 =
    move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX as u32;

/// The parsed move-power table + its id → index map, ready for id lookups.
#[derive(Debug, Clone)]
pub struct MovePowerCatalog {
    /// 26-byte power records (index 0 is the unused all-zero slot).
    table: Vec<MoveRecord>,
    /// 128-byte battle-move-id → table-index map (`0x801F4E63`).
    id_index_map: [u8; MOVE_ID_INDEX_MAP_LEN],
}

impl MovePowerCatalog {
    /// Parse the table + map out of the raw PROT 0898 (battle-action overlay)
    /// entry bytes. Returns `None` if either structural guard fails (the pinned
    /// offsets no longer land on the table — e.g. a different build).
    pub fn from_overlay_0898(overlay_0898: &[u8]) -> Option<Self> {
        let table = move_power::parse(overlay_0898)?;
        let id_index_map = move_power::parse_id_index_map(overlay_0898)?;
        Some(Self {
            table,
            id_index_map,
        })
    }

    /// The power record for a battle move id (`actor[+0x1df]`), via the id →
    /// index map. `None` for ids the map marks as having no power record
    /// (`0`/`0xFF`) or out of the map's `0x00..=0x7F` range.
    pub fn record_for_move_id(&self, move_id: u8) -> Option<&MoveRecord> {
        move_power::record_for_move_id(&self.table, &self.id_index_map, move_id)
    }

    /// The roll-modulus base power `FUN_801dd0ac` derives from a move id's
    /// record (`(i16)power >> 2`), or `None` when the id has no record. This is
    /// the `power` fed to [`legaia_engine_vm::battle_formulas::arts_physical_predamage`].
    pub fn power_for_move_id(&self, move_id: u8) -> Option<i32> {
        self.record_for_move_id(move_id).map(|r| r.power())
    }

    /// Number of parsed records (including the unused slot 0).
    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::move_power::{
        MOVE_ID_INDEX_MAP_FILE_OFFSET, MOVE_POWER_RECORD_STRIDE, MOVE_POWER_TABLE_FILE_OFFSET,
        MOVE_POWER_TABLE_LEN,
    };

    /// Build a synthetic PROT-0898-shaped buffer with a known map + table so the
    /// wrapper can be exercised without a disc.
    fn synthetic_overlay() -> Vec<u8> {
        let mut buf = vec![
            0u8;
            MOVE_POWER_TABLE_FILE_OFFSET
                + MOVE_POWER_RECORD_STRIDE * MOVE_POWER_TABLE_LEN
        ];
        // map[4] = 1 (the structural guard + first mapped id), map[5] = 2.
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1;
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 5] = 2;
        // table record 1 power 0x02ee (>>2 = 187), record 2 power 0x09c4 (625).
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE] = 0xee;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE + 1] = 0x02;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE * 2] = 0xc4;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE * 2 + 1] = 0x09;
        buf
    }

    #[test]
    fn resolves_move_ids_through_the_map() {
        let cat = MovePowerCatalog::from_overlay_0898(&synthetic_overlay()).expect("parses");
        assert_eq!(cat.power_for_move_id(4), Some(187));
        assert_eq!(cat.power_for_move_id(5), Some(625));
        // Unmapped ids (map byte 0) resolve to no record.
        assert_eq!(cat.power_for_move_id(6), None);
        assert_eq!(cat.power_for_move_id(0), None);
        assert!(cat.record_for_move_id(4).is_some());
    }

    #[test]
    fn rejects_a_buffer_that_misses_the_table() {
        // All zeros: map[4] != 1 guard fails, so no catalog.
        let buf = vec![
            0u8;
            MOVE_POWER_TABLE_FILE_OFFSET
                + MOVE_POWER_RECORD_STRIDE * MOVE_POWER_TABLE_LEN
        ];
        assert!(MovePowerCatalog::from_overlay_0898(&buf).is_none());
        // Too short.
        assert!(MovePowerCatalog::from_overlay_0898(&[0u8; 16]).is_none());
    }
}
