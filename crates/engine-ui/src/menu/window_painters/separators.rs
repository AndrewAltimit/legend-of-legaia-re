//! The separator glyph and the right-aligned digit run the painters share.

use super::*;

// --- Separator glyph (`FUN_8003C1F8`) ---------------------------------
//
// `FUN_8003C1F8(glyph, x, y)` draws one symbol from the fixed-cell glyph
// page. Two independent uses pin the same id space: the records screen
// (`crate::menu::screens::records_screen`, whose fields are `:` / `/` / `.`) and
// window 37, which calls it with glyph `6` between its quantity and held
// counts (`li a0,0x6; addiu a1,s5,0x20` at `0x801D59D4`) - a `/` between
// "how many" and "how many you have", which is what that row reads as.

/// Separator glyph `4`: the stat-compare **rise** arrow (up).
pub const SEPARATOR_GLYPH_RISE: u8 = 4;
/// Separator glyph `5`: the stat-compare **fall** arrow (down).
pub const SEPARATOR_GLYPH_FALL: u8 = 5;
/// Separator glyph `6`: `/`.
pub const SEPARATOR_GLYPH_SLASH: u8 = 6;
/// Separator glyph `9`: `:`.
pub const SEPARATOR_GLYPH_COLON: u8 = 9;
/// Separator glyph `0xD`: `.`.
pub const SEPARATOR_GLYPH_DOT: u8 = 0x0D;

/// The character a separator-glyph id draws, or `None` for an id outside
/// the three the corpus pins as text. The arrows (`4` / `5`) have no font
/// character: they draw only off the glyph page
/// ([`legaia_font::sprite_cell_ids::SEPARATOR`]).
pub fn separator_glyph_char(glyph: u8) -> Option<char> {
    match glyph {
        SEPARATOR_GLYPH_SLASH => Some('/'),
        SEPARATOR_GLYPH_COLON => Some(':'),
        SEPARATOR_GLYPH_DOT => Some('.'),
        _ => None,
    }
}

/// Draw one separator glyph at `pen`: the 8x12 cell off the menu-glyph
/// page when the font carries it (`uv = (0x50 + glyph * 8, 0xD0)`, tinted
/// by the staged ink the way retail adds the ink to the CLUT word at
/// `0x8003C260`), else the font character for the three text glyphs.
///
/// `pub(crate)` on purpose: this is a retail *primitive*, not a screen, and
/// the UI host-drift gate classifies every `pub fn` returning draws as a
/// screen builder. Keeping it crate-internal states what it is - the thing
/// screens call - and keeps the gate's surface a list of screens.
///
/// PORT: FUN_8003c1f8
pub(crate) fn separator_glyph_draws(
    font: &legaia_font::Font,
    glyph: u8,
    pen: (i32, i32),
    color: [f32; 4],
) -> Vec<TextDraw> {
    if let Some(c) = font.sprite_cell(legaia_font::sprite_cell_ids::SEPARATOR + u16::from(glyph)) {
        return vec![TextDraw {
            dst: (pen.0, pen.1, c.w, c.h),
            src: (c.atlas_x, c.atlas_y, c.w, c.h),
            color: crate::numeral_cell_tint(color),
        }];
    }
    match separator_glyph_char(glyph) {
        Some(ch) => separator_glyph_char_draws(font, ch, pen, color),
        None => Vec::new(),
    }
}

/// [`separator_glyph_draws`] for a caller that already resolved the glyph to
/// a character.
pub(crate) fn separator_glyph_char_draws(
    font: &legaia_font::Font,
    ch: char,
    pen: (i32, i32),
    color: [f32; 4],
) -> Vec<TextDraw> {
    text_draws_for(&font.layout_ascii(&ch.to_string()), pen, color)
}

/// Right-align `value` into a `digits`-wide fixed cell field.
///
/// The retail number writer (`FUN_80034B78`) takes the field width as its
/// second argument and fills cells from the right, so a short value leaves
/// the leading cells blank rather than shifting left.
pub(super) fn digits_draws(
    font: &legaia_font::Font,
    value: u64,
    x: i32,
    y: i32,
    digits: i32,
    color: [f32; 4],
) -> Vec<TextDraw> {
    let s = value.to_string();
    let len = s.len() as i32;
    let mut out = Vec::new();
    for (i, ch) in s.chars().enumerate() {
        let cell = (digits - len + i as i32).max(0);
        out.extend(crate::numeral_cell_draws(
            font,
            ch,
            (x + cell * NUM_CELL_W, y),
            color,
        ));
    }
    out
}
