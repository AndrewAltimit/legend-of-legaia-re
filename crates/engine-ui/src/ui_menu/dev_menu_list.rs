//! World-map dev-menu scrolling list-body renderer - the layout skeleton of
//! `FUN_801EAD98`. The row model, the CLOSED gating, the fixed-width decimal
//! formatter and the camera-word decode are ported renderer-free in
//! `legaia_engine_vm::world_map_overlay` (`DevMenuRow`, `format_fixed_decimal`,
//! `decode_camera_readout`); this module ports the *draw* half: the vertical
//! list geometry every one of the 24 switch arms funnels through.
//!
//! `FUN_801EAD98(param_1 = ctx, param_2 = x, param_3 = y, param_4 = first row,
//! param_5 = last row)` walks `local_40 = param_4 ..= param_5`, breaks at the
//! `0x17` bound, draws each row's label string via `FUN_8001AA68` at
//! `(param_2 + 8, y)` and steps `param_3 += 0x80000` (`>> 0x10` = **8 px**)
//! per row. Each numeric arm additionally draws its readout at a per-row value
//! column (`param_2 + {0x30, 0x38, 0x50, 0x58, 0x68, 0x70, 0x78, 0xa0, ...}`),
//! formatted by the inlined digit kernel. The cursor row is the one where
//! `local_40 == *(short *)(param_1 + 0x9e)`; the cursor arrow sprite itself is
//! drawn by the list-picker `FUN_801ECA08` (at panel `x + 4`), not here.
//!
//! Because the per-arm value column and readout source differ per row, the
//! caller supplies each visible row already reduced to `(label, optional
//! (value_text, value_col))`. What this module owns is the load-base
//! independent geometry: the `+8` label column, the 8-px row pitch, the
//! `0x17` clamp and the first-row `y` origin.
//!
//! PORT: FUN_801EAD98 - dev-menu list-body renderer (row geometry)
//!
//! Source: `ghidra/scripts/funcs/overlay_world_map_801ead98.txt`.

use crate::{MENU_TEXT_WHITE, TextDraw, text_draws_for};

/// Row-to-row vertical pitch (`param_3 += 0x80000`, `>> 0x10` = 8).
pub const DEV_MENU_ROW_STEP: i32 = 8;
/// Label column offset from the x origin (`local_38 = (param_2 + 8) << 16`).
pub const DEV_MENU_LABEL_COL: i32 = 8;
/// Highest valid row index (`if (0x17 < local_40) break`).
pub const DEV_MENU_MAX_ROW: i32 = 0x17;
/// Cursor-arrow x offset from the panel origin (`desc[+0x08]` read back as
/// `x + 4` in `FUN_801ECA08`).
pub const DEV_MENU_CURSOR_COL: i32 = 4;

/// One visible dev-menu row reduced to its draw inputs.
#[derive(Debug, Clone, Copy)]
pub struct DevMenuListRow<'a> {
    /// The row label (`MAP_CHANGE`, `CAMERA`, `ENCOUNT`, ... or `CLOSED`).
    pub label: &'a str,
    /// Formatted numeric readout and its x column offset from the origin, for
    /// the rows that show a live value. `None` for label-only rows.
    pub value: Option<(&'a str, i32)>,
}

/// Screen y of the row at absolute index `row` given a list scrolled so
/// `scroll_start` sits at the pen origin.
///
/// PORT: FUN_801EAD98 (`param_3` step)
pub fn dev_menu_row_y(pen_y: i32, row: i32, scroll_start: i32) -> i32 {
    pen_y + (row - scroll_start) * DEV_MENU_ROW_STEP
}

/// Cursor-arrow position for `cursor_row`, mirroring the picker's `x + 4`
/// column and this renderer's 8-px pitch. Returns `None` when the cursor is
/// scrolled out of the visible `scroll_start ..= scroll_end` window.
pub fn dev_menu_cursor_xy(
    pen: (i32, i32),
    cursor_row: i32,
    scroll_start: i32,
    scroll_end: i32,
) -> Option<(i32, i32)> {
    if cursor_row < scroll_start || cursor_row > scroll_end || cursor_row > DEV_MENU_MAX_ROW {
        return None;
    }
    Some((
        pen.0 + DEV_MENU_CURSOR_COL,
        dev_menu_row_y(pen.1, cursor_row, scroll_start),
    ))
}

