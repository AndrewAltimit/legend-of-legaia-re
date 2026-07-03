//! Capture observation: a story-Seru capture granting a spell into the character record.

/// Vahn's character-record base in retail RAM (the capturer here).
pub const VAHN_RECORD_BASE: u32 = 0x80084708;

/// Offset of the spell-list count byte within the record.
pub const SPELL_COUNT_OFFSET: u32 = 0x13C;
/// Offset of the spell-id array (first entry) within the record.
pub const SPELL_IDS_OFFSET: u32 = 0x13D;
/// Offset of the spell-level array (first entry) within the record.
pub const SPELL_LEVELS_OFFSET: u32 = 0x161;

/// Spell id Gimard teaches, observed at `+0x13D` post-capture.
pub const GIMARD_SPELL_ID: u8 = 0x81;
/// Spell level a freshly-captured story Seru is granted at.
pub const GRANTED_LEVEL: u8 = 1;

/// `(count, id[0], level[0])` before the capture - an empty spell list.
pub const BEFORE: (u8, u8, u8) = (0, 0, 0);
/// `(count, id[0], level[0])` after the capture.
pub const AFTER: (u8, u8, u8) = (1, GIMARD_SPELL_ID, GRANTED_LEVEL);

/// Read `(count, id[0], level[0])` for the given record base from a
/// main-RAM image. Returns `None` if the window is out of range.
pub fn read_spell_head(main_ram: &[u8], record_base: u32) -> Option<(u8, u8, u8)> {
    let base = (record_base - 0x80000000) as usize;
    let count = *main_ram.get(base + SPELL_COUNT_OFFSET as usize)?;
    let id0 = *main_ram.get(base + SPELL_IDS_OFFSET as usize)?;
    let lvl0 = *main_ram.get(base + SPELL_LEVELS_OFFSET as usize)?;
    Some((count, id0, lvl0))
}
