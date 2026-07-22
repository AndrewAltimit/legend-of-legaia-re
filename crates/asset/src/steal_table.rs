//! Per-monster **steal-item** table parser (`DAT_80077828` in `SCUS_942.54`).
//!
//! What the player steals from an enemy with the **Evil God Icon** equipped is
//! looked up here, NOT in the PROT 867 `battle_data` monster record. The reward
//! block at `+0x44..+0x49` holds only gold / exp / drop; the steal item lives in
//! this separate static `SCUS_942.54` table instead, which is why every
//! record-only search came up empty.
//!
//! That negative is disc-measured over the whole archive: for the 185 monster
//! ids that are both populated in PROT 867 and stealable in the SCUS table, no
//! byte offset carries the steal pair in either field order - not in the
//! 13,030,964 bytes of LZS-decoded monster block (every offset, full block
//! length, not just the `0x4C` stat head), nor in the 15,155,200 raw bytes of
//! the `0x14000` slots that hold them. Best agreement in any layer is
//! `[chance,item]` 2/185 and `[item,chance]` 2/185.
//!
//! Do not read the one elevated offset as a near-miss: single-byte offset `0x48`
//! scores 31/185, but `0x48` is the `drop_item` field, and steal and drop draw
//! from the same 39-item consumable pool, so incidental agreement is expected -
//! none of those 31 also agree on chance at `0x49`, and the best non-drop offset
//! anywhere is 7/185. A drop-order (`[item, chance]`) scan could not have faked
//! the negative either; it tops out at 2/185.
//!
//! Independent of any scan: monster ids `187..190` are stealable here but have
//! **no archive slot at all** (PROT 867 is 194 slots of `0x14000`, 186
//! populated), so the record cannot be the source for those ids.
//! See `docs/reference/re-settled-threads.md`.
//!
//! ## Record layout (2 bytes, stride `0x02`)
//!
//! The table is indexed by **1-based monster id** (the same id space as
//! [`crate::monster_archive`]); entry `M` sits at `TABLE_VA + M*2`. Entry `0`
//! is a reserved sentinel (there is no monster id 0).
//!
//! | Offset | Type | Field |
//! |---|---|---|
//! | `+0` | u8 | `steal_chance_pct` - steal success chance in percent |
//! | `+1` | u8 | `steal_item_id` - item id stolen (the same id space [`crate::item_names`] names; `0` = none) |
//!
//! Note the field order is **chance-then-item**, the reverse of the drop fields
//! in the monster record (`+0x48 item / +0x49 chance`). A `steal_chance_pct`
//! of `0` (or `steal_item_id == 0`) means the enemy has no steal.
//!
//! Provenance: pinned from a live player-steal RAM capture (Evil God Icon
//! equipped; Skeleton id 13 -> Incense `0x8a` @ 30%) and verified byte-exact
//! against the complete published steal table (item + chance) across every
//! resolvable monster id. The table is static rodata in the executable's data
//! segment, so it resolves the same way as [`crate::item_names`] /
//! [`crate::spell_names`].

/// RAM address of the steal table (`DAT_80077828`). Entry `M` is at
/// `TABLE_VA + M*2`.
pub const TABLE_VA: u32 = 0x8007_7828;
/// Per-id stride in bytes (one `[chance, item]` pair).
pub const RECORD_STRIDE: usize = 0x02;
/// Number of monster-id slots the table covers (`0..=255`; id 0 is the reserved
/// sentinel, real monsters are 1-based). The retail archive populates ids up to
/// ~190; covering the full byte id space keeps every reachable id in range.
pub const ENTRY_COUNT: usize = 256;

/// PSX-EXE `t_addr` -> file-offset resolver. `SCUS_942.54` loads its data
/// segment at `t_addr` from file offset `0x800` (same shape as the resolver in
/// [`crate::item_names`]; kept local so this module stands alone).
struct ExeMap {
    t_addr: u32,
    t_size: u32,
}

impl ExeMap {
    fn parse(scus: &[u8]) -> Option<Self> {
        if scus.len() < 0x800 || &scus[0..8] != b"PS-X EXE" {
            return None;
        }
        let t_addr = u32::from_le_bytes(scus[0x18..0x1C].try_into().ok()?);
        let t_size = u32::from_le_bytes(scus[0x1C..0x20].try_into().ok()?);
        Some(Self { t_addr, t_size })
    }

    /// File offset for a virtual address, or `None` if outside the data segment.
    fn off(&self, va: u32) -> Option<usize> {
        if va < self.t_addr || va >= self.t_addr.checked_add(self.t_size)? {
            return None;
        }
        Some((va - self.t_addr) as usize + 0x800)
    }
}

/// File offset within a `SCUS_942.54` image of the steal table's entry 0 - the
/// byte the `+id*2` indexing is relative to. Monster `M`'s chance byte is at
/// `base + M*2`, its item byte at `base + M*2 + 1`. Returns `None` if `scus`
/// isn't a PSX-EXE or the table address is outside its data segment.
///
/// The randomizer ([`legaia_patcher::steal`](../../../crates/patcher/src/steal.rs))
/// uses this to turn a monster id into a patch offset for the SCUS file.
pub fn table_file_offset(scus: &[u8]) -> Option<usize> {
    ExeMap::parse(scus)?.off(TABLE_VA)
}

