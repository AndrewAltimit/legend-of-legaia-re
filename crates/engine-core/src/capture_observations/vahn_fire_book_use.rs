/// Vahn's character-record base in retail RAM.
pub const VAHN_RECORD_BASE: u32 = 0x80084708;

/// Offset of the changed cluster within Vahn's record. Aliased by
/// [`legaia_save::character::CharacterRecord::displayed_skills`].
pub const CHANGED_OFFSET: u32 = 0x185;

/// Length of the changed cluster.
pub const CHANGED_LEN: usize = 3;

/// Pre-event bytes at `VAHN_RECORD_BASE + CHANGED_OFFSET`.
pub const BEFORE: [u8; 3] = [0x01, 0x0C, 0x00];

/// Post-event bytes at `VAHN_RECORD_BASE + CHANGED_OFFSET`.
pub const AFTER: [u8; 3] = [0x02, 0x03, 0x0C];

/// Address of the menu-overlay reader's leading instruction (`lbu
/// t2,0x185(t2)`) - the loop that surfaces the displayed-skill list.
pub const MENU_READER_ADDR: u32 = 0x801D4440;

/// Address of the menu-overlay function the reader belongs to.
/// Same address shows up across `overlay_menu_*`, `overlay_save_ui_*`,
/// and `overlay_shop_save_*` dumps - they're identical copies of the
/// menu overlay function.
pub const MENU_OVERLAY_FN: u32 = 0x801D33D8;

/// Absolute address of the cluster (handy for direct callers).
pub const fn changed_addr() -> u32 {
    VAHN_RECORD_BASE + CHANGED_OFFSET
}
