//! The **sparring-tutorial prompt box** - the in-battle "how to fight"
//! window of the scripted Tetsu fight, shared by both hosts.
//!
//! The placement law lives in `engine-core`
//! (`battle_tutorial::BoxStyle`, ported from the emitter `FUN_801F747C`);
//! this module is the draw half: measure the prompt in the host's font, ask
//! the engine for the box rect, then emit the window skin and the text rows.
//!
//! ## Why it is a framed window
//!
//! `FUN_801F747C` does not print loose text. It measures the prompt
//! (`FUN_8003CBA8` = line count, `FUN_80035F04` = pixel width) and registers a
//! **text actor** with an explicit rect:
//! `FUN_8003541C(1 + waits, 0xD, str, x, y, width, lines*14 - 4, 0x44 - waits)`
//! (`a3 = x`, `sp+0x10 = y`, `sp+0x14 = w`, `sp+0x18 = h`). A rect the emitter
//! goes out of its way to measure is a window, and the retail frame is the
//! same gold double-line 9-slice + blue gradient skin the dialog reading box
//! uses - so this module frames the prompt with
//! [`crate::dialog_window_chrome_draws_for`] at the engine's rect, which
//! inflates the centre rect by 8 px on every side exactly as the reading box
//! does.
//!
//! That skin/rect pairing is what a retail capture of the drill prompt shows:
//! style `0` puts the centre rect at `(0x10, 0x0E, w, 2*14 - 4)`, so the drawn
//! footprint is `x 8 .. 24 + w`, `40` tall, with the text rows at the centre
//! rect's origin on the 14-px pitch. All four of those hold in the capture.
//!
//! ## Coordinates
//!
//! Everything here is **320x240 stage pixels**, the space the retail
//! constants are written in. Both hosts run the result through their stage
//! transform (`scale_stage_text_draws` / the chrome builders' `stage_origin` +
//! `stage_scale` arguments), so the box keeps its retail position at any
//! surface size. Emitting these at raw surface coordinates puts the box in the
//! screen's top-left corner at anything but a 1x 320x240 window.
//!
//! REF: FUN_801F747C, FUN_8003541C, FUN_8003CBA8, FUN_80035F04

use crate::*;

/// Row pitch of the prompt's text lines, in stage pixels. The emitter's own
/// height arithmetic (`lines * 14 - 4`) is built on it.
pub const TUTORIAL_ROW_PITCH: i32 = 14;

/// Measure a prompt's pixel width the way `FUN_80035F04` does for the
/// emitter: the widest rendered line, in the host's dialog font.
pub fn battle_tutorial_text_width(font: &legaia_font::Font, text: &str) -> i16 {
    text.lines()
        .map(|l| font.layout_ascii(l).advance_x as i16)
        .max()
        .unwrap_or(0)
}

/// Text rows of a tutorial prompt, in **stage pixels**.
///
/// Retail draws each line at the box rect's origin on the
/// [`TUTORIAL_ROW_PITCH`] - the same "pen = box origin" relation the dialog
/// reading box uses - in the staged CLUT-7 menu white.
pub fn battle_tutorial_text_draws_for(
    font: &legaia_font::Font,
    text: &str,
    rect: (i32, i32, i32, i32),
) -> Vec<TextDraw> {
    let (x, y, _, _) = rect;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let layout = font.layout_ascii(line);
        out.extend(text_draws_for(
            &layout,
            (x, y + i as i32 * TUTORIAL_ROW_PITCH),
            MENU_TEXT_WHITE,
        ));
    }
    out
}

/// Window chrome for a tutorial prompt: the gradient fill + gold 9-slice
/// frame at the engine's centre `rect`, plus the page-advance hand on a box
/// that waits for acknowledgement.
///
/// `rect` is in stage pixels; `stage_origin` / `stage_scale` are the host's
/// stage transform, as for every other chrome builder.
///
/// The advance hand is a **port affordance**, not a traced one: retail's
/// waiting styles register a different text actor (slot `2`, priority `0x43`)
/// rather than adding a marker sprite, and what that actor draws to signal the
/// wait is not decoded. The hand is the engine's existing "press confirm" cue
/// from the dialog pager, reused so a waiting box is distinguishable from one
/// that dismisses itself.
pub fn battle_tutorial_chrome_draws_for(
    rects: &SaveMenuAtlasRects,
    rect: (i32, i32, i32, i32),
    waits_for_input: bool,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut out = dialog_window_chrome_draws_for(rects, rect, stage_origin, stage_scale);
    if waits_for_input {
        out.push(dialog_advance_hand_sprite(
            rects,
            rect,
            stage_origin,
            stage_scale,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retail draws each prompt row at the box rect's origin on the 14-px
    /// pitch - the pen is the rect corner exactly, no interior inset.
    #[test]
    fn rows_start_at_the_rect_origin_on_the_retail_pitch() {
        let font = legaia_font::Font::placeholder();
        let rect = (0x10, 0x0E, 177, 24);
        let draws = battle_tutorial_text_draws_for(&font, "ab\ncd", rect);
        assert!(!draws.is_empty());
        let row0 = draws.iter().map(|d| d.dst.1).min().unwrap();
        let row1 = draws.iter().map(|d| d.dst.1).max().unwrap();
        // Both rows are inside their cell, and the second sits one pitch down.
        assert!((rect.1..rect.1 + 14).contains(&row0), "row 0 at {row0}");
        assert!(
            (rect.1 + TUTORIAL_ROW_PITCH..rect.1 + 2 * TUTORIAL_ROW_PITCH).contains(&row1),
            "row 1 at {row1}"
        );
        assert!(draws.iter().all(|d| d.dst.0 >= rect.0));
    }

    /// The measured width is the widest line, which is what the emitter's
    /// centring arms divide in half.
    #[test]
    fn width_is_the_widest_line() {
        let font = legaia_font::Font::placeholder();
        let wide = battle_tutorial_text_width(&font, "aaaaaaaa");
        let mixed = battle_tutorial_text_width(&font, "a\naaaaaaaa\naa");
        assert_eq!(wide, mixed);
        assert!(wide > 0);
    }
}
