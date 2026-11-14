//! The pause menu's blips: which cue a frame's pad edges fire, decided once
//! for both hosts.
//!
//! The pause menu's rows are paged by the menu overlay's picker `FUN_801D688C`
//! (21 callers in PROT 0899): a step pushes `0x21` (`0x801D6928`, `0x801D697C`,
//! ...), Cross `0x36` (`0x801D68C8`), Circle `0x37` (`0x801D68F0`). A screen
//! that accepts the pick then overwrites the ring's last slot through
//! `FUN_80035BD0`: `0x20` for most, `0x23` for a refusal. The root picker
//! `FUN_801D6B20` overwrites only for Load and Save (`0x801D6C98`, `0x801D6CD0`,
//! `0x801D6CE8`), so Items through Options sound the bare `0x36`. A retail walk
//! of the menu reads the same (`scripts/pcsx-redux/autorun_keyon_census.lua` with
//! a pad script): `0x36` on the root's Items row, `0x36` then `0x20` on the
//! Items window's Use. The cursor steps and cancel keep the list kernel's ids
//! (`FUN_80032A44`, the widget system's kind-4 list). All are category `0` in
//! the descriptor table (`sfx-table.md`), so they sound out of the slot-0 system
//! bank.
//!
//! Start closing the whole menu from the root row list is the port's own
//! edge (retail's menu is closed through its list windows), and it blips as
//! the cancel it amounts to. Start inside a sub-screen closes nothing - the
//! sub-screen owns the pad - so it fires no cue of its own.

/// Cursor-step cue (`FUN_80032A44`, `li a2,0x21` at `0x80032B9C`).
pub const MENU_CURSOR_CUE: u8 = 0x21;
/// Enabled-row confirm cue (`FUN_80032A44`, `li a1,0x20` at `0x80032D24`).
pub const MENU_CONFIRM_CUE: u8 = 0x20;
/// Cancel cue (`FUN_801D688C`, `li a0,0x37` at `0x801D68F4`).
pub const MENU_CANCEL_CUE: u8 = 0x37;
/// The picker's bare pick (`FUN_801D688C`, `li a0,0x36` at `0x801D68CC`),
/// which the root's Items through Options rows leave sounding.
pub const MENU_PICK_CUE: u8 = 0x36;
/// A refused row (`FUN_801D6B20`, `li a0,0x23` at `0x801D6CEC`).
pub const MENU_REFUSE_CUE: u8 = 0x23;
/// An item used or a spell cast from the pause menu. The four apply screens
/// overwrite the picker's pick with `0` (`FUN_80035BD0`) and push `0x25`
/// (`FUN_80035B50`): the single-target item apply at `0x801D84B8`, the
/// all-party apply at `0x801D8120`, the group cast at `0x801D9368` and the
/// target cast at `0x801D9680`.
pub const MENU_USE_CUE: u8 = 0x25;
/// An equipment change from the pause menu's Equip screen: Best Equipment
/// that swapped a piece (`FUN_801D99F0`, `li a0,0x24` at `0x801D9B7C`), and
/// the candidate list's commit or occupied Remove (`FUN_801D9C14`,
/// `0x801DA110` / `0x801DA154`).
pub const MENU_EQUIP_CUE: u8 = 0x24;

use legaia_engine_system::input::PadButton;

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
    /// The root picker's rows that overwrite nothing.
    Pick,
    /// A gated row.
    Refuse,
    /// An apply screen's use or cast ([`MENU_USE_CUE`]).
    Use,
    /// An equipment change ([`MENU_EQUIP_CUE`]).
    Equip,
    /// A confirm the screen silences: it overwrites the picker's pick with
    /// `0` and pushes nothing (the Equip candidate list's already-worn row,
    /// `FUN_801D9C14`'s `0x801DA274`). Hosts never key it; the frame's rule
    /// answers `None` ([`MenuBlip::sounds`]).
    Silent,
}

impl MenuBlip {
    /// The descriptor id this blip keys.
    pub const fn cue(self) -> u8 {
        match self {
            Self::Cursor => MENU_CURSOR_CUE,
            Self::Confirm => MENU_CONFIRM_CUE,
            Self::Cancel => MENU_CANCEL_CUE,
            Self::Pick => MENU_PICK_CUE,
            Self::Refuse => MENU_REFUSE_CUE,
            Self::Use => MENU_USE_CUE,
            Self::Equip => MENU_EQUIP_CUE,
            Self::Silent => 0,
        }
    }

    /// `false` for [`Self::Silent`], the one blip that keys nothing.
    pub const fn sounds(self) -> bool {
        !matches!(self, Self::Silent)
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
    fn the_ids_are_the_pickers_ring_writes() {
        assert_eq!(MenuBlip::Cursor.cue(), 0x21);
        assert_eq!(MenuBlip::Confirm.cue(), 0x20);
        assert_eq!(MenuBlip::Cancel.cue(), 0x37);
        assert_eq!(MenuBlip::Pick.cue(), 0x36);
        assert_eq!(MenuBlip::Refuse.cue(), 0x23);
        assert_eq!(MenuBlip::Use.cue(), 0x25);
        assert_eq!(MenuBlip::Equip.cue(), 0x24);
        assert!(!MenuBlip::Silent.sounds());
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
