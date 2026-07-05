//! Shared menu cursor-navigation primitive.
//!
//! PORT: FUN_801d688c
//!
//! The retail menu / shop / save-slot state-handlers all funnel their
//! list-cursor navigation through a single overlay helper
//! `FUN_801d688c(cursor: *u32, count, mode) -> 0/1/2/3`
//! (`ghidra/scripts/funcs/overlay_save_ui_select_801d688c.txt`). It reads the
//! overlay confirm / cancel pad masks (`_DAT_8007B874 & DAT_801EF0F0` /
//! `DAT_801EF0F4`) and the held-pad word `_DAT_8007BB84`, mutates the caller's
//! cursor cell in place, enqueues a UI SFX cue through `FUN_80035B50`, and
//! returns a small result enum:
//!
//! | retail | meaning | SFX cue |
//! |--------|---------|---------|
//! | `1`    | Confirm | `0x36`  |
//! | `2`    | Cancel  | `0x37`  |
//! | `3`    | Moved   | `0x21`  |
//! | `0`    | None    | -       |
//!
//! This module ports the primitive as engine idiom: a plain
//! [`menu_cursor_nav`] function over a caller-owned cursor cell plus a
//! [`NavButtons`] snapshot (the host derives the four booleans from
//! [`crate::input::InputState`]), returning a [`CursorNav`] whose
//! [`CursorNav::sfx_cue`] surfaces the retail cue id for the host to play
//! through its `SfxBank` - matching how the rest of engine-core surfaces
//! sound cues (return values, not a global enqueue).
//!
//! ## Cursor packing
//!
//! Retail treats the cursor cell as a packed word: the **low 12 bits**
//! ([`CURSOR_INDEX_MASK`]) are the list index and the **high nibble**
//! ([`CURSOR_FLAGS_MASK`], `0xF000`) carries caller-private flags that the
//! navigator preserves across a move. `menu_cursor_nav` reproduces that split
//! exactly (a wrap drops bits above `0xFFFF`, matching retail's `& 0xf000`
//! high-half mask), so a caller that packs flags in the high nibble can hand
//! its raw cell straight in. Callers that only need a plain index just pass a
//! value `< 0x1000` and read the index back with `cursor & CURSOR_INDEX_MASK`.

/// SFX cue enqueued on a Confirm (retail `func_0x80035b50(0x36)`).
pub const SFX_CONFIRM: u8 = 0x36;
/// SFX cue enqueued on a Cancel (retail `func_0x80035b50(0x37)`).
pub const SFX_CANCEL: u8 = 0x37;
/// SFX cue enqueued on a cursor move (retail `func_0x80035b50(0x21)`).
pub const SFX_MOVE: u8 = 0x21;

/// Low-12-bit list-index field of the packed cursor cell.
pub const CURSOR_INDEX_MASK: u32 = 0x0fff;
/// High-nibble caller-flag field the navigator preserves across a move
/// (retail's `& 0xf000` high half).
pub const CURSOR_FLAGS_MASK: u32 = 0xf000;

/// Result of a [`menu_cursor_nav`] call - the ported `FUN_801d688c` return
/// enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorNav {
    /// No confirm / cancel / move this frame (retail `0`).
    None,
    /// Confirm button edge (retail `1`).
    Confirm,
    /// Cancel button edge (retail `2`).
    Cancel,
    /// Cursor moved left or right (retail `3`).
    Moved,
}

impl CursorNav {
    /// The retail UI SFX cue id the primitive enqueues for this result, or
    /// `None` for [`CursorNav::None`]. Hosts play it through their `SfxBank`.
    pub fn sfx_cue(self) -> Option<u8> {
        match self {
            CursorNav::None => None,
            CursorNav::Confirm => Some(SFX_CONFIRM),
            CursorNav::Cancel => Some(SFX_CANCEL),
            CursorNav::Moved => Some(SFX_MOVE),
        }
    }

    /// `true` for [`CursorNav::Moved`].
    pub fn moved(self) -> bool {
        matches!(self, CursorNav::Moved)
    }
}

