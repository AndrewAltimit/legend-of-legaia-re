//! Panel constants: the packed pad word, action masks, slot counts and the flash release counters.

use super::*;

/// Convert a raw BIOS pad word into the packed layout every kernel in this
/// module reads.
///
/// The two words hold the same 16 buttons with the byte halves swapped
/// (`FUN_8001822C` builds `~((b2 << 8) | b3)`), so the conversion is a byte
/// swap. Two fixed points pin the direction: Cross is `0x4000` raw and `0x40`
/// packed, Up is `0x0010` raw and `0x1000` packed.
pub fn packed_pad(raw: u16) -> u16 {
    raw.swap_bytes()
}

/// Packed-pad mask for action button A (`_DAT_800846D0`), the confirm the
/// shared list cursor tests against the **held** word.
pub const PANEL_ACTION_A: u32 = 0x0040;
/// Packed-pad mask for action button B (`_DAT_800846D4`), the cancel.
pub const PANEL_ACTION_B: u32 = 0x0020;

/// Panel-descriptor slots the host allocates.
///
/// Retail's array is overlay data at `0x801F2B98`; the engine sizes it to
/// cover every index the ported arms address - the flag window's `14`
/// ([`crate::world_map_panel_actors::FLAG_WINDOW_PANEL_INDEX`])
/// and the party panel's `7`
/// ([`crate::world_map_panel::PARTY_PANEL_INDEX`]).
pub const PANEL_SLOTS: usize = 16;

/// Panel index the port's stand-in sub-list script opens.
pub const SUBLIST_PANEL_INDEX: i16 = 3;

/// Value the host arms the flash counter `_DAT_8007B43C` with to release a
/// parked brightness flash.
///
/// Not a port-invented number: it is retail's own
/// [`FLASH_COUNTER_SEED_A`]` + `[`FLASH_COUNTER_DONE_STEP`] (`4 + 3`), the
/// value the menu overlay's save-side completion screen leaves behind. The
/// ramp-down arm decodes it as `phase = counter - 1`, so `7` selects the
/// phase-6 terminal arm rather than the phase-5 dead end.
pub const FLASH_RELEASE_COUNTER: i32 = FLASH_COUNTER_SEED_A + FLASH_COUNTER_DONE_STEP;

/// The sibling release value, `5 + 3`, which selects the phase-7 terminal arm
/// and the other handler id.
pub const FLASH_RELEASE_COUNTER_ALT: i32 = FLASH_COUNTER_SEED_B + FLASH_COUNTER_DONE_STEP;
