//! The pause menu's blips: which cue a frame's pad edges fire, decided once
//! for both hosts.
//!
//! The three ids are retail's own ring writes in the SCUS-resident list
//! kernel `FUN_80032A44`, the routine every pause-menu list window is paged
//! by. Its producer is `FUN_80035B50`'s ring enqueue, inlined, so each
//! literal sits beside its own store: the cursor step `li a2,0x21` at
//! `0x80032B9C`, the enabled-row confirm `li a1,0x20` at `0x80032D24`, the
//! cancel `li a2,0x37` at `0x80032D74`. A disabled row buzzes `0x23` instead
//! (`li a1,0x23` at `0x80032D0C`), a distinction neither host's edge mapping
//! has a path for. All three are category `0` in the descriptor table
//! (`sfx-table.md`), so they sound out of the slot-0 system bank.
//!
//! Start closing the whole menu from the root row list is the port's own
//! edge (retail's menu is closed through its list windows), and it blips as
//! the cancel it amounts to. Start inside a sub-screen closes nothing - the
//! sub-screen owns the pad - so it fires no cue of its own.

/// Cursor-step cue (`FUN_80032A44`, `li a2,0x21` at `0x80032B9C`).
pub const MENU_CURSOR_CUE: u8 = 0x21;
/// Enabled-row confirm cue (`FUN_80032A44`, `li a1,0x20` at `0x80032D24`).
pub const MENU_CONFIRM_CUE: u8 = 0x20;
/// Cancel cue (`FUN_80032A44`, `li a2,0x37` at `0x80032D74`).
pub const MENU_CANCEL_CUE: u8 = 0x37;

use crate::input::PadButton;

// Pad bits of the host pad word both hosts hand the engine.
const PAD_START: u16 = PadButton::Start as u16;
const PAD_DIRS: u16 = PadButton::Up as u16
    | PadButton::Right as u16
    | PadButton::Down as u16
    | PadButton::Left as u16;
const PAD_CIRCLE: u16 = PadButton::Circle as u16;
const PAD_CROSS: u16 = PadButton::Cross as u16;

/// One pause-menu blip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuBlip {
    Cursor,
    Confirm,
    Cancel,
}

impl MenuBlip {
    /// The descriptor id this blip keys.
    pub const fn cue(self) -> u8 {
        match self {
            Self::Cursor => MENU_CURSOR_CUE,
            Self::Confirm => MENU_CONFIRM_CUE,
            Self::Cancel => MENU_CANCEL_CUE,
        }
    }
}

/// The blip a frame's just-pressed `pressed` word fires while the pause menu
/// is up. `start_closes_menu` is whether this frame's Start closes the whole
/// menu - true on the root row list, false while a sub-screen owns the pad.
///
/// One cue per frame, in priority order: a closing Start is a cancel;
/// otherwise Cross confirms, Circle cancels, and a direction steps the
/// cursor. A frame with none of those fires nothing.
pub fn menu_edge_blip(pressed: u16, start_closes_menu: bool) -> Option<MenuBlip> {
    if start_closes_menu && pressed & PAD_START != 0 {
        return Some(MenuBlip::Cancel);
    }
    if pressed & PAD_CROSS != 0 {
        Some(MenuBlip::Confirm)
    } else if pressed & PAD_CIRCLE != 0 {
        Some(MenuBlip::Cancel)
    } else if pressed & PAD_DIRS != 0 {
        Some(MenuBlip::Cursor)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_ids_are_the_list_kernel_ring_writes() {
        assert_eq!(MenuBlip::Cursor.cue(), 0x21);
        assert_eq!(MenuBlip::Confirm.cue(), 0x20);
        assert_eq!(MenuBlip::Cancel.cue(), 0x37);
    }

    /// Start on the root list closes the menu and blips a cancel, whatever
    /// else is down on the same frame.
    #[test]
    fn a_closing_start_is_a_cancel() {
        assert_eq!(menu_edge_blip(PAD_START, true), Some(MenuBlip::Cancel));
        assert_eq!(
            menu_edge_blip(PAD_START | PAD_CROSS, true),
            Some(MenuBlip::Cancel)
        );
    }

    /// Start inside a sub-screen closes nothing and so fires nothing of its
    /// own; the frame's other edges still cue.
    #[test]
    fn start_in_a_sub_screen_is_silent() {
        assert_eq!(menu_edge_blip(PAD_START, false), None);
        assert_eq!(
            menu_edge_blip(PAD_START | PAD_CROSS, false),
            Some(MenuBlip::Confirm)
        );
    }

    #[test]
    fn cross_then_circle_then_a_direction() {
        assert_eq!(
            menu_edge_blip(PAD_CROSS | PAD_CIRCLE | 0x0010, false),
            Some(MenuBlip::Confirm)
        );
        assert_eq!(
            menu_edge_blip(PAD_CIRCLE | 0x0010, false),
            Some(MenuBlip::Cancel)
        );
        for dir in [0x0010, 0x0020, 0x0040, 0x0080] {
            assert_eq!(menu_edge_blip(dir, false), Some(MenuBlip::Cursor));
        }
        // Triangle / Square / nothing: no cue.
        assert_eq!(menu_edge_blip(0x1000 | 0x8000, false), None);
        assert_eq!(menu_edge_blip(0, true), None);
    }
}
