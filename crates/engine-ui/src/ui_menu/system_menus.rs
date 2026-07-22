use crate::*;

/// Build [`TextDraw`]s for the game-over panel.
pub fn game_over_draws_for(
    font: &legaia_font::Font,
    cursor: u8,
    continue_enabled: bool,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 16;
    let red: [f32; 4] = [1.0, 0.4, 0.4, 1.0];
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let dim: [f32; 4] = [0.4, 0.4, 0.4, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];

    let mut out = Vec::new();
    out.extend(text_draws_for(&font.layout_ascii("GAME OVER"), pen, red));

    let rows = ["Continue", "Retry", "Quit"];
    for (i, row) in rows.iter().enumerate() {
        let y = pen.1 + LINE_H * 2 + i as i32 * LINE_H;
        let row_disabled = i == 0 && !continue_enabled;
        let color = if row_disabled {
            dim
        } else if i as u8 == cursor {
            gold
        } else {
            white
        };
        if i as u8 == cursor && !row_disabled {
            out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
        }
        out.extend(text_draws_for(
            &font.layout_ascii(row),
            (pen.0 + 14, y),
            color,
        ));
    }
    out
}

/// One row in the options panel (mirrors
/// `legaia_engine_core::options::OptionsRowView`).
pub struct OptionsRowView<'a> {
    pub label: &'a str,
    /// Right-column value string; `None` on the "Dual Shock" header row.
    pub value: Option<&'a str>,
    /// Teal label ink (retail ink 5 - the Dual Shock sub-rows). White
    /// (ink 7) otherwise.
    pub teal: bool,
    /// Pixel pitch below this row (retail layout advance: 14, or 20 on
    /// the group-separator rows).
    pub advance: i32,
}

/// The value-choice popup (retail window id 47), when a row is being
/// edited.
pub struct OptionsPopupDraw<'a> {
    /// Popup **content** rect in stage pixels (see
    /// `legaia_engine_core::options::options_popup_content_rect`).
    pub rect: (i32, i32, i32, i32),
    pub choices: &'a [&'a str],
    pub cursor: u8,
}

/// Retail menu ink 7 (labels), sampled from the options-screen VRAM
/// capture.
pub const OPTIONS_INK_WHITE: [f32; 4] = [0.81, 0.81, 0.81, 1.0];
/// Retail menu ink 6 (the gold value column).
pub const OPTIONS_INK_GOLD: [f32; 4] = [0.91, 0.68, 0.0, 1.0];
/// Retail menu ink 5 (the teal Dual Shock sub-row labels).
pub const OPTIONS_INK_TEAL: [f32; 4] = [0.26, 0.87, 0.87, 1.0];

