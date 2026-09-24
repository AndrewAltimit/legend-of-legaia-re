//! Koru's **timed-fight strip** - the `Turns Left / HP Left` line the battle
//! overlay puts up for the one turn-limited boss fight, shared by both hosts.
//!
//! The gate, the numbers and the strip's lifetime are `engine-core`'s
//! (`timed_fight`); this module is the draw half, in **320x240 stage
//! pixels** like every other battle builder.
//!
//! ## What retail registers
//!
//! The round-start arm of `FUN_801D0748` (state `0x14`) registers one text
//! actor and two number records on it:
//!
//! * `FUN_8003541C(1, 0, 0x801CE818, 0x10, 0x0E, 0x120, 0x0C, 0x44)` - key
//!   `1`, the format string at the head of PROT 0898, the content rect
//!   `(16, 14, 288, 12)`, and the style word `0x44` the non-waiting tutorial
//!   prompts register with (`FUN_801F747C`: `0x44 - waits`). A text actor
//!   with an explicit rect is a window, so this frames the strip with the
//!   same skin the tutorial box wears ([`crate::dialog_window_chrome_draws_for`]
//!   at the centre rect, inflated 8 px on every side);
//! * `FUN_8003563C(1, &turns, 1, 0x68, 0, 1, 7)` and
//!   `FUN_8003563C(1, &hp_pct, 1, 0xD2, 0, 3, 7)` - the two numbers, at x
//!   `0x68` (one digit) and `0xD2` (three) inside the actor, in the same
//!   CLUT-7 white.
//!
//! The format string itself is space-padded where the numbers go; it is read
//! off the user's own PROT 0898 (`legaia_asset::battle_ui_strings`), never
//! carried here.
//!
//! **Not pinned, and chosen here:** how the record renderer pads a number
//! narrower than its field. A three-digit field right-aligns its value
//! ([`number_pen_x`]); no Koru-fight capture exists to read it off.
//!
//! REF: FUN_8003541C, FUN_8003563C

use crate::*;

/// The strip's content rect `(x, y, w, h)` in stage pixels, exactly the
/// registration's `(0x10, 0x0E, 0x120, 0x0C)`.
pub const TIMED_FIGHT_STRIP_RECT: (i32, i32, i32, i32) = (0x10, 0x0E, 0x120, 0x0C);
/// `Turns Left` digit's x offset inside the strip and its digit count.
pub const TIMED_FIGHT_TURNS_LEFT_X: i32 = 0x68;
pub const TIMED_FIGHT_TURNS_LEFT_DIGITS: usize = 1;
/// `HP Left` number's x offset inside the strip and its digit count.
pub const TIMED_FIGHT_HP_LEFT_X: i32 = 0xD2;
pub const TIMED_FIGHT_HP_LEFT_DIGITS: usize = 3;

/// One frame of the strip, projected by the host from
/// `legaia_engine_core::timed_fight::timed_fight_strip`.
#[derive(Debug, Clone, Copy)]
pub struct TimedFightStripView<'a> {
    /// The format string off the disc (the two field labels, space padded).
    pub label: &'a str,
    /// `Turns Left` digit.
    pub turns_left: u32,
    /// `HP Left` percentage.
    pub hp_left: u32,
}

/// Pen x for a number of `text` in a `digits`-wide field whose left edge is
/// `field_x`: right-aligned on the font's `0` advance.
pub fn number_pen_x(font: &legaia_font::Font, field_x: i32, digits: usize, text: &str) -> i32 {
    let cell = font.layout_ascii("0").advance_x as i32;
    let w = font.layout_ascii(text).advance_x as i32;
    field_x + (cell * digits as i32 - w).max(0)
}

/// Keep the low `digits` decimal digits of `value` - a field never grows.
fn field_text(value: u32, digits: usize) -> String {
    let modulus = 10u32.pow(digits as u32);
    (value % modulus).to_string()
}

/// The strip's text in stage pixels: the label at the rect origin and the two
/// numbers at their registered offsets.
pub fn timed_fight_strip_text_draws(
    font: &legaia_font::Font,
    view: &TimedFightStripView<'_>,
) -> Vec<TextDraw> {
    let (x, y, _, _) = TIMED_FIGHT_STRIP_RECT;
    let mut out = text_draws_for(&font.layout_ascii(view.label), (x, y), MENU_TEXT_WHITE);
    for (value, off, digits) in [
        (
            view.turns_left,
            TIMED_FIGHT_TURNS_LEFT_X,
            TIMED_FIGHT_TURNS_LEFT_DIGITS,
        ),
        (
            view.hp_left,
            TIMED_FIGHT_HP_LEFT_X,
            TIMED_FIGHT_HP_LEFT_DIGITS,
        ),
    ] {
        let text = field_text(value, digits);
        let pen = (number_pen_x(font, x + off, digits, &text), y);
        out.extend(text_draws_for(
            &font.layout_ascii(&text),
            pen,
            MENU_TEXT_WHITE,
        ));
    }
    out
}

/// The strip's window skin at the registered rect, through the host's stage
/// transform.
pub fn timed_fight_strip_chrome_draws(
    rects: &SaveMenuAtlasRects,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    dialog_window_chrome_draws_for(rects, TIMED_FIGHT_STRIP_RECT, stage_origin, stage_scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_sit_at_their_registered_offsets_inside_the_rect() {
        let font = legaia_font::Font::placeholder();
        let view = TimedFightStripView {
            label: "   T:     H: ",
            turns_left: 3,
            hp_left: 47,
        };
        let draws = timed_fight_strip_text_draws(&font, &view);
        assert!(!draws.is_empty());
        let (x, y, w, _) = TIMED_FIGHT_STRIP_RECT;
        assert!(draws.iter().all(|d| d.dst.0 >= x && d.dst.0 < x + w));
        assert!(draws.iter().all(|d| (y..y + 14).contains(&d.dst.1)));
        // A glyph starts at or after each number field's left edge.
        assert!(draws.iter().any(|d| d.dst.0 >= x + TIMED_FIGHT_HP_LEFT_X));
    }

    #[test]
    fn a_field_keeps_its_width() {
        assert_eq!(field_text(4, 1), "4");
        assert_eq!(field_text(14, 1), "4");
        assert_eq!(field_text(100, 3), "100");
        assert_eq!(field_text(7, 3), "7");
    }

    #[test]
    fn the_rect_is_the_registration() {
        assert_eq!(TIMED_FIGHT_STRIP_RECT, (16, 14, 288, 12));
        assert_eq!(
            (TIMED_FIGHT_TURNS_LEFT_X, TIMED_FIGHT_HP_LEFT_X),
            (104, 210)
        );
    }
}
