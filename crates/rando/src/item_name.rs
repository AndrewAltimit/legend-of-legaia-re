//! Inject a display name for the otherwise-unnamed accessory (item `0xFD`).
//!
//! The unnamed accessory ships with its item-name-table pointer aimed at the
//! shared empty-string slot, so without help it would render as a blank line
//! when the `--unused-items` toggle hands it out in a chest / drop / steal.
//! This gives it the name **"Seru Bell"** by a same-size `SCUS_942.54` patch:
//! write the string into a reserved constant region of the executable and
//! repoint **only** id `0xFD`'s name pointer at it — the other ids that share
//! the empty-string slot (`0x12`/`0x1A`/`0x52`/`0xB9`) are left alone.
//!
//! ## Where the string goes (and where it must NOT)
//!
//! The naive choice — the trailing zero-fill at the end of the data segment —
//! is **wrong**: that span is zero in the file but is `.sbss`/`.bss`-class
//! scratch the game overwrites with variables at runtime, so a string placed
//! there renders as a single glyph that changes every frame (confirmed in
//! mednafen: the name pointer read back correctly but the bytes at it were live
//! counters). The string must instead land in a region that is **constant at
//! runtime**.
//!
//! [`SERU_BELL_STRING_VA`] is such a region — pinned for the US retail build by
//! intersecting the all-zero runs across seventeen diverse savestates (battle /
//! field / menu / world-map / title): a 3376-byte block at `0x80079840` is zero
//! in the file *and* in every captured state, so it is reserved space the game
//! never writes, not transient scratch. The injection lands the string well
//! inside that block and guards on the file bytes there being zero, so a
//! differently-laid-out image is skipped rather than corrupted.
//!
//! No game bytes are committed: the string is the randomizer's own, and the
//! write is validated against the user's disc at runtime.

use legaia_asset::item_names;

/// Item id of the unnamed accessory.
pub const SERU_BELL_ID: u8 = 0xFD;
/// The name to give it. ASCII, exactly as the item-name renderer expects (the
/// retail name strings are ASCII glyph codes; a leading icon escape is
/// optional, so this plain name renders cleanly without one).
pub const SERU_BELL_NAME: &str = "Seru Bell";

/// Virtual address the injected name string is written to — a reserved,
/// runtime-constant region of the US-build data segment (see the module docs:
/// inside the 3376-byte block at `0x80079840` that is zero across the file and
/// every sampled runtime state). Sixteen-byte aligned, deep inside the block so
/// it is clear of the used region that begins just below `0x80079840`.
pub const SERU_BELL_STRING_VA: u32 = 0x8007_9900;

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
    /// Plan injecting `name` for item `id` into a `SCUS_942.54` image, stashing
    /// the string at [`SERU_BELL_STRING_VA`]. Returns `None` if the executable
    /// layout can't be resolved, the target VA is outside the segment, or the
    /// target bytes there aren't all zero (so a differently-laid-out image is
    /// skipped, never corrupted).
    pub fn plan(scus: &[u8], id: u8, name: &str) -> Option<Self> {
        Self::plan_at(scus, id, name, SERU_BELL_STRING_VA)
    }

    /// Like [`Self::plan`] but with an explicit target VA (for tests).
    pub fn plan_at(scus: &[u8], id: u8, name: &str, string_va: u32) -> Option<Self> {
        let mut name_bytes = name.as_bytes().to_vec();
        name_bytes.push(0); // NUL terminator
        let (ptr_file_off, _current) = item_names::name_ptr_slot(scus, id)?;
        let string_file_off = item_names::file_offset_for_va(scus, string_va)?;
        // The target must be genuine dead space in the file: all-zero across the
        // span we'd write. Bail (rather than corrupt) if it isn't.
        let end = string_file_off.checked_add(name_bytes.len())?;
        if scus.get(string_file_off..end)?.iter().any(|&b| b != 0) {
            return None;
        }
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
    /// so the planner can be exercised without any Sony bytes. The target VA is
    /// passed explicitly (the pinned US-build VA wouldn't fall inside a tiny
    /// synthetic image).
    fn synth_scus() -> (Vec<u8>, u32) {
        use legaia_asset::item_names::{RECORD_STRIDE, TABLE_VA};
        const T_ADDR: u32 = 0x8001_0000;
        let table_off = (TABLE_VA - T_ADDR) as usize + 0x800;
        let table_bytes = 256 * RECORD_STRIDE;
        let pool_va = TABLE_VA + table_bytes as u32;
        let pool_off = (pool_va - T_ADDR) as usize + 0x800;
        let empty_va = pool_va; // id 0xFD points at the shared empty string
        let total = pool_off + 1 /* the NUL */ + 0x40 /* zero tail */;
        let mut buf = vec![0u8; total];
        buf[0..8].copy_from_slice(b"PS-X EXE");
        buf[0x18..0x1C].copy_from_slice(&T_ADDR.to_le_bytes());
        buf[0x1C..0x20].copy_from_slice(&((total - 0x800) as u32).to_le_bytes());
        let rec = table_off + 0xFD * RECORD_STRIDE;
        buf[rec..rec + 4].copy_from_slice(&empty_va.to_le_bytes());
        // A zero region inside the (small) synthetic segment to stash into.
        let target_va = TABLE_VA - 0x20;
        (buf, target_va)
    }

    #[test]
    fn plan_targets_dead_space_and_repoints_only_the_chosen_slot() {
        let (scus, target) = synth_scus();
        let plan =
            NameInjection::plan_at(&scus, SERU_BELL_ID, SERU_BELL_NAME, target).expect("plan");
        assert_eq!(plan.name_bytes, b"Seru Bell\0");
        assert_eq!(plan.string_va, target);
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
    fn plan_refuses_a_nonzero_target() {
        let (mut scus, target) = synth_scus();
        let off = item_names::file_offset_for_va(&scus, target).unwrap();
        scus[off] = 0x42; // pretend something lives there
        assert!(
            NameInjection::plan_at(&scus, SERU_BELL_ID, SERU_BELL_NAME, target).is_none(),
            "must not stash a string over non-zero bytes"
        );
    }

    #[test]
    fn applying_the_plan_makes_the_item_resolve_to_the_name() {
        let (mut scus, target) = synth_scus();
        let plan =
            NameInjection::plan_at(&scus, SERU_BELL_ID, SERU_BELL_NAME, target).expect("plan");
        scus[plan.string_file_off..plan.string_file_off + plan.name_bytes.len()]
            .copy_from_slice(&plan.name_bytes);
        scus[plan.ptr_file_off..plan.ptr_file_off + 4]
            .copy_from_slice(&plan.string_va.to_le_bytes());
        let table = item_names::ItemNameTable::from_scus(&scus).unwrap();
        assert_eq!(table.name(SERU_BELL_ID), Some(SERU_BELL_NAME));
    }
}
