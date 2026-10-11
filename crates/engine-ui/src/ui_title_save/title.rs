use crate::*;

/// Build [`TextDraw`]s for the title screen.
///
/// Phase argument controls which UI is rendered:
/// - `phase` = 0: fade-in (no text - engines fade the screen to black);
/// - `phase` = 1: "Press START" prompt (centered roughly mid-screen);
/// - `phase` = 2: main menu (New Game / Continue / Options stacked).
///
/// `cursor` is ignored for phases 0/1 and selects the highlighted row
/// (0..=2) in phase 2. `continue_enabled = false` dims the Continue row.
/// `blink_on` toggles the prompt visibility on phase 1 every blink_period
/// frames; engines drive this from the title session's blink phase.
///
/// When the engine has uploaded the PROT 0888 title TIM atlas, pass
/// `atlas_present = true` to suppress the font-rendered "PRESS START"
/// prompt (phase 1) - the TIM's own "PRESS START BUTTON" band is drawn
/// in its place by the sprite layer. The menu rows (phase 2) are
/// still rendered via font because retail uses larger font glyphs
/// there too, not the tiny "NEW GAME CONTINUE" band at the bottom of
/// the TIM.
///
/// A natural anchor for a 320×240 surface is `pen = (96, 100)` - the
/// renderer offsets each line from this top-left.
pub fn title_draws_for(
    font: &legaia_font::Font,
    phase: u8,
    cursor: u8,
    continue_enabled: bool,
    blink_on: bool,
    atlas_present: bool,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 16;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let dim: [f32; 4] = [0.45, 0.45, 0.45, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];

    let mut out = Vec::new();

    match phase {
        0 => {}
        1 if blink_on && !atlas_present => {
            let l = font.layout_ascii("PRESS START");
            out.extend(text_draws_for(&l, pen, white));
        }
        1 => {}
        2 => {
            // Retail title menu carries only two rows; Options lives in
            // the in-game field menu. Color is the selection indicator
            // (selected = white, unselected = dim) - no arrow / cursor
            // mark in retail. The disabled-Continue row reads the same
            // as a non-highlighted row.
            let _ = (gold, continue_enabled);
            let rows = ["NEW GAME", "CONTINUE"];
            for (i, label) in rows.iter().enumerate() {
                let row_y = pen.1 + i as i32 * LINE_H;
                let selected = i as u8 == cursor;
                let color = if selected { white } else { dim };
                let l = font.layout_ascii(label);
                out.extend(text_draws_for(&l, (pen.0, row_y), color));
            }
        }
        _ => {}
    }
    out
}

/// Build [`SpriteDraw`]s for the title-screen main-menu rows ("NEW GAME"
/// / "CONTINUE") sampling the dedicated menu-glyph atlas from
/// `PROT.DAT` (see [`legaia_asset::menu_glyph_atlas`]).
///
/// Retail-faithful equivalent of phase 2 in [`title_draws_for`] - same
/// row labels and cursor / dim semantics, but each row is a horizontal
/// strip of sprite cells sampled from the menu-glyph atlas instead of
/// dialog-font glyphs. Selected row gets a gold tint; the Continue
/// row is dimmed when `continue_enabled = false`. Retail's title menu
/// only carries two rows (NEW GAME / CONTINUE); Options is reached via
/// the in-game field menu, not from the title.
///
/// `cell_scale` is an integer multiplier applied to source-pixel sizes
/// so engines can match the title-art's `play-window` stage scale
/// (mirrors the per-band SpriteDraw scaling). `pen` is the top-left
/// corner of the first row's first glyph in surface pixels.
///
/// Note: the menu-glyph atlas carries only uppercase letters and
/// digits - no cursor marks.
///
/// Returns an empty vec for any phase other than 2.
pub fn title_menu_draws_for(
    phase: u8,
    cursor: u8,
    continue_enabled: bool,
    pen: (i32, i32),
    cell_scale: u32,
) -> Vec<SpriteDraw> {
    if phase != 2 {
        return Vec::new();
    }
    // Retail uses color as the SELECTION INDICATOR: the highlighted row
    // is bright white and unselected rows are dim gray. There is no
    // arrow / cursor mark - the brightness IS the cursor. Disabled
    // (Continue with no save) reads the same as a non-highlighted row.
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];

    use legaia_asset::menu_glyph_atlas as mga;
    let cell_w = mga::GLYPH_W as i32;
    let cell_h = mga::ALPHABET_GLYPH_H as i32;
    let scale = cell_scale.max(1) as i32;
    // One blank row of padding between rows so the small-caps glyphs
    // sit clearly apart (matches the retail menu vertical pitch).
    let line_h = cell_h + 2;

    let rows = ["NEW GAME", "CONTINUE"];
    let mut out = Vec::new();
    for (i, label) in rows.iter().enumerate() {
        let row_y = pen.1 + i as i32 * line_h * scale;
        let selected = i as u8 == cursor;
        let row_disabled = i == 1 && !continue_enabled;
        let _ = row_disabled; // disabled rows render the same as unselected
        let color = if selected { white } else { dim };
        let mut x = pen.0;
        for c in label.chars() {
            if let Some((sx, sy, sw, sh)) = mga::glyph_rect(c) {
                out.push(SpriteDraw {
                    dst: (x, row_y, sw * scale as u32, sh * scale as u32),
                    src: (sx, sy, sw, sh),
                    color,
                });
            }
            x += cell_w * scale;
        }
    }
    out
}
