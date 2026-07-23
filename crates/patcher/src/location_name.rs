//! **Location / landmark name renaming.**
//!
//! The 16 place names shown on the world-map quick-travel menu and echoed by
//! the save / load / pause location display live in a fixed table in
//! `SCUS_942.54`: [`legaia_asset::worldmap_menu::NAME_TABLE_ADDR`] = `0x80073B18`,
//! [`legaia_asset::worldmap_menu::NAME_COUNT`] = 16 slots of
//! [`legaia_asset::worldmap_menu::NAME_STRIDE`] = `0x20` bytes each, every slot
//! a NUL-terminated ASCII string zero-padded to 32 bytes.
//!
//! Renaming is a same-size in-place edit of one slot: write the new ASCII name
//! and zero-pad the remainder of the 32-byte slot (so no stale tail bytes
//! remain), keeping at least one trailing NUL - i.e. a new name is at most 31
//! bytes. This is the same overwrite mechanism the item / spell name tables use.
//! No Sony bytes are embedded; only the user's own disc strings are rewritten.
//!
//! The default table (element caves at idx 3/4, "Vidna" at 6, "Conkram" at 14)
//! is what an element-swap hack renames - e.g. rename "Ancient Wind Cave" to
//! "Ancient Fire Cave" to match a re-elemented party.

use anyhow::{Result, bail};

use legaia_asset::item_names::file_offset_for_va;
use legaia_asset::worldmap_menu::{NAME_COUNT, NAME_STRIDE, NAME_TABLE_ADDR};

/// Max bytes a renamed slot's string can use (31 chars + a terminating NUL in
/// the 32-byte slot).
pub const MAX_NAME_LEN: usize = NAME_STRIDE - 1;

/// One planned rename: the SCUS file offset of a name slot and the new 32-byte
/// slot contents (ASCII + NUL padding).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameEdit {
    /// Landmark index (0..16).
    pub index: usize,
    /// SCUS file offset of the slot.
    pub offset: usize,
    /// The prior name (decoded).
    pub old_name: String,
    /// The new name.
    pub new_name: String,
    /// The full 32-byte replacement slot (ASCII + zero padding).
    pub slot: [u8; NAME_STRIDE],
}

/// SCUS file offset of landmark slot `index`.
pub fn slot_offset(scus: &[u8], index: usize) -> Option<usize> {
    let va = NAME_TABLE_ADDR + (index * NAME_STRIDE) as u32;
    file_offset_for_va(scus, va)
}

/// Decode the current name of landmark `index` from its SCUS slot.
pub fn current_name(scus: &[u8], index: usize) -> Option<String> {
    let off = slot_offset(scus, index)?;
    let slot = scus.get(off..off + NAME_STRIDE)?;
    let end = slot.iter().position(|&b| b == 0).unwrap_or(NAME_STRIDE);
    Some(String::from_utf8_lossy(&slot[..end]).into_owned())
}

/// List all 16 landmark names (index, name), for UX.
pub fn list_names(scus: &[u8]) -> Result<Vec<(usize, String)>> {
    let mut out = Vec::with_capacity(NAME_COUNT);
    for i in 0..NAME_COUNT {
        let name = current_name(scus, i)
            .ok_or_else(|| anyhow::anyhow!("landmark name table not resolvable in SCUS"))?;
        out.push((i, name));
    }
    Ok(out)
}

/// Plan a rename of landmark `index` to `new_name`. Fails on an out-of-range
/// index, a name that doesn't fit the 32-byte slot, a non-ASCII name (the
/// dialog font only renders the ASCII set here), or an unresolvable table.
/// Returns `Ok(None)` when the name already matches (idempotent no-op).
pub fn plan_rename(scus: &[u8], index: usize, new_name: &str) -> Result<Option<RenameEdit>> {
    if index >= NAME_COUNT {
        bail!("landmark index {index} out of range (0..{NAME_COUNT})");
    }
    if new_name.len() > MAX_NAME_LEN {
        bail!(
            "name {new_name:?} is {} bytes; the slot holds at most {MAX_NAME_LEN} (plus a NUL)",
            new_name.len()
        );
    }
    if !new_name.is_ascii() {
        bail!("name {new_name:?} has non-ASCII bytes (the menu font renders ASCII only here)");
    }
    let off = slot_offset(scus, index)
        .ok_or_else(|| anyhow::anyhow!("landmark slot {index} unresolvable"))?;
    let old_name = current_name(scus, index).unwrap_or_default();
    if old_name == new_name {
        return Ok(None);
    }
    let mut slot = [0u8; NAME_STRIDE];
    slot[..new_name.len()].copy_from_slice(new_name.as_bytes());
    Ok(Some(RenameEdit {
        index,
        offset: off,
        old_name,
        new_name: new_name.to_string(),
        slot,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::worldmap_menu::SCUS_LOAD_ADDR;

    /// Build a synthetic SCUS-EXE-shaped buffer with the name table populated.
    fn synthetic(names: &[&str]) -> Vec<u8> {
        // Enough buffer to cover the table's file offset.
        let table_off = 0x800 + (NAME_TABLE_ADDR - SCUS_LOAD_ADDR) as usize;
        let mut scus = vec![0u8; table_off + NAME_COUNT * NAME_STRIDE + 0x20];
        // Minimal PS-X EXE header so file_offset_for_va resolves: magic + t_addr
        // + t_size are what the resolver reads. Mirror worldmap_menu's mapping
        // (file = va - t_addr + 0x800). We fake it by writing the header fields.
        scus[0..8].copy_from_slice(b"PS-X EXE");
        scus[0x18..0x1C].copy_from_slice(&SCUS_LOAD_ADDR.to_le_bytes()); // t_addr
        // t_size must cover the table's file offset (file = va - t_addr + 0x800).
        let t_size = (scus.len() - 0x800) as u32;
        scus[0x1C..0x20].copy_from_slice(&t_size.to_le_bytes());
        for (i, n) in names.iter().enumerate() {
            let b = table_off + i * NAME_STRIDE;
            scus[b..b + n.len()].copy_from_slice(n.as_bytes());
        }
        scus
    }

    #[test]
    fn plan_rewrites_the_slot_and_zero_pads() {
        let scus = synthetic(&["Ancient Wind Cave", "Vidna"]);
        let edit = plan_rename(&scus, 0, "Ancient Fire Cave")
            .expect("valid")
            .expect("changed");
        assert_eq!(edit.old_name, "Ancient Wind Cave");
        assert_eq!(&edit.slot[..17], b"Ancient Fire Cave");
        // Everything past the name is zero (no stale tail).
        assert!(edit.slot[17..].iter().all(|&b| b == 0));
    }

    #[test]
    fn same_name_is_a_noop() {
        let scus = synthetic(&["Vidna"]);
        assert!(plan_rename(&scus, 0, "Vidna").unwrap().is_none());
    }

    #[test]
    fn refuses_too_long_non_ascii_and_oob() {
        let scus = synthetic(&["Vidna"]);
        assert!(plan_rename(&scus, 0, &"x".repeat(32)).is_err());
        assert!(plan_rename(&scus, 0, "Vïdna").is_err());
        assert!(plan_rename(&scus, NAME_COUNT, "X").is_err());
    }

    #[test]
    fn list_reads_all_slots() {
        let scus = synthetic(&["A", "B", "C"]);
        let names = list_names(&scus).unwrap();
        assert_eq!(names.len(), NAME_COUNT);
        assert_eq!(names[0].1, "A");
        assert_eq!(names[2].1, "C");
        assert_eq!(names[3].1, ""); // unpopulated slot decodes empty
    }
}
