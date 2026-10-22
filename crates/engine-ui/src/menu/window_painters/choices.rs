//! Options / choice windows - 46 and 5 - and the label list.

use super::*;

// ---------------------------------------------------------------------
// Options / choice windows - 46 and 5
// ---------------------------------------------------------------------

/// The choice-state word both two-row option windows branch on
/// (`DAT_801E46D0`).
///
/// The bit layout, read off the branch chain the two routines share:
///
/// * `0x4000` - suppress every marker. Set while the group is inert.
/// * `0x2000` - the group is mid-change. The marker variant then depends
///   only on `0x1000`: clear gives variant `4`, set gives variant `0`, and
///   **both rows draw** because this arm never compares the row index.
/// * otherwise - the low 12 bits are the selected row index and only the
///   matching row draws, with variant `1` when bit `0x1000` is clear and
///   `0` when it is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChoiceFlags(pub u32);

impl ChoiceFlags {
    /// Marker variant for `row`, or `None` when that row draws no marker.
    ///
    /// REF: FUN_801D603C (the branch chain; `FUN_801D61B0` repeats it
    /// verbatim for its own two rows - both are tagged on the painters
    /// that own them)
    pub fn marker_variant(self, row: u32) -> Option<u8> {
        let f = self.0;
        if f & 0x4000 != 0 {
            return None;
        }
        if f & 0x2000 != 0 {
            return Some(if f & 0x1000 == 0 { 4 } else { 0 });
        }
        if f & 0xFFF != row {
            return None;
        }
        Some(((f >> 12) ^ 1) as u8 & 1)
    }
}

/// One row of a two-row choice group: its label pen and its marker pen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChoiceRow {
    pub label_pen: (i32, i32),
    pub marker_pen: (i32, i32),
}

/// Geometry of the shared two-row choice block.
///
/// Both windows put the marker column at `marker_x` and the label column
/// `0x14` further right, and step the second row down by one row pitch.
fn choice_rows(marker_x: i32, first_row_y: i32) -> [ChoiceRow; 2] {
    [
        ChoiceRow {
            label_pen: (marker_x + 0x14, first_row_y),
            marker_pen: (marker_x, first_row_y),
        },
        ChoiceRow {
            label_pen: (marker_x + 0x14, first_row_y + PAINTER_ROW_PITCH),
            marker_pen: (marker_x, first_row_y + PAINTER_ROW_PITCH),
        },
    ]
}

/// Emit the markers a choice group's state calls for.
fn choice_marker_sprites(rows: &[ChoiceRow; 2], flags: ChoiceFlags) -> Vec<PainterSprite> {
    rows.iter()
        .enumerate()
        .filter_map(|(i, r)| {
            flags.marker_variant(i as u32).map(|variant| PainterSprite {
                sprite: 0,
                variant,
                x: r.marker_pen.0,
                y: r.marker_pen.1,
            })
        })
        .collect()
}

/// Window 46: one heading line over a two-row choice group.
///
/// `FUN_801D603C` draws the heading at the content origin, then puts the
/// choice block's marker column at `WX + 0x30` with its first row one
/// `0x10` step below the heading.
///
/// **This is the casino prize counter's Yes/No confirm**, not an options
/// screen. Exactly one command in the menu overlay opens window `0x2E`:
/// script `0x801E4F2C`, run from sub-screen `0x20` (`FUN_801DC1CC`, the
/// prize exchange) at the point where a chosen prize passes the coin and
/// held-cap gates. The sibling script `0x801E4F34` closes it again on No or
/// cancel, and the exchange seeds the cursor to row `1` first - retail's
/// "No is the default" convention.
///
/// PORT: FUN_801D603C
/// REF: FUN_801d5de0 - window 44's content renderer, the prize list this
/// confirm sits over. Ported as [`crate::prize_exchange::prize_row_ink`].
/// REF: FUN_801d688c - the shared picker that maintains the cursor word the
/// `flags` argument carries. Ported as `engine-menus::shop::shop_cursor_mode`.
///
/// The sub-screen-`0x20` host this painter waited on exists now: the
/// op-`0x49` sub-op-7 arm (`World::try_arm_prize_exchange`, over the PROT
/// 0899 file `0x15D00` block table read beside `menu_windows::parse`)
/// stages an `engine-minigames::prize_exchange::PrizeExchangeSession`, the menu
/// runtime drives it, and both hosts draw it through
/// [`crate::prize_exchange::prize_exchange_draws_for`] - which calls
/// this painter for window 46 whenever the session's Yes/No phase is up.
pub fn choice_panel_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    heading: &str,
    choices: [&str; 2],
    flags: ChoiceFlags,
) -> (Vec<TextDraw>, Vec<PainterSprite>) {
    let rows = choice_rows(rect.x + 0x30, rect.y + 0x10);
    let mut out = text_draws_for(
        &font.layout_ascii(heading),
        (rect.x, rect.y),
        MENU_TEXT_WHITE,
    );
    for (row, label) in rows.iter().zip(choices) {
        out.extend(text_draws_for(
            &font.layout_ascii(label),
            row.label_pen,
            MENU_TEXT_WHITE,
        ));
    }
    (out, choice_marker_sprites(&rows, flags))
}

