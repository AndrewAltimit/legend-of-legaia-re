//! The level-up and Seru-capture banners.

use super::*;

/// Build [`TextDraw`]s for a level-up banner overlay.
///
/// Renders two lines anchored at `pen`:
/// ```text
/// LEVEL UP!  (char_id, new_level)
/// HP +hp_gained  MP +mp_gained
/// ```
/// Designed for a PSX-style 320×240 surface; a typical anchor is around
/// `(8, 60)` to appear near the top of the screen after battle.
pub fn level_up_draws_for(
    font: &legaia_font::Font,
    char_id: u8,
    new_level: u8,
    hp_gained: u16,
    mp_gained: u16,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 16;
    let yellow: [f32; 4] = [1.0, 0.9, 0.2, 1.0];
    let green: [f32; 4] = [0.4, 1.0, 0.4, 1.0];

    let line1 = format!("LEVEL UP! (char {} -> Lv {})", char_id + 1, new_level);
    let line2 = format!("HP +{}  MP +{}", hp_gained, mp_gained);

    let layout1 = font.layout_ascii(&line1);
    let layout2 = font.layout_ascii(&line2);

    let mut out = text_draws_for(&layout1, pen, yellow);
    out.extend(text_draws_for(&layout2, (pen.0, pen.1 + LINE_H), green));
    out
}

/// Build [`TextDraw`]s for the post-battle Seru-capture banner.
///
/// `text` is the single active banner line from
/// `SeruCaptureSession::current_banner` (e.g. `"Captured: Spark!"` or
/// `"Character 1 learned Aqua!"`). Drawn in cyan, the sibling of
/// [`level_up_draws_for`]; a natural anchor near the top of a 320×240 surface
/// is `(8, 40)`.
pub fn capture_banner_draws_for(
    font: &legaia_font::Font,
    text: &str,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    let cyan: [f32; 4] = [0.4, 0.9, 1.0, 1.0];
    let layout = font.layout_ascii(text);
    text_draws_for(&layout, pen, cyan)
}
