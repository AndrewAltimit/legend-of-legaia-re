//! The `4C E1` **text balloon** - draw half of the field-VM's screen-anchored
//! one-caption presenter, shared by both hosts.
//!
//! The record, its tick and its geometry live in `engine-core`
//! ([`legaia_engine_core::text_balloon`], ported from `FUN_8003C764` +
//! `FUN_801DA7F0`); this module is what puts it on screen. Nothing drew it
//! before, which is also why the record's `x` stayed `None`: retail measures
//! the line at spawn (`FUN_80035F04`) and the engine's font atlas is
//! host-side, so the measurement has to come back from here.
//!
//! As with [`crate::battle_tutorial_box`], the retail literals stay in
//! `engine-core` and arrive as arguments - this crate does not depend on the
//! simulation, and a second copy of `0x58` / `0x90` / `0x140` on this side is
//! exactly the paired-constant drift the host-drift gate exists to catch.
//!
//! ## What retail draws
//!
//! The handler's running arm is three calls, in this order
//! (`0x801DA8F8..0x801DA930`):
//!
//! ```text
//! FUN_80034B6C(3)                        ; stage widget kind 3
//! FUN_8002C69C(0x58, y, 0x90, 0xB)       ; the balloon frame rect
//! FUN_80036888(text, 0, 0, x, y)         ; the line, at the centred x
//! ```
//!
//! So the frame is a fixed-width window whose only moving part is `y`, while
//! the *text* is centred on the full 320-px screen, not on the frame. A line
//! wider than `0x90` therefore overhangs its own frame in retail, which is why
//! the pen and the rect are two arguments here rather than one.
//!
//! ## Coordinates
//!
//! Everything is **320x240 stage pixels**, the space the retail constants are
//! written in; hosts run the result through their stage transform, as for
//! every other builder in this crate.
//!
//! REF: FUN_801DA7F0, FUN_8003C764, FUN_80035F04, FUN_8002C69C, FUN_80036888

use crate::*;

/// Measure a balloon line the way `FUN_80035F04` does for the spawner: the
/// rendered advance of the raw page bytes in the host's dialog font.
///
/// Hosts feed the result to
/// `legaia_engine_core::world::World::commit_text_balloon_width`, which
/// applies the retail centring and leaves the record carrying the same `x`
/// retail computes at spawn.
pub fn text_balloon_text_width(font: &legaia_font::Font, text: &[u8]) -> i16 {
    font.layout(text).advance_x.min(i16::MAX as u32) as i16
}

/// Window chrome for the balloon: the same gradient fill + gold 9-slice the
/// dialog reading box uses, at the engine's `frame_rect`
/// (`TextBalloon::frame_rect`, i.e. `FUN_8002C69C`'s four arguments).
pub fn text_balloon_chrome_draws_for(
    rects: &SaveMenuAtlasRects,
    frame_rect: (i32, i32, i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    dialog_window_chrome_draws_for(rects, frame_rect, stage_origin, stage_scale)
}

/// The balloon's single line at `pen` (`TextBalloon::pen`), in the staged
/// menu white. Empty for an empty line.
pub fn text_balloon_text_draws_for(
    font: &legaia_font::Font,
    text: &[u8],
    pen: (i32, i32),
) -> Vec<TextDraw> {
    if text.is_empty() {
        return Vec::new();
    }
    text_draws_for(&font.layout(text), pen, MENU_TEXT_WHITE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> legaia_font::Font {
        legaia_font::Font::placeholder()
    }

    #[test]
    fn width_is_the_fonts_own_advance() {
        let f = font();
        assert_eq!(
            text_balloon_text_width(&f, b"abcd"),
            f.layout(b"abcd").advance_x as i16
        );
        assert_eq!(text_balloon_text_width(&f, b""), 0);
    }

    #[test]
    fn a_longer_line_measures_wider() {
        let f = font();
        assert!(text_balloon_text_width(&f, b"iiiiiiii") > text_balloon_text_width(&f, b"i"));
    }

    #[test]
    fn glyphs_start_at_the_pen_and_an_empty_line_draws_nothing() {
        let f = font();
        let draws = text_balloon_text_draws_for(&f, b"ab", (40, 180));
        assert!(!draws.is_empty());
        assert_eq!(draws[0].dst.1, 180);
        assert!(draws[0].dst.0 >= 40);
        assert!(text_balloon_text_draws_for(&f, b"", (40, 180)).is_empty());
    }

    // The chrome builder is a pure delegation to
    // `dialog_window_chrome_draws_for`, whose 9-slice arithmetic needs real
    // atlas rects (a default-constructed set divides by a zero border). It is
    // covered end-to-end against the disc atlas by
    // `legaia-web-viewer`'s `text_balloon_reaches_the_page_draw_channel`.
}