/// Build [`TextDraw`]s for the options screen.
///
/// Retail geometry (row renderer `FUN_801d2910`, hosted by the id-48
/// window renderer `FUN_801dcef0`, content rect `(24,40,256,148)`):
/// cursor arrow at `x-10`, labels at `x+8`, values at `x+140`; per-row
/// pitch from the layout table (14 px, 20 px on separator rows). Labels
/// draw in ink 7 (white) or ink 5 (teal), values in ink 6 (gold). While
/// the value popup is open the retail renderer drops every other row's
/// ink to 0 (rows above the cursor keep the header lit); the engine dims
/// them instead. The popup (`FUN_801d2b44`, window id 47) lists choices
/// at a 13-px pitch, text inset `+0x14`, its cursor at the content
/// origin. No footer hint - retail has none.
// PORT: FUN_801d2910
// PORT: FUN_801d2b44
// PORT: FUN_801dcef0 - the id-48 window renderer is a 9-instruction
// wrapper (`FUN_801d2910(win, 0, 9)`); drawing all ten rows through the
// row renderer *is* its whole body.
pub fn options_draws_for(
    font: &legaia_font::Font,
    rows: &[OptionsRowView<'_>],
    cursor: u8,
    popup: Option<&OptionsPopupDraw<'_>>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    let dim = |c: [f32; 4]| [c[0] * 0.45, c[1] * 0.45, c[2] * 0.45, c[3]];

    let mut out = Vec::new();
    let mut y = pen.1;
    for (i, row) in rows.iter().enumerate() {
        let selected = i as u8 == cursor;
        // Edit mode dims every non-cursor row; the retail exception keeps
        // header rows above the cursor at full ink.
        let dimmed = popup.is_some() && !selected && !(row.value.is_none() && (i as u8) < cursor);
        let label_ink = if row.teal {
            OPTIONS_INK_TEAL
        } else {
            OPTIONS_INK_WHITE
        };
        // The selected row is marked by the pointing-hand sprite at
        // `x-10` (`FUN_8002b994` kind 0, emitted by the sprite chrome
        // pass through `options_hand_cursor_sprite`), not by an ink
        // change or a text glyph.
        out.extend(text_draws_for(
            &font.layout_ascii(row.label),
            (pen.0 + 8, y),
            if dimmed { dim(label_ink) } else { label_ink },
        ));
        if let Some(value) = row.value {
            out.extend(text_draws_for(
                &font.layout_ascii(value),
                (pen.0 + 140, y),
                if dimmed {
                    dim(OPTIONS_INK_GOLD)
                } else {
                    OPTIONS_INK_GOLD
                },
            ));
        }
        y += row.advance;
    }
    if let Some(p) = popup {
        const POPUP_LINE_H: i32 = 13;
        let (px, py, _, _) = p.rect;
        for (i, choice) in p.choices.iter().enumerate() {
            let cy = py + i as i32 * POPUP_LINE_H;
            if i as u8 == p.cursor {
                out.extend(text_draws_for(
                    &font.layout_ascii(">"),
                    (px, cy),
                    OPTIONS_INK_WHITE,
                ));
            }
            out.extend(text_draws_for(
                &font.layout_ascii(choice),
                (px + 0x14, cy),
                OPTIONS_INK_WHITE,
            ));
        }
    }
    out
}

/// The options settings window's selected-row marker: the 16x16
/// pointing-hand sprite at `(x-10, row_y)`. Retail marks the cursor row
/// through the shared animated-cursor primitive `FUN_8002b994` kind 0 -
/// the same hand as the status party list, 18 px left of the label ink
/// in both windows (party list: name `WX+6`, hand `WX-0xc`; options:
/// label `x+8`, cursor anchor `x-10`). `row_y_off` is the running sum of
/// the row advances above the cursor row (the same walk
/// [`options_draws_for`] does).
///
/// REF: FUN_801D2910 - the options row renderer.
pub fn options_hand_cursor_sprite(
    rects: &SaveMenuAtlasRects,
    pen: (i32, i32),
    row_y_off: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> SpriteDraw {
    let scale = stage_scale.max(1) as i32;
    let (_, _, w, h) = rects.cursor;
    SpriteDraw {
        dst: (
            stage_origin.0 + (pen.0 - 10) * scale,
            stage_origin.1 + (pen.1 + row_y_off) * scale,
            w * stage_scale,
            h * stage_scale,
        ),
        src: rects.cursor,
        color: [1.0, 1.0, 1.0, 1.0],
    }
}

/// Build [`TextDraw`]s for the key-rebind panel. Each row shows a button
/// label paired with the currently-bound key string.
pub fn key_rebind_draws_for(
    font: &legaia_font::Font,
    rows: &[(&str, &str)],
    cursor: u8,
    awaiting: bool,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.6, 0.6, 0.6, 1.0];

    let mut out = Vec::new();
    out.extend(text_draws_for(&font.layout_ascii("KEY REBIND"), pen, gold));

    for (i, (button, key)) in rows.iter().enumerate() {
        let y = pen.1 + LINE_H * 2 + i as i32 * LINE_H;
        let selected = i as u8 == cursor;
        let color = if selected { gold } else { white };
        if selected {
            out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
        }
        out.extend(text_draws_for(
            &font.layout_ascii(button),
            (pen.0 + 14, y),
            color,
        ));
        let value = if selected && awaiting { "..." } else { *key };
        out.extend(text_draws_for(
            &font.layout_ascii(value),
            (pen.0 + 100, y),
            color,
        ));
    }
    out.extend(text_draws_for(
        &font.layout_ascii("Cross: Bind  Circle: Cancel  Start: Save"),
        (
            pen.0,
            pen.1 + LINE_H * 2 + rows.len() as i32 * LINE_H + LINE_H,
        ),
        dim,
    ));
    out
}

/// Menu ink 5 - the teal the confirm-prompt option rows stage
/// (`DAT_8007B454 = 5` before each option string).
pub const CONFIRM_OPTION_INK: [f32; 4] = OPTIONS_INK_TEAL;

/// Row pitch of the confirm-prompt windows.
pub const CONFIRM_PROMPT_PITCH_Y: i32 = 0xE;
/// X inset of the option strings (`WX + 0x44`).
pub const CONFIRM_OPTION_X: i32 = 0x44;
/// X inset of the pointing-hand cursor on the focused option row
/// (`WX + 0x30`).
pub const CONFIRM_HAND_X: i32 = 0x30;
/// Second-prompt-line indent of the 3-line variant (`WX + 0xC`).
pub const CONFIRM_PROMPT_LINE2_X: i32 = 0xC;

/// Build [`TextDraw`]s for the pause-menu save-flow Yes/No prompt
/// windows: `prompt` lines from the window origin (line 2, when
/// present, indents [`CONFIRM_PROMPT_LINE2_X`]), then the two option
/// rows at `WX + 0x44` in menu ink 5, each a [`CONFIRM_PROMPT_PITCH_Y`]
/// step down. The pointing-hand cursor belongs on the focused option
/// row at `WX + 0x30` ([`confirm_prompt_hand_pos`]); the retail cursor
/// word (`DAT_801E46D0`) hides it under bit `0x4000`.
///
/// PORT: FUN_801d1dac (1 prompt line + 2 options; see
/// `ghidra/scripts/funcs/overlay_menu_801d1dac.txt`)
/// PORT: FUN_801d1f10 (the 3-line variant - a second, indented prompt
/// line shifts the option block one pitch down)
pub fn confirm_prompt_draws(
    font: &legaia_font::Font,
    prompt: &[&str],
    options: &[&str],
    pen: (i32, i32),
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    let mut y = pen.1;
    for (i, line) in prompt.iter().enumerate() {
        let x = if i == 0 {
            pen.0
        } else {
            pen.0 + CONFIRM_PROMPT_LINE2_X
        };
        out.extend(text_draws_for(
            &font.layout_ascii(line),
            (x, y),
            OPTIONS_INK_WHITE,
        ));
        y += CONFIRM_PROMPT_PITCH_Y;
    }
    for opt in options {
        out.extend(text_draws_for(
            &font.layout_ascii(opt),
            (pen.0 + CONFIRM_OPTION_X, y),
            CONFIRM_OPTION_INK,
        ));
        y += CONFIRM_PROMPT_PITCH_Y;
    }
    out
}

/// Stage position of the hand cursor for option row `cursor` of a
/// [`confirm_prompt_draws`] window with `prompt_lines` prompt rows.
///
/// REF: FUN_801d1dac / FUN_801d1f10 (the `FUN_8002B994(kind 0, blink,
/// WX + 0x30, row_y)` calls)
pub fn confirm_prompt_hand_pos(pen: (i32, i32), prompt_lines: usize, cursor: u8) -> (i32, i32) {
    (
        pen.0 + CONFIRM_HAND_X,
        pen.1 + (prompt_lines as i32 + cursor as i32) * CONFIRM_PROMPT_PITCH_Y,
    )
}

#[cfg(test)]
mod confirm_prompt_tests {
    use super::*;

    /// Top-left of the first glyph of each emitted run, in draw order.
    fn run_origins(draws: &[TextDraw]) -> Vec<(i32, i32)> {
        // Every glyph of a run shares the run's baseline row, and the
        // builder emits runs back to back, so a new row (or a leftward
        // jump) starts a new run.
        let mut out: Vec<(i32, i32)> = Vec::new();
        let mut prev: Option<(i32, i32)> = None;
        for d in draws {
            let (x, y, _, _) = d.dst;
            let starts_run = match prev {
                None => true,
                Some((px, py)) => y != py || x < px,
            };
            if starts_run {
                out.push((x, y));
            }
            prev = Some((x, y));
        }
        out
    }

    /// `FUN_801D1DAC`: prompt at the window pen, then the two option rows
    /// at `WX + 0x44`, one `0xE` pitch apart. The X inset is the thing
    /// that separates this window from its Throw Out sibling
    /// (`FUN_801D1B20`, options at `WX + 0x3C`), so pin it.
    #[test]
    fn one_line_variant_places_options_at_the_traced_inset() {
        let font = legaia_font::synthetic_for_tests();
        let pen = (14, 38);
        let draws = confirm_prompt_draws(&font, &["Do you wish to save?"], &["Yes", "No"], pen);
        let origins = run_origins(&draws);
        assert_eq!(origins.len(), 3, "prompt + two options");
        assert_eq!(origins[0], pen);
        assert_eq!(origins[1], (pen.0 + 0x44, pen.1 + 0xE));
        assert_eq!(origins[2], (pen.0 + 0x44, pen.1 + 0x1C));
    }

    /// `FUN_801D1F10`: a second prompt line indented `WX + 0xC` pushes the
    /// whole option block one pitch down. Its rows then coincide with the
    /// Throw Out confirm's `WY + 0x1C` / `WY + 0x2A`, which is what makes
    /// the two easy to confuse - the inset is the discriminator.
    #[test]
    fn three_line_variant_indents_line_two_and_shifts_the_options() {
        let font = legaia_font::synthetic_for_tests();
        let pen = (14, 38);
        let draws = confirm_prompt_draws(
            &font,
            &["Overwrite the", "saved data?"],
            &["Yes", "No"],
            pen,
        );
        let origins = run_origins(&draws);
        assert_eq!(origins.len(), 4);
        assert_eq!(origins[0], pen);
        assert_eq!(origins[1], (pen.0 + CONFIRM_PROMPT_LINE2_X, pen.1 + 0xE));
        assert_eq!(origins[2], (pen.0 + 0x44, pen.1 + 0x1C));
        assert_eq!(origins[3], (pen.0 + 0x44, pen.1 + 0x2A));
    }

    /// Retail stages `DAT_8007B454 = 5` before each option string and
    /// leaves the prompt on the default white - the option rows are teal,
    /// the prompt is not.
    #[test]
    fn options_take_menu_ink_five_and_the_prompt_does_not() {
        let font = legaia_font::synthetic_for_tests();
        let draws = confirm_prompt_draws(&font, &["Save?"], &["Yes", "No"], (0, 0));
        let prompt_row: Vec<_> = draws.iter().filter(|d| d.dst.1 == 0).collect();
        assert!(!prompt_row.is_empty());
        assert!(prompt_row.iter().all(|d| d.color == OPTIONS_INK_WHITE));
        let option_rows: Vec<_> = draws.iter().filter(|d| d.dst.1 > 0).collect();
        assert!(!option_rows.is_empty());
        assert!(option_rows.iter().all(|d| d.color == CONFIRM_OPTION_INK));
    }

    /// The hand sits `0x14` left of the option strings (`WX + 0x30` vs
    /// `WX + 0x44`) on the focused row, and steps by the same pitch the
    /// rows do.
    #[test]
    fn hand_tracks_the_focused_option_row() {
        let pen = (14, 38);
        assert_eq!(
            confirm_prompt_hand_pos(pen, 1, 0),
            (pen.0 + CONFIRM_HAND_X, pen.1 + CONFIRM_PROMPT_PITCH_Y)
        );
        assert_eq!(
            confirm_prompt_hand_pos(pen, 1, 1),
            (pen.0 + CONFIRM_HAND_X, pen.1 + 2 * CONFIRM_PROMPT_PITCH_Y)
        );
        assert_eq!(
            confirm_prompt_hand_pos(pen, 2, 0),
            (pen.0 + CONFIRM_HAND_X, pen.1 + 2 * CONFIRM_PROMPT_PITCH_Y)
        );
        assert_eq!(CONFIRM_OPTION_X - CONFIRM_HAND_X, 0x14);
    }
}
