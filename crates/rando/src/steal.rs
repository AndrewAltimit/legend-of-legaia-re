//! Steal-item randomization: reassign the per-monster **steal items** in the
//! static `SCUS_942.54` table (`DAT_80077828`).
//!
//! Unlike drops (which live in the LZS-packed PROT 867 record) the steal item
//! is a plain byte in an executable table: entry `M` for monster id `M`
//! (1-based) is `[steal_chance_pct: u8, steal_item_id: u8]` at
//! `table_base + M*2` (see [`legaia_asset::steal_table`] and
//! `docs/formats/steal-table.md`). An edit is therefore a single same-size byte
//! overwrite of the **item** byte (the chance is preserved) — no re-pack, no
//! overflow risk, so nothing is ever skipped.
//!
//! The planning is identical to the drop randomizer's (reassign the item for
//! every *stealable* monster, keep its chance), so this reuses
//! [`crate::drops::plan_drops`]; only the addressing differs (SCUS file offset
//! vs. PROT record). `Shuffle` redistributes the existing steal-item multiset
//! among the stealable monsters; `Random` draws each from the item pool.

use legaia_asset::steal_table::{self, StealTable};

use crate::drops::{CurrentDrop, DropAssignment, DropMode, plan_drops};

/// ISO 9660 file holding the steal table.
pub const SCUS_NAME: &str = "SCUS_942.54";

/// The steal table located inside `SCUS_942.54`, ready to plan + emit patches.
pub struct StealEdits {
    /// File offset of the table's entry 0 within `SCUS_942.54` (monster `M`'s
    /// item byte is at `table_file_off + M*2 + 1`).
    table_file_off: usize,
    /// Decoded table, indexed by monster id.
    table: StealTable,
}

impl StealEdits {
    /// Locate + decode the steal table from a whole disc image. Returns `None`
    /// when `SCUS_942.54` isn't present or isn't a parseable PSX-EXE.
    pub fn locate(image: &[u8]) -> Option<Self> {
        let scus = legaia_iso::iso9660::read_file_in_image(image, SCUS_NAME)?;
        let table = StealTable::from_scus(&scus)?;
        let table_file_off = steal_table::table_file_offset(&scus)?;
        Some(Self {
            table_file_off,
            table,
        })
    }

    /// Every stealable monster's current steal as a [`CurrentDrop`] (so the
    /// shared [`plan_drops`] planner applies unchanged): `item` = steal item,
    /// `chance` = steal chance. Non-stealable entries (`item == 0` or
    /// `chance == 0`) are omitted, in ascending monster-id order.
    pub fn current(&self) -> Vec<CurrentDrop> {
        (1..self.table.len() as u16)
            .filter_map(|id| {
                let e = self.table.entry(id)?;
                e.is_stealable().then_some(CurrentDrop {
                    monster_id: id,
                    item: e.item_id,
                    chance: e.chance_pct,
                })
            })
            .collect()
    }

    /// Plan a reassignment from a seed (delegates to the shared drop planner).
    /// `item_pool` is only consulted for [`DropMode::Random`].
    pub fn plan(&self, item_pool: &[u8], seed: u64, mode: DropMode) -> Vec<DropAssignment> {
        plan_drops(&self.current(), item_pool, seed, mode)
    }

    /// File offset of monster `id`'s steal **item** byte within `SCUS_942.54`.
    pub fn item_file_offset(&self, monster_id: u16) -> u64 {
        (self.table_file_off + monster_id as usize * 2 + 1) as u64
    }

    /// Turn a plan into `(scus_file_offset, new_item)` byte patches, dropping
    /// no-op assignments (where the planned item equals the current one). The
    /// chance byte is never touched.
    pub fn item_patches(&self, plan: &[DropAssignment]) -> Vec<(u64, u8)> {
        plan.iter()
            .filter_map(|a| {
                let cur = self.table.entry(a.monster_id)?.item_id;
                (a.item != cur).then_some((self.item_file_offset(a.monster_id), a.item))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::steal_table::StealEntry;

    fn synth() -> StealEdits {
        // ids: 0 sentinel, 1 = 30%/0x7e, 2 = 0%/0 (not stealable), 3 = 20%/0xf2.
        let entries = vec![
            StealEntry {
                chance_pct: 0,
                item_id: 0xff,
            },
            StealEntry {
                chance_pct: 30,
                item_id: 0x7e,
            },
            StealEntry {
                chance_pct: 0,
                item_id: 0,
            },
            StealEntry {
                chance_pct: 20,
                item_id: 0xf2,
            },
        ];
        StealEdits {
            table_file_off: 0x68028,
            table: StealTable::from_entries(entries),
        }
    }

    #[test]
    fn current_lists_only_stealable_monsters() {
        let s = synth();
        let cur = s.current();
        let ids: Vec<u16> = cur.iter().map(|c| c.monster_id).collect();
        assert_eq!(ids, vec![1, 3], "id 2 (0% / no item) is excluded");
        assert_eq!(cur[0].item, 0x7e);
        assert_eq!(cur[0].chance, 30);
    }

    #[test]
    fn item_offset_targets_the_item_byte_not_the_chance() {
        let s = synth();
        // entry M item byte = base + M*2 + 1.
        assert_eq!(s.item_file_offset(1), 0x68028 + 2 + 1);
        assert_eq!(s.item_file_offset(13), 0x68028 + 26 + 1);
    }

    #[test]
    fn shuffle_patches_preserve_the_item_multiset_and_skip_noops() {
        let s = synth();
        let plan = s.plan(&[], 0x99, DropMode::Shuffle);
        // Only stealable monsters are planned.
        assert_eq!(plan.len(), 2);
        // The planned item multiset equals the original stealable items.
        let mut got: Vec<u8> = plan.iter().map(|a| a.item).collect();
        got.sort_unstable();
        assert_eq!(got, vec![0x7e, 0xf2]);
        // Patches target item bytes only, and any no-op (item unchanged) is
        // dropped.
        for (off, _item) in s.item_patches(&plan) {
            assert!(
                off % 2 == 1,
                "item byte sits at an odd file offset (base+2M+1)"
            );
        }
    }

    #[test]
    fn random_draws_only_from_pool() {
        let s = synth();
        let pool = vec![0x10, 0x20, 0x30];
        let plan = s.plan(&pool, 7, DropMode::Random);
        for a in &plan {
            assert!(pool.contains(&a.item), "item {} not in pool", a.item);
        }
        // Chance is preserved (Random keeps each monster's steal rate).
        assert_eq!(plan[0].chance, 30);
    }
}
