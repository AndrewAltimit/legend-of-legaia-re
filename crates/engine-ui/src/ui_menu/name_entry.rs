use crate::*;

/// Renderer-agnostic view of the name-entry overlay (engine-core's
/// `name_entry::NameEntry` projected to primitives, so this crate stays
/// decoupled from engine-core). Built per frame by the shell.
pub struct NameEntryView<'a> {
    /// The six selectable glyph rows (`|` marks a non-selectable separator).
    pub grid_rows: &'a [&'a str],
    /// Bottom control-bar button labels, left to right (e.g. Back / Space / End).
    pub control_labels: &'a [&'a str],
    /// Working name buffer.
    pub name: &'a str,
    /// Highlighted glyph cell `(row, col)` when the cursor is in the grid.
    pub grid_cursor: Option<(usize, usize)>,
    /// Highlighted control button index when the cursor is on the bar.
    pub control_cursor: Option<usize>,
    /// `true` while the "Is this name okay?" Yes/No prompt is showing.
    pub confirming: bool,
    /// Yes/No selection during the confirm prompt (`true` = Yes).
    pub confirm_yes: bool,
    /// Caret blink state (the trailing `_` after the name).
    pub caret_on: bool,
}

/// Build [`TextDraw`]s for the name-entry overlay - the screen the opening
/// `town01` field script opens so the player names the lead character.
///
/// Clean-room layout (the retail renderer is `FUN_801E6B34`): a heading +
/// working name with a blinking caret, the 6x17 character grid (separators
/// skipped), a control bar (Back / Space / End), and a Yes/No box while
/// confirming. Grid metrics: 14 px per column, 16 px per row.
///
/// REF: FUN_801E6B34
pub fn name_entry_draws_for(
    font: &legaia_font::Font,
    view: &NameEntryView<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const COL_W: i32 = 14;
    const ROW_H: i32 = 16;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.6, 0.6, 0.6, 1.0];

    let mut out = Vec::new();
    out.extend(text_draws_for(
        &font.layout_ascii("Select your name."),
        pen,
        gold,
    ));

    // Working name + blinking caret.
    let name_y = pen.1 + ROW_H;
    out.extend(text_draws_for(
        &font.layout_ascii(view.name),
        (pen.0, name_y),
        white,
    ));
    if view.caret_on {
        let name_w = font.layout_ascii(view.name).advance_x as i32;
        out.extend(text_draws_for(
            &font.layout_ascii("_"),
            (pen.0 + name_w, name_y),
            white,
        ));
    }

    // Character grid.
    let grid_y0 = pen.1 + ROW_H * 3;
    for (r, row) in view.grid_rows.iter().enumerate() {
        for (c, ch) in row.bytes().enumerate() {
            if ch == b'|' || ch == b' ' {
                continue;
            }
            let selected = view.grid_cursor == Some((r, c));
            let color = if selected { gold } else { white };
            let x = pen.0 + c as i32 * COL_W;
            let y = grid_y0 + r as i32 * ROW_H;
            if selected {
                // Cursor bracket to the left of the highlighted glyph.
                out.extend(text_draws_for(
                    &font.layout_ascii(">"),
                    (x - COL_W / 2, y),
                    gold,
                ));
            }
            out.extend(text_draws_for(
                &font.layout_ascii(&(ch as char).to_string()),
                (x, y),
                color,
            ));
        }
    }

    // Control bar (Back / Space / End).
    let bar_y = grid_y0 + view.grid_rows.len() as i32 * ROW_H + ROW_H / 2;
    let mut bx = pen.0;
    for (i, label) in view.control_labels.iter().enumerate() {
        let selected = view.control_cursor == Some(i);
        let color = if selected { gold } else { white };
        if selected {
            out.extend(text_draws_for(
                &font.layout_ascii(">"),
                (bx - COL_W / 2, bar_y),
                gold,
            ));
        }
        out.extend(text_draws_for(
            &font.layout_ascii(label),
            (bx, bar_y),
            color,
        ));
        bx += font.layout_ascii(label).advance_x as i32 + COL_W;
    }

    // Confirm Yes/No box.
    if view.confirming {
        let cy = bar_y + ROW_H * 2;
        out.extend(text_draws_for(
            &font.layout_ascii("Is this name okay?"),
            (pen.0, cy),
            gold,
        ));
        let yes_color = if view.confirm_yes { gold } else { white };
        let no_color = if view.confirm_yes { white } else { gold };
        out.extend(text_draws_for(
            &font.layout_ascii("Yes"),
            (pen.0, cy + ROW_H),
            yes_color,
        ));
        out.extend(text_draws_for(
            &font.layout_ascii("No"),
            (pen.0 + 64, cy + ROW_H),
            no_color,
        ));
    } else {
        out.extend(text_draws_for(
            &font.layout_ascii("Cross: Select  Triangle: Back"),
            (
                pen.0,
                grid_y0 + view.grid_rows.len() as i32 * ROW_H + ROW_H * 2,
            ),
            dim,
        ));
    }

    out
}

/// Build [`TextDraw`]s for one opening-cutscene narration page (a single
/// subtitle line), horizontally centered at `center_x` with its baseline at
/// `top_y`, both in surface pixels. The host calls this with the active
/// page text from [`legaia_engine_core::cutscene_narration::CutsceneNarration`]
/// each frame; an empty / completed narration draws nothing.
///
/// Centering is computed from the font metrics (`center_x - width / 2`), the
/// same scheme [`now_checking_text_draws_for`] uses for retail-style centered
/// dialog. Subtitles are bottom-anchored by the caller's `top_y`.
pub fn cutscene_narration_draws_for(
    font: &legaia_font::Font,
    text: &str,
    center_x: i32,
    top_y: i32,
    color: [f32; 4],
) -> Vec<TextDraw> {
    if text.is_empty() {
        return Vec::new();
    }
    let layout = font.layout_ascii(text);
    let left_x = center_x - (layout.advance_x as i32 / 2);
    text_draws_for(&layout, (left_x, top_y), color)
}
