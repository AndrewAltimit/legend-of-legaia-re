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

/// Width of the boot scene-name field the dev `initmap.txt` override is
/// read into (`0x10` bytes at `0x8007050C`). The override text is read as a
/// raw line, so it carries whatever line terminator the host file used;
/// [`sanitize_initmap_scene_name`] strips those in place.
pub const INITMAP_NAME_FIELD_LEN: usize = 0x10;

/// Strip the text-file line terminators out of a scene-name field read from
/// the dev `initmap.txt` override, in place.
///
/// PORT: FUN_8001D424 (the initmap-override sanitizer loop at
/// `0x8001D758..0x8001D7B0`; the rest of `FUN_8001D424` is display-env /
/// GTE-scratchpad / work-table init and calls into already-ported helpers -
/// see the crate notes, it is not game-state logic and is not ported here)
///
/// Retail reads the override line into the 16-byte field at `0x8007050C`
/// (via `FUN_8001A8B0(&DAT_8007050C, line, 0x10)`), then walks all 16 bytes
/// nulling any EOF (`0x1A`), LF (`0x0A`) or CR (`0x0D`) byte it finds:
///
/// ```text
///   for i in 0..0x10:
///       if buf[i] == 0x1A { buf[i] = 0 }   // MS-DOS EOF marker
///       if buf[i] == 0x0A { buf[i] = 0 }   // LF
///       if buf[i] == 0x0D { buf[i] = 0 }   // CR
/// ```
///
/// This is a per-byte null-out, not a truncate: each terminator byte
/// becomes a NUL where it sits, so a `"town01\r\n"` line resolves to a
/// clean NUL-terminated `"town01"`. Retail only takes this path on the dev
/// arm (`_DAT_8007B8C2 == 0`); retail hardware boots with the flag set and
/// keeps the compiled-in default scene name untouched. The buffer is
/// caller-supplied - no Sony bytes live here.
pub fn sanitize_initmap_scene_name(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        if *b == 0x1A || *b == 0x0A || *b == 0x0D {
            *b = 0;
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

    // ---- sanitize_initmap_scene_name (FUN_8001D424 override loop) ----

    #[test]
    fn sanitize_strips_crlf_into_nul_terminated_name() {
        let mut field = [0u8; INITMAP_NAME_FIELD_LEN];
        field[..8].copy_from_slice(b"town01\r\n");
        sanitize_initmap_scene_name(&mut field);
        // The CR and LF became NULs; the name reads clean up to the first NUL.
        let end = field.iter().position(|&b| b == 0).unwrap();
        assert_eq!(&field[..end], b"town01");
    }

    #[test]
    fn sanitize_nulls_dos_eof_marker() {
        let mut field = *b"map01\x1axxxxxxxxxx";
        sanitize_initmap_scene_name(&mut field);
        assert_eq!(field[5], 0, "0x1A EOF marker becomes NUL");
        assert_eq!(&field[..5], b"map01");
        // Bytes after the EOF are left as-is (per-byte null-out, not truncate).
        assert_eq!(&field[6..], b"xxxxxxxxxx");
    }

    #[test]
    fn sanitize_leaves_clean_name_untouched() {
        let mut field = [0u8; INITMAP_NAME_FIELD_LEN];
        field[..7].copy_from_slice(b"garmel\0");
        let before = field;
        sanitize_initmap_scene_name(&mut field);
        assert_eq!(field, before);
    }
}