/// One monster's steal entry: the item it yields and the success chance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct StealEntry {
    /// Steal success chance, percent (`+0`).
    pub chance_pct: u8,
    /// Stolen item id (`+1`; `0` = no item).
    pub item_id: u8,
}

impl StealEntry {
    /// Whether this entry yields a real steal (a non-zero item and a non-zero
    /// chance). A `0` item or `0` chance means the enemy can't be stolen from.
    pub fn is_stealable(&self) -> bool {
        self.item_id != 0 && self.chance_pct != 0
    }
}

/// The decoded steal table: one [`StealEntry`] per monster id (`0..=255`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StealTable {
    entries: Vec<StealEntry>,
}

impl StealTable {
    /// Parse the steal table out of a `SCUS_942.54` image. Returns `None` if the
    /// image isn't a PSX-EXE or the table address is out of range.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let map = ExeMap::parse(scus)?;
        let mut entries = Vec::with_capacity(ENTRY_COUNT);
        for id in 0..ENTRY_COUNT {
            let rec = map.off(TABLE_VA + (id * RECORD_STRIDE) as u32)?;
            let chance_pct = *scus.get(rec)?;
            let item_id = *scus.get(rec + 1)?;
            entries.push(StealEntry {
                chance_pct,
                item_id,
            });
        }
        Some(Self { entries })
    }

    /// Build directly from a list of entries (tests / non-SCUS callers).
    pub fn from_entries(entries: Vec<StealEntry>) -> Self {
        Self { entries }
    }

    /// The steal entry for monster id `monster_id` (1-based), or `None` for an
    /// out-of-range id. Note the entry may still be non-stealable
    /// ([`StealEntry::is_stealable`]).
    pub fn entry(&self, monster_id: u16) -> Option<StealEntry> {
        self.entries.get(monster_id as usize).copied()
    }

    /// The stolen item id for monster `monster_id`, or `None` when the id is out
    /// of range or the entry is non-stealable (`item == 0` or `chance == 0`).
    pub fn steal_item(&self, monster_id: u16) -> Option<u8> {
        self.entry(monster_id)
            .filter(StealEntry::is_stealable)
            .map(|e| e.item_id)
    }

    /// Number of id slots the table covers.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when the table holds no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Count of slots that hold a real steal ([`StealEntry::is_stealable`]).
    pub fn stealable_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_stealable()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal PSX-EXE image whose steal table holds the given
    /// `(chance, item)` entries (indexed from id 0), so the parser can be
    /// exercised without any Sony bytes.
    fn synth_scus(entries: &[(u8, u8)]) -> Vec<u8> {
        const T_ADDR: u32 = 0x8001_0000;
        let table_off = (TABLE_VA - T_ADDR) as usize + 0x800;
        let total = table_off + ENTRY_COUNT * RECORD_STRIDE + 0x10;
        let mut buf = vec![0u8; total];
        buf[0..8].copy_from_slice(b"PS-X EXE");
        buf[0x18..0x1C].copy_from_slice(&T_ADDR.to_le_bytes());
        let t_size = (total - 0x800) as u32;
        buf[0x1C..0x20].copy_from_slice(&t_size.to_le_bytes());
        for (id, &(chance, item)) in entries.iter().enumerate() {
            let rec = table_off + id * RECORD_STRIDE;
            buf[rec] = chance;
            buf[rec + 1] = item;
        }
        buf
    }

    #[test]
    fn parses_chance_then_item_indexed_by_monster_id() {
        // id 0 sentinel, id 1 = 30% item 0x7e, id 2 = 25% item 0x7f.
        let scus = synth_scus(&[(0, 0xff), (30, 0x7e), (25, 0x7f)]);
        let table = StealTable::from_scus(&scus).expect("parse");
        assert_eq!(table.len(), ENTRY_COUNT);
        assert_eq!(
            table.entry(1),
            Some(StealEntry {
                chance_pct: 30,
                item_id: 0x7e
            })
        );
        assert_eq!(table.steal_item(1), Some(0x7e));
        assert_eq!(table.steal_item(2), Some(0x7f));
        // Out-of-range id.
        assert_eq!(table.entry(9999), None);
    }

    #[test]
    fn non_stealable_entries_yield_none() {
        // chance 0 (no steal) and item 0 (no item) both count as non-stealable.
        let scus = synth_scus(&[(0, 0xff), (0, 0x7e), (30, 0x00)]);
        let table = StealTable::from_scus(&scus).unwrap();
        assert_eq!(table.steal_item(1), None, "0% chance = no steal");
        assert_eq!(table.steal_item(2), None, "0 item = no steal");
        assert!(!table.entry(1).unwrap().is_stealable());
        assert_eq!(table.stealable_count(), 0);
    }

    #[test]
    fn non_psx_exe_returns_none() {
        assert!(StealTable::from_scus(b"not an exe").is_none());
        assert!(StealTable::from_scus(&[0u8; 0x900]).is_none());
    }
}
