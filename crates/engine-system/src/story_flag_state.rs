//! Story / system flag words: the retail flag arrays and the story-flag bit image the scripts test and set.
//!
//! One subsystem's state, held as a unit on the composite `World`.
//! Fields keep their retail provenance notes.

/// Story / system flag words: the retail flag arrays and the story-flag bit image the scripts test and set.
pub struct StoryFlagState {
    /// Shared system flag bank at `_DAT_80085758` - bitfield read / written
    /// by:
    /// - field VM high-byte default routes 0x5x / 0x6x / 0x7x
    ///   (`system_flag_set` / `system_flag_clear` / `system_flag_test`)
    /// - move-VM ext sub-ops 0x13 / 0x14 / 0x1C / 0x1D
    ///   (`ext_query_flag_bank` / `ext_set_flag_bank` / `ext_clear_flag_bank`)
    ///
    /// Lazily grown on write - the field VM's opcode-encoded idx ranges over
    /// `0..=0x87FF`, so a fixed 256-bit array is too small.
    pub system_flags: Vec<u8>,
    /// Field-VM `extra_flags` register read by op 0x42 mode 0 - the
    /// `_DAT_8007B8F4` **region-type mask**: bit `n` set when the player's
    /// tile sits inside a type-`n` region of the scene `.MAP` region table.
    /// Latched, as retail latches it: rebuilt by the scene-entry and
    /// warp-landing re-centres, the scripted player moves, `[4C 3D]` and the
    /// camera-zone queries (the `FUN_800180EC` / `FUN_801DBA20` ports in
    /// [`legaia_engine_field::field_regions`]), and per frame only behind
    /// scratchpad bit 22 - never by a walk's tile crossing. Host-owned
    /// scene-local state when no per-scene tables are installed.
    pub extra_flags: u32,
    /// Field-VM scratchpad flag word (`_DAT_1F800394` in retail). Set
    /// by op `0x2E` GFLAG_SET; cleared by op `0x2F` GFLAG_CLR; tested
    /// by op `0x30` GFLAG_TST.
    ///
    /// Independent of [`StoryFlagState::story_flag_bits`]: retail seeds this from
    /// the game-mode descriptor table on mode init (low 16 bits of
    /// `mode_table[mode_idx].param`) and the SC save/load bulk copy
    /// from RAM `0x80084340` never reaches scratchpad, so the bitmap
    /// and this word are not mirror copies of each other.
    pub story_flags: u32,
    /// The SC block's window at offset `0x14C0`, mirroring retail RAM
    /// `0x80085600..`, carried for the save and load only. In retail the
    /// window is not a flag array of its own: its first `0x158` bytes are
    /// record slot 3's tail (`0x80085344 + 0x2BC..0x414`, Terra's template
    /// record), and from `+0x158` on it is the system-flag bank
    /// [`StoryFlagState::system_flags`] holds live. No retail code addresses
    /// `0x80085600..0x80085758` by a static `lui` pair or base offset (a
    /// register-tracking sweep of `SCUS_942.54` and every PROT entry finds
    /// none); every flag access is `FUN_8003CE08` / `FUN_8003CE34` /
    /// `FUN_8003CE64` (base `0x80085758`, byte `idx >> 3`) or op `4C 32`'s
    /// clear of the bank. So no script can see the two vectors part: the
    /// bank is read and written through `system_flags` alone, and the save
    /// folds it back into this window at `+0x158`
    /// (`engine-save-state`'s `save_full` / `load_story_flags`).
    ///
    /// Empty (`vec![]`) when the engine hasn't been booted from a retail
    /// SC block; populated via `World::load_full` when a retail-shaped
    /// [`legaia_save::SaveFile`] is restored.
    pub story_flag_bits: Vec<u8>,
}

impl StoryFlagState {
    pub fn new() -> Self {
        Self {
            system_flags: Vec::new(),
            extra_flags: 0,
            story_flags: 0,
            story_flag_bits: Vec::new(),
        }
    }
}

impl Default for StoryFlagState {
    fn default() -> Self {
        Self::new()
    }
}
