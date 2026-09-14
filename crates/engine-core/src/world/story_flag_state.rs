//! Story / system flag words: the retail flag arrays and the story-flag bit image the scripts test and set.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

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
    /// Rebuilt per tile crossing by [`crate::world::World::refresh_field_regions`] (the
    /// `FUN_800180EC` / `FUN_801DBA20` ports in [`crate::field_regions`])
    /// when the per-scene tables are installed; otherwise host-owned
    /// scene-local state.
    pub extra_flags: u32,
    /// Field-VM scratchpad flag word (`_DAT_1F800394` in retail). Set
    /// by op `0x2E` GFLAG_SET; cleared by op `0x2F` GFLAG_CLR; tested
    /// by op `0x30` GFLAG_TST.
    ///
    /// Independent of [`crate::world::StoryFlagState::story_flag_bits`]: retail seeds this from
    /// the game-mode descriptor table on mode init (low 16 bits of
    /// `mode_table[mode_idx].param`) and the SC save/load bulk copy
    /// from RAM `0x80084340` never reaches scratchpad, so the bitmap
    /// and this word are not mirror copies of each other.
    pub story_flags: u32,
    /// Full 512-byte story-flag bitmap mirroring retail RAM
    /// `0x80085600..0x80085800` (SC block offset `0x14C0`). This is the
    /// narrative-progress bitmap the SC block persists, separate from
    /// the per-mode scratchpad word [`crate::world::StoryFlagState::story_flags`].
    ///
    /// Empty (`vec![]`) when the engine hasn't been booted from a retail
    /// SC block; populated via [`crate::world::World::load_full`] when a retail-shaped
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