/// Per-frame button snapshot the navigator consumes. The host derives these
/// from [`crate::input::InputState`]: `confirm` / `cancel` are the game's
/// confirm / cancel bindings (retail `_DAT_8007B874 & DAT_801EF0F0` /
/// `DAT_801EF0F4`), and `left` / `right` are the held-pad decrement /
/// increment directions (retail `_DAT_8007BB84 & 0x1000` / `0x4000`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NavButtons {
    /// Confirm binding down this frame.
    pub confirm: bool,
    /// Cancel binding down this frame.
    pub cancel: bool,
    /// Move-left (decrement) direction down this frame.
    pub left: bool,
    /// Move-right (increment) direction down this frame.
    pub right: bool,
}

impl NavButtons {
    /// Convenience constructor.
    pub fn new(confirm: bool, cancel: bool, left: bool, right: bool) -> Self {
        Self {
            confirm,
            cancel,
            left,
            right,
        }
    }
}

/// Advance a packed menu cursor cell for one frame - the clean-room port of
/// `FUN_801d688c`.
///
/// `cursor` is the caller-owned packed cell (index in the low 12 bits, flags
/// in [`CURSOR_FLAGS_MASK`]); it is mutated in place on a move. `count` is the
/// list length. `wrap` selects the retail `mode` variant: `false` clamps at
/// the ends, `true` wraps around (retail passes `mode = 1` for every ported
/// call site).
///
/// Confirm and cancel are tested **before** the `count` guard, so a
/// zero-length list still reports Confirm / Cancel (matching retail). On a
/// move the cursor's low-12-bit index changes and the high-nibble flags are
/// preserved.
pub fn menu_cursor_nav(cursor: &mut u32, count: u32, wrap: bool, buttons: NavButtons) -> CursorNav {
    // Confirm / cancel first - retail checks these ahead of the count guard.
    if buttons.confirm {
        return CursorNav::Confirm;
    }
    if buttons.cancel {
        return CursorNav::Cancel;
    }
    if count == 0 {
        return CursorNav::None;
    }

    let mut moved = false;
    if wrap {
        if buttons.left {
            let idx = *cursor & CURSOR_INDEX_MASK;
            if idx == 0 {
                // Wrap to the last index, keeping the flag nibble.
                *cursor = (*cursor & CURSOR_FLAGS_MASK) | ((count - 1) & CURSOR_INDEX_MASK);
            } else {
                *cursor -= 1;
            }
            moved = true;
        }
        if buttons.right {
            let next = cursor.wrapping_add(1);
            *cursor = next;
            if (next & CURSOR_INDEX_MASK) == count {
                // Rolled past the last index - snap the index back to 0.
                *cursor = next & CURSOR_FLAGS_MASK;
            }
            moved = true;
        }
    } else {
        if buttons.left && (*cursor & CURSOR_INDEX_MASK) != 0 {
            *cursor -= 1;
            moved = true;
        }
        if buttons.right && (*cursor & CURSOR_INDEX_MASK) + 1 < count {
            *cursor += 1;
            moved = true;
        }
    }

    if moved {
        CursorNav::Moved
    } else {
        CursorNav::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buttons(confirm: bool, cancel: bool, left: bool, right: bool) -> NavButtons {
        NavButtons::new(confirm, cancel, left, right)
    }

    #[test]
    fn confirm_returns_confirm_with_cue() {
        let mut c = 0;
        let r = menu_cursor_nav(&mut c, 3, false, buttons(true, false, false, false));
        assert_eq!(r, CursorNav::Confirm);
        assert_eq!(r.sfx_cue(), Some(SFX_CONFIRM));
        assert_eq!(c, 0, "confirm must not move the cursor");
    }

    #[test]
    fn cancel_returns_cancel_with_cue() {
        let mut c = 1;
        let r = menu_cursor_nav(&mut c, 3, false, buttons(false, true, false, false));
        assert_eq!(r, CursorNav::Cancel);
        assert_eq!(r.sfx_cue(), Some(SFX_CANCEL));
        assert_eq!(c, 1);
    }

    #[test]
    fn confirm_beats_cancel_and_movement() {
        // Retail order: confirm tested before cancel before movement.
        let mut c = 1;
        let r = menu_cursor_nav(&mut c, 3, false, buttons(true, true, true, true));
        assert_eq!(r, CursorNav::Confirm);
        assert_eq!(c, 1);
    }

    #[test]
    fn confirm_fires_even_when_count_zero() {
        let mut c = 0;
        assert_eq!(
            menu_cursor_nav(&mut c, 0, false, buttons(true, false, false, false)),
            CursorNav::Confirm
        );
        assert_eq!(
            menu_cursor_nav(&mut c, 0, false, buttons(false, true, false, false)),
            CursorNav::Cancel
        );
    }

    #[test]
    fn no_input_returns_none() {
        let mut c = 1;
        let r = menu_cursor_nav(&mut c, 3, false, buttons(false, false, false, false));
        assert_eq!(r, CursorNav::None);
        assert_eq!(r.sfx_cue(), None);
        assert_eq!(c, 1);
    }

    #[test]
    fn clamp_left_moves_and_stops_at_zero() {
        let mut c = 2;
        // 2 -> 1
        assert_eq!(
            menu_cursor_nav(&mut c, 3, false, buttons(false, false, true, false)),
            CursorNav::Moved
        );
        assert_eq!(c, 1);
        // 1 -> 0
        assert_eq!(
            menu_cursor_nav(&mut c, 3, false, buttons(false, false, true, false)),
            CursorNav::Moved
        );
        assert_eq!(c, 0);
        // 0 -> 0 (clamped, no move)
        assert_eq!(
            menu_cursor_nav(&mut c, 3, false, buttons(false, false, true, false)),
            CursorNav::None
        );
        assert_eq!(c, 0);
    }

    #[test]
    fn clamp_right_moves_and_stops_at_count_minus_one() {
        let mut c = 0;
        assert_eq!(
            menu_cursor_nav(&mut c, 3, false, buttons(false, false, false, true)),
            CursorNav::Moved
        );
        assert_eq!(c, 1);
        assert_eq!(
            menu_cursor_nav(&mut c, 3, false, buttons(false, false, false, true)),
            CursorNav::Moved
        );
        assert_eq!(c, 2);
        // At last index: clamped.
        assert_eq!(
            menu_cursor_nav(&mut c, 3, false, buttons(false, false, false, true)),
            CursorNav::None
        );
        assert_eq!(c, 2);
    }

    #[test]
    fn wrap_left_from_zero_goes_to_last() {
        let mut c = 0;
        let r = menu_cursor_nav(&mut c, 3, true, buttons(false, false, true, false));
        assert_eq!(r, CursorNav::Moved);
        assert_eq!(c, 2);
    }

    #[test]
    fn wrap_right_from_last_goes_to_zero() {
        let mut c = 2;
        let r = menu_cursor_nav(&mut c, 3, true, buttons(false, false, false, true));
        assert_eq!(r, CursorNav::Moved);
        assert_eq!(c, 0);
    }

    #[test]
    fn wrap_two_item_toggles() {
        // The retail Yes/No confirm picker: FUN_801D688C(&cur, 2, 1).
        let mut c = 1;
        menu_cursor_nav(&mut c, 2, true, buttons(false, false, true, false));
        assert_eq!(c, 0);
        menu_cursor_nav(&mut c, 2, true, buttons(false, false, true, false));
        assert_eq!(c, 1, "left wraps 0 -> 1 for a 2-item list");
        menu_cursor_nav(&mut c, 2, true, buttons(false, false, false, true));
        assert_eq!(c, 0, "right wraps 1 -> 0 for a 2-item list");
    }

    #[test]
    fn flag_nibble_preserved_across_moves() {
        // Pack a caller flag in the high nibble; the index navigates while
        // the flag survives.
        let flag = 0xA000;
        let mut c = flag | 1;
        // clamp-left: 1 -> 0
        menu_cursor_nav(&mut c, 3, false, buttons(false, false, true, false));
        assert_eq!(c & CURSOR_INDEX_MASK, 0);
        assert_eq!(c & CURSOR_FLAGS_MASK, flag);
        // wrap-left from index 0 -> last, flag still there
        menu_cursor_nav(&mut c, 3, true, buttons(false, false, true, false));
        assert_eq!(c & CURSOR_INDEX_MASK, 2);
        assert_eq!(c & CURSOR_FLAGS_MASK, flag);
        // wrap-right past last -> index 0, flag preserved
        menu_cursor_nav(&mut c, 3, true, buttons(false, false, false, true));
        assert_eq!(c & CURSOR_INDEX_MASK, 0);
        assert_eq!(c & CURSOR_FLAGS_MASK, flag);
    }
}
