//! Inject a display name for the otherwise-unnamed accessory (item `0xFD`).
//!
//! The unnamed accessory ships with its item-name-table pointer aimed at the
//! shared empty-string slot, so without help it would render as a blank line
//! when the `--unused-items` toggle hands it out in a chest / drop / steal.
//! This gives it the name **"Seru Bell"** by the same same-size SCUS-patch
//! technique the starting-inventory seed uses: write the string into the
//! executable's reclaimable data-segment tail (zero-fill padding past the
//! string pool, found by [`legaia_asset::item_names::data_segment_free_tail`])
//! and repoint **only** id `0xFD`'s name pointer at it — the other ids that
//! share the empty-string slot are left alone.
//!
//! No game bytes are committed: the string is the randomizer's own, and the
//! write target is derived from the user's disc at runtime.

use legaia_asset::item_names;

/// Item id of the unnamed accessory.
pub const SERU_BELL_ID: u8 = 0xFD;
/// The name to give it. ASCII, exactly as the item-name renderer expects (the
/// retail name strings are ASCII glyph codes; a leading icon escape is
/// optional, so this plain name renders cleanly without one).
pub const SERU_BELL_NAME: &str = "Seru Bell";

/// A planned name injection: two same-size writes to `SCUS_942.54`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameInjection {
    /// Item id whose name pointer is repointed.
    pub id: u8,
    /// File offset of the `name_ptr` word to repoint (to `string_va`).
    pub ptr_file_off: usize,
    /// File offset where the NUL-terminated `name_bytes` are written.
    pub string_file_off: usize,
    /// Load VA of `string_file_off` (the value written into the pointer word).
    pub string_va: u32,
    /// The name bytes to write (ASCII + a trailing NUL).
    pub name_bytes: Vec<u8>,
}

impl NameInjection {
    /// Plan injecting `name` for item `id` into a `SCUS_942.54` image. Returns
    /// `None` if the executable layout can't be resolved or there's no
    /// reclaimable tail space big enough for the string.
    pub fn plan(scus: &[u8], id: u8, name: &str) -> Option<Self> {
        let mut name_bytes = name.as_bytes().to_vec();
        name_bytes.push(0); // NUL terminator
        let (ptr_file_off, _current) = item_names::name_ptr_slot(scus, id)?;
        let (string_file_off, string_va) =
            item_names::data_segment_free_tail(scus, name_bytes.len())?;
        Some(Self {
            id,
            ptr_file_off,
            string_file_off,
            string_va,
            name_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tiny PS-X EXE with the item table + a string pool + a zero tail,
    /// so the planner can be exercised without any Sony bytes.
    fn synth_scus() -> Vec<u8> {
        use legaia_asset::item_names::{RECORD_STRIDE, TABLE_VA};
        const T_ADDR: u32 = 0x8001_0000;
        let table_off = (TABLE_VA - T_ADDR) as usize + 0x800;
        let table_bytes = 256 * RECORD_STRIDE;
        let pool_va = TABLE_VA + table_bytes as u32;
        let pool_off = (pool_va - T_ADDR) as usize + 0x800;
        // One shared empty string at the pool start; id 0xFD points at it.
        let empty_va = pool_va;
        let total = pool_off + 1 /* the NUL */ + 0x40 /* zero tail */;
        let mut buf = vec![0u8; total];
        buf[0..8].copy_from_slice(b"PS-X EXE");
        buf[0x18..0x1C].copy_from_slice(&T_ADDR.to_le_bytes());
        buf[0x1C..0x20].copy_from_slice(&((total - 0x800) as u32).to_le_bytes());
        let rec = table_off + 0xFD * RECORD_STRIDE;
        buf[rec..rec + 4].copy_from_slice(&empty_va.to_le_bytes());
        buf
    }

    #[test]
    fn plan_targets_dead_space_and_repoints_only_the_chosen_slot() {
        let scus = synth_scus();
        let plan = NameInjection::plan(&scus, SERU_BELL_ID, SERU_BELL_NAME).expect("plan");
        assert_eq!(plan.name_bytes, b"Seru Bell\0");
        // The pointer word is the 0xFD slot; the string goes into all-zero tail.
        let (slot_off, cur) = item_names::name_ptr_slot(&scus, SERU_BELL_ID).unwrap();
        assert_eq!(plan.ptr_file_off, slot_off);
        assert_ne!(
            plan.string_va, cur,
            "repoint moves the pointer off the empty slot"
        );
        assert!(
            scus[plan.string_file_off..plan.string_file_off + plan.name_bytes.len()]
                .iter()
                .all(|&b| b == 0),
            "string target is dead space"
        );
    }

    #[test]
    fn applying_the_plan_makes_the_item_resolve_to_the_name() {
        let mut scus = synth_scus();
        let plan = NameInjection::plan(&scus, SERU_BELL_ID, SERU_BELL_NAME).expect("plan");
        // Apply both writes in memory, exactly as the disc patcher would.
        scus[plan.string_file_off..plan.string_file_off + plan.name_bytes.len()]
            .copy_from_slice(&plan.name_bytes);
        scus[plan.ptr_file_off..plan.ptr_file_off + 4]
            .copy_from_slice(&plan.string_va.to_le_bytes());
        let table = item_names::ItemNameTable::from_scus(&scus).unwrap();
        assert_eq!(table.name(SERU_BELL_ID), Some(SERU_BELL_NAME));
    }
}