/// Window 5: two heading lines over the same two-row choice group.
///
/// `FUN_801D61B0` steps `0x0E` between its two headings and then `0x10`
/// again before the choice block, and sets the marker column at
/// `WX + 0x3C` rather than `WX + 0x30`. Everything below that is the same
/// branch chain as window 46.
///
/// Its screen is a different one from window 46's. Script `0x801E4BD4`
/// (`05 00`, `01 05`) is the only opener, and it belongs to menu sub-screen
/// `3` (`FUN_801D6D38`): a two-row confirm seeded to row `1` = No, whose
/// Yes routes to sub-screen `0` (leave the menu) and whose No returns to
/// the pause root. The pause root reaches it from **cancel**, and only
/// under one condition - `_DAT_8007B450` non-null with its first byte
/// `0x0D`, the entry-context kind (`0x801d6cf8..0x801d6d18`).
///
/// It is a **battle-start ready check**, not a "really leave?" gate: the
/// renderer's own two heading pointers are `0x801CEC78` / `0x801CEC94`, and
/// its Yes exits the menu into the fight the entry context was staged for.
/// The distinction matters because the routing alone reads either way.
///
/// PORT: FUN_801D61B0
///
/// Live on both hosts: `FieldMenuSession::ReadyConfirm` is the phase, and
/// the same two host builders that draw window 6 draw this one, off the
/// same disc-parsed rects and the same disc-read labels.
pub fn two_line_choice_panel_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    headings: [&str; 2],
    choices: [&str; 2],
    flags: ChoiceFlags,
) -> (Vec<TextDraw>, Vec<PainterSprite>) {
    let first_row_y = rect.y + PAINTER_ROW_PITCH + 0x10;
    let rows = choice_rows(rect.x + 0x3C, first_row_y);
    let mut out = Vec::new();
    for (i, h) in headings.iter().enumerate() {
        out.extend(text_draws_for(
            &font.layout_ascii(h),
            (rect.x, rect.y + i as i32 * PAINTER_ROW_PITCH),
            MENU_TEXT_WHITE,
        ));
    }
    for (row, label) in rows.iter().zip(choices) {
        out.extend(text_draws_for(
            &font.layout_ascii(label),
            row.label_pen,
            MENU_TEXT_WHITE,
        ));
    }
    (out, choice_marker_sprites(&rows, flags))
}

/// Window 6: six stacked labels with a cursor pinned to the bottom-right
/// **corner of the window extent**.
///
/// `FUN_801D6360` is the only painter in this block that reads `+0xE` and
/// `+0x10` at all: the six labels stack from the content origin at the row
/// pitch, and the cursor lands at `(WX + W - 0x10, WY + H - 0xE)`. Every
/// other painter here anchors on the origin alone, so this one is the only
/// one whose cursor moves when a window is resized.
///
/// The screen it belongs to is menu sub-screen `4` (`FUN_801DD1B8`, script
/// `0x801E4BE0`, the only opener of window `0x06`): it opens the window,
/// waits for a confirm / cancel press and routes back to the pause root.
/// That is a **notice panel**, which is why its only cursor is the corner
/// advance hand. `FUN_801DC6B4` selects sub-screen `4` as the menu's
/// *entry* screen when the entry-context byte is `0x0D`
/// (`0x801dc8d0..0x801dc8e4`) - the same kind whose exit confirm is
/// [`two_line_choice_panel_draws_for`]'s window 5.
///
/// Its six labels are **static VAs in the menu overlay's own pool**, loaded
/// by the `lui a0,0x801d` + `addiu` pairs at `0x801d636c..0x801d6448` - a
/// multi-line battle briefing plus a one-byte control string, not content
/// the entry-context record owns. `engine-world-views::pause_screens` carries the
/// six coordinates and reads the text out of the caller's own image.
///
/// PORT: FUN_801D6360
///
/// Live on both hosts: `FieldMenuSession::Notice` is the phase, entered by
/// `open_entry_screen` when the op-`0x49` park's kind byte is `0x0D`, and
/// each host resolves this id off the disc window table
/// (`window/menu_draws.rs::context_locked_screen_draws`,
/// `web-play/src/play_menu.rs::build_context_locked`).
pub fn label_list_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    labels: &[&str],
) -> (Vec<TextDraw>, PainterSprite) {
    let mut out = Vec::new();
    for (i, label) in labels.iter().enumerate() {
        out.extend(text_draws_for(
            &font.layout_ascii(label),
            (rect.x, rect.y + i as i32 * PAINTER_ROW_PITCH),
            MENU_TEXT_WHITE,
        ));
    }
    (
        out,
        PainterSprite {
            sprite: 1,
            variant: 1,
            x: rect.x + rect.w - 0x10,
            y: rect.y + rect.h - PAINTER_ROW_PITCH,
        },
    )
}