/// Build the [`TextDraw`] list for the visible rows `scroll_start ..=
/// scroll_end`, indexing `rows` by absolute row index. Rows past
/// [`DEV_MENU_MAX_ROW`] are dropped (the retail `break`). Labels draw at
/// `x + 8`; each row's optional readout draws at its caller-supplied column.
///
/// PORT: FUN_801EAD98 (row loop)
pub fn dev_menu_list_draws_for(
    font: &legaia_font::Font,
    rows: &[DevMenuListRow<'_>],
    scroll_start: i32,
    scroll_end: i32,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    let ink = MENU_TEXT_WHITE;
    let end = scroll_end.min(DEV_MENU_MAX_ROW);
    for row in scroll_start..=end {
        if row < 0 {
            continue;
        }
        let Some(r) = rows.get(row as usize) else {
            continue;
        };
        let y = dev_menu_row_y(pen.1, row, scroll_start);
        out.extend(text_draws_for(
            &font.layout_ascii(r.label),
            (pen.0 + DEV_MENU_LABEL_COL, y),
            ink,
        ));
        if let Some((value, col)) = r.value {
            out.extend(text_draws_for(
                &font.layout_ascii(value),
                (pen.0 + col, y),
                ink,
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<DevMenuListRow<'static>> {
        vec![
            DevMenuListRow {
                label: "MAP_CHANGE",
                value: None,
            },
            DevMenuListRow {
                label: "CARD_OPTION",
                value: None,
            },
            DevMenuListRow {
                label: "PLAYER_STATUS",
                value: None,
            },
            DevMenuListRow {
                label: "CAMERA",
                value: Some(("000 000", 0x58)),
            },
            DevMenuListRow {
                label: "ENCOUNT",
                value: Some(("15", 0x50)),
            },
        ]
    }

    #[test]
    fn row_y_steps_by_eight() {
        assert_eq!(dev_menu_row_y(100, 0, 0), 100);
        assert_eq!(dev_menu_row_y(100, 3, 0), 100 + 3 * 8);
        // scrolled: row 5 is the first visible row -> sits at the origin.
        assert_eq!(dev_menu_row_y(100, 5, 5), 100);
        assert_eq!(dev_menu_row_y(100, 7, 5), 100 + 2 * 8);
    }

    #[test]
    fn cursor_hidden_when_out_of_window() {
        let pen = (20, 40);
        assert_eq!(dev_menu_cursor_xy(pen, 3, 0, 4), Some((24, 40 + 3 * 8)));
        // above the window.
        assert_eq!(dev_menu_cursor_xy(pen, 0, 2, 6), None);
        // below the window.
        assert_eq!(dev_menu_cursor_xy(pen, 7, 0, 4), None);
        // past the 0x17 clamp.
        assert_eq!(dev_menu_cursor_xy(pen, 0x18, 0, 0x20), None);
    }

    #[test]
    fn draws_visible_window_only() {
        let font = legaia_font::Font::placeholder();
        let r = rows();
        // Draw rows 1..=3: three labels; two of them (CAMERA) carry a value.
        let d = dev_menu_list_draws_for(&font, &r, 1, 3, (0, 100));
        assert!(!d.is_empty());
        // First drawn label is at the origin y (row 1 = scroll_start).
        assert_eq!(d[0].dst.1, 100);
        // Label column is +8.
        assert_eq!(d[0].dst.0, 8);
    }

    #[test]
    fn clamps_end_to_max_row() {
        let font = legaia_font::Font::placeholder();
        // 30 blank rows, ask for scroll_end well past 0x17.
        let many = vec![
            DevMenuListRow {
                label: "X",
                value: None
            };
            30
        ];
        let d_clamped = dev_menu_list_draws_for(&font, &many, 0, 0x40, (0, 0));
        let d_exact = dev_menu_list_draws_for(&font, &many, 0, DEV_MENU_MAX_ROW, (0, 0));
        // Rows past 0x17 are dropped, so both draw the same set.
        assert_eq!(d_clamped.len(), d_exact.len());
    }

    #[test]
    fn value_column_is_caller_supplied() {
        let font = legaia_font::Font::placeholder();
        let r = rows();
        // Draw only the CAMERA row (index 3); its value sits at x+0x58.
        let d = dev_menu_list_draws_for(&font, &r, 3, 3, (0, 0));
        // Some quad must start at x = 0x58 (the value column).
        assert!(d.iter().any(|q| q.dst.0 == 0x58));
    }
}
