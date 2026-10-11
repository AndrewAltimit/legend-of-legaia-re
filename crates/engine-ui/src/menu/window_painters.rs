//! Content painters for a block of the menu overlay's **window-descriptor
//! table** (`legaia_game_tables::menu_windows`, 52 records at overlay VA
//! `0x801E4738`).
//!
//! Each descriptor's `renderer_va` names the routine that fills that
//! window's content rect; the 9-slice frame around it is drawn by the
//! caller, not here (see `docs/subsystems/field-menu.md`).
//! [`crate::menu::window_dispatch`] maps a parsed descriptor to the painter
//! below that draws it, the way the retail window walker maps the live
//! window's `+0x28` to a routine. This module ports the painters for the
//! tab / prompt / counter / shop-panel block:
//!
//! | window | routine | content |
//! |---|---|---|
//! | 5 | `FUN_801D61B0` | two-line prompt over a two-row choice |
//! | 6 | `FUN_801D6360` | six-row label list with a corner cursor |
//! | 7 | `FUN_801DCCB4` | one-line prompt with a substituted character |
//! | 24 | `FUN_801DCC20` | count field over a reserved sub-rect |
//! | 31 | `FUN_801DCE20` | label, wide number field, trailing label |
//! | 32 | `FUN_801DCF84` | pictogram + 8-digit counter |
//! | 33 | `FUN_801DCF14` | title tab whose text comes from a record |
//! | 34 | `FUN_801D4A80` | item name, owned count, description line |
//! | 36 | `FUN_801D56FC` | equip-target list gated on the character mask |
//! | 37 | `FUN_801D5944` | sell quantity + halved gold total |
//! | 43 | `FUN_801DCFE4` | plain title tab |
//! | 45 | `FUN_801DD028` | second pictogram + 8-digit counter |
//! | 46 | `FUN_801D603C` | one-line prompt over a two-row choice |
//!
//! Plus `FUN_801E4140` ([`guarded_box_rect`]), a bottom-clipped box emit
//! that sits beside them in the overlay but serves none of them: its one
//! call is in a title-tick arm retail never enters (see its tag).
//!
//! ## What the port keeps and what it drops
//!
//! Every routine writes the shared draw-order word `DAT_8007B454` (values
//! 5, 6 and 7) before each primitive and some also stage the glyph-advance
//! byte `DAT_80073F20` (`0x10` for the wide pass, `0x0C` for the narrow
//! one). Both are properties of retail's ordering table and its fixed-cell
//! text writer; a host that composites in call order and lays glyphs out
//! through [`legaia_font`] has no use for either, so the port keeps only
//! the **geometry** - the pen positions, the field widths, and the
//! cursor-variant arithmetic.
//!
//! Strings are caller-supplied. The routines resolve theirs from overlay
//! literals and from live records, which are disc bytes; the port takes
//! `&str` so the text can come from the translation layer instead.
//!
//! Evidence: `ghidra/scripts/funcs/overlay_menu_801d61b0.txt`,
//! `overlay_menu_801d6360.txt`, `overlay_menu_801dccb4.txt`,
//! `overlay_menu_801dcc20.txt`, `overlay_menu_801dce20.txt`,
//! `overlay_menu_801dcf84.txt`, `overlay_menu_801dcf14.txt`,
//! `overlay_menu_801dcfe4.txt`, `overlay_menu_801dd028.txt`,
//! `overlay_menu_801d603c.txt`, `overlay_menu_801d4a80.txt`,
//! `overlay_menu_801d56fc.txt`, `overlay_menu_801d5944.txt`,
//! `overlay_menu_801e4140.txt` (PROT entry 0899, the menu overlay).

use crate::*;

mod box_emit;
mod choices;
mod counter_panel;
mod prompts;
mod separators;
mod shop_items;
mod title_tabs;

pub use box_emit::*;
pub use choices::*;
pub use counter_panel::*;
pub use prompts::*;
pub use separators::*;
pub use shop_items::*;
pub use title_tabs::*;

/// A window's content rect, as the descriptor table stores it.
///
/// The painters read `+0xA`/`+0xC` as the content origin and `+0xE`/`+0x10`
/// as the content extent; those are exactly
/// `legaia_game_tables::menu_windows::MenuWindowDescriptor::rect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PainterRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl PainterRect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
}

/// One pointing-hand / marker sprite request (`FUN_8002B994`).
///
/// `sprite` and `variant` are the routine's first two arguments verbatim -
/// the port does not interpret them, because the sprite bank they index is
/// a VRAM resource, not a layout fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PainterSprite {
    pub sprite: u8,
    pub variant: u8,
    pub x: i32,
    pub y: i32,
}

/// One pictogram request (`FUN_8002C488`): a bank id drawn at a pen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PainterPictogram {
    pub id: u8,
    pub x: i32,
    pub y: i32,
}

/// Row pitch the label painters step by (`addiu s0,s0,0xe`).
pub const PAINTER_ROW_PITCH: i32 = 0x0E;

/// The **accent pen** three painters here stage for exactly one field before
/// restoring the default.
///
/// Retail stages `_DAT_8007B454 = 6` around window 34's item name, window
/// 24's count and window 31's number, and `7` for everything else in the
/// block. `6` is the same staging id the compare panels use for a rising
/// stat, so it resolves to the same colour ([`compare_panel_ink`]).
///
/// [`compare_panel_ink`]: crate::compare_panel_ink
pub const PAINTER_INK_ACCENT: [f32; 4] = MENU_TEXT_GOLD;

/// Right-hand pictogram id of the primary counter window (id 32).
pub const COUNTER_PICTOGRAM_PRIMARY: u8 = 0x62;
/// Pictogram id of the secondary counter window (id 45).
pub const COUNTER_PICTOGRAM_SECONDARY: u8 = 0x66;
/// Digit width both counter windows request (`li a1,0x8`).
pub const COUNTER_DIGITS: i32 = 8;
/// Horizontal gap between a counter's pictogram and its digit field.
pub const COUNTER_DIGIT_INSET: i32 = 0x28;

/// Advance of one fixed-width digit cell in the number writer.
const NUM_CELL_W: i32 = 8;

#[cfg(test)]
mod tests;
