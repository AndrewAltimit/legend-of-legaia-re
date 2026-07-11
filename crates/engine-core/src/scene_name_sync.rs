//! Scene-name sync for the name-based scene-change packet.
//!
//! PORT: FUN_8001D7F8
//! REF: FUN_8001FD44 (scene-change-packet API caller; ported in [`crate::dialog`])
//! REF: FUN_80056738 (BIOS strcmp), FUN_80056758 (BIOS strcpy)
//! REF: FUN_801D6704 (the field-init that loads the synced active buffer)
//!
//! Retail keeps three pieces of scene-name state:
//!
//! - the **staged** name at `0x8007050C` (8 bytes; `FUN_8001FD44` copies the
//!   packet's target name here),
//! - the **active** buffer at `0x80084548` (what the next field-init
//!   `FUN_801D6704` loads),
//! - the resolved **scene-index word** at `0x80084540` (the scene-bundle
//!   pool slot the streaming loaders key on).
//!
//! `FUN_8001D7F8` bridges them: lowercase the staged name in place
//! (`'A'..='Z'` +0x20, all 8 bytes), linear-scan the 150-entry in-RAM scene
//! name table at `0x80088758` (stride 16: NUL-terminated name + a u16 index
//! at `+0xC`) writing every strcmp match's index into the scene-index word
//! (no match leaves it untouched), then strcpy the staged name into the
//! active buffer.
//!
//! Clean-room boundary: `ghidra/scripts/funcs/8001d7f8.txt` is the spec; the
//! name table itself is built from the user's disc at runtime (CDNAME), so
//! no Sony bytes live here. Tests use synthetic tables.

/// Both scene-name buffers are 8 bytes in retail (`0x8007050C` /
/// `0x80084548`), NUL-terminated.
pub const SCENE_NAME_LEN: usize = 8;

/// One record of the in-RAM scene name table at `0x80088758` (16-byte
/// stride): a NUL-terminated name (12-byte field) and the u16 scene index
/// stored at record `+0xC`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneNameEntry {
    pub name: [u8; 12],
    pub index: u16,
}

impl SceneNameEntry {
    /// Convenience constructor from a `&str` name (truncated / NUL-padded to
    /// the 12-byte record field).
    pub fn new(name: &str, index: u16) -> Self {
        let mut buf = [0u8; 12];
        for (dst, src) in buf.iter_mut().zip(name.bytes()) {
            *dst = src;
        }
        Self { name: buf, index }
    }

    /// The record's name bytes up to (not including) its NUL terminator -
    /// what the retail strcmp compares against.
    fn name_bytes(&self) -> &[u8] {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(12);
        &self.name[..end]
    }
}

/// Sync the staged scene name into the active buffer, resolving the scene
/// index on the way:
///
/// 1. Lowercase `staged` in place - every byte in `'A'..='Z'` gets `+0x20`
///    (all [`SCENE_NAME_LEN`] bytes, even past the NUL, exactly as retail
///    walks the raw buffer).
/// 2. Scan `table` in order; every entry whose name strcmp-equals the staged
///    name writes its `index` into `scene_index` (the last match wins; no
///    match leaves the previous value in place - retail's loop has no early
///    exit and no "not found" write).
/// 3. strcpy `staged` into `active` (bytes up to and including the NUL;
///    trailing bytes of `active` keep their previous content, as retail's
///    BIOS strcpy does).
// PORT: FUN_8001D7F8
pub fn sync_scene_name(
    staged: &mut [u8; SCENE_NAME_LEN],
    table: &[SceneNameEntry],
    active: &mut [u8; SCENE_NAME_LEN],
    scene_index: &mut u32,
) {
    for b in staged.iter_mut() {
        if b.is_ascii_uppercase() {
            *b += 0x20;
        }
    }

    let staged_end = staged.iter().position(|&b| b == 0).unwrap_or(staged.len());
    let staged_name = &staged[..staged_end];
    for entry in table {
        if entry.name_bytes() == staged_name {
            *scene_index = u32::from(entry.index);
        }
    }

    // strcpy: copy through the NUL, leave the tail alone.
    for (dst, &src) in active.iter_mut().zip(staged.iter()) {
        *dst = src;
        if src == 0 {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(name: &str) -> [u8; SCENE_NAME_LEN] {
        let mut b = [0u8; SCENE_NAME_LEN];
        for (dst, src) in b.iter_mut().zip(name.bytes()) {
            *dst = src;
        }
        b
    }

    #[test]
    fn lowercases_staged_name_in_place() {
        let mut staged = buf("Town01");
        let mut active = [0u8; SCENE_NAME_LEN];
        let mut index = 0u32;
        sync_scene_name(&mut staged, &[], &mut active, &mut index);
        assert_eq!(&staged[..6], b"town01");
    }

    #[test]
    fn match_resolves_scene_index_and_copies_to_active() {
        let table = [
            SceneNameEntry::new("map01", 0x55),
            SceneNameEntry::new("town01", 0x12),
        ];
        let mut staged = buf("TOWN01");
        let mut active = [0u8; SCENE_NAME_LEN];
        let mut index = 0u32;
        sync_scene_name(&mut staged, &table, &mut active, &mut index);
        assert_eq!(index, 0x12, "matched entry's u16 lands in the index word");
        assert_eq!(&active[..7], b"town01\0");
    }

    #[test]
    fn no_match_leaves_scene_index_untouched() {
        let table = [SceneNameEntry::new("map01", 0x55)];
        let mut staged = buf("nowhere");
        let mut active = [0u8; SCENE_NAME_LEN];
        let mut index = 0xAB;
        sync_scene_name(&mut staged, &table, &mut active, &mut index);
        assert_eq!(index, 0xAB, "retail writes the index only on a match");
        assert_eq!(&active[..8], b"nowhere\0");
    }

    #[test]
    fn last_matching_entry_wins() {
        // Retail's scan has no early exit: a duplicate name later in the
        // table overwrites the earlier match's index.
        let table = [
            SceneNameEntry::new("dupe", 1),
            SceneNameEntry::new("dupe", 2),
        ];
        let mut staged = buf("dupe");
        let mut active = [0u8; SCENE_NAME_LEN];
        let mut index = 0u32;
        sync_scene_name(&mut staged, &table, &mut active, &mut index);
        assert_eq!(index, 2);
    }

    #[test]
    fn strcpy_leaves_active_tail_alone() {
        let mut staged = buf("ab");
        let mut active = *b"map01\0xx";
        let mut index = 0u32;
        sync_scene_name(&mut staged, &[], &mut active, &mut index);
        // "ab\0" copied; bytes past the NUL keep their previous content.
        assert_eq!(&active[..3], b"ab\0");
        assert_eq!(&active[3..], b"01\0xx");
    }

    #[test]
    fn comparison_happens_after_lowercasing() {
        // The table holds lowercase CDNAME names; an uppercase staged name
        // must still match (retail lowercases before the scan).
        let table = [SceneNameEntry::new("garmel", 7)];
        let mut staged = buf("GARMEL");
        let mut active = [0u8; SCENE_NAME_LEN];
        let mut index = 0u32;
        sync_scene_name(&mut staged, &table, &mut active, &mut index);
        assert_eq!(index, 7);
    }
}
