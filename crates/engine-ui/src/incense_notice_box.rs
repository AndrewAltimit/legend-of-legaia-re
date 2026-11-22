//! The **Incense wear-off notice** panel - the draw half of the field
//! overlay's `FUN_801F1E48` / `FUN_801F1B64` pair, shared by both hosts.
//!
//! The state machine and the geometry live in `engine-core`
//! (`legaia_engine_core::incense_notice`: `notice_frame_rect`,
//! `notice_text_pen`) and arrive here as arguments, as the tile board
//! prompt's do. The line is the field overlay's own string, read off the
//! disc by the host with its item-name escape expanded; this module lays it
//! out.
//!
//! Retail's painter also draws a marker sprite at the panel's right edge
//! (`FUN_8002B994(1, 1, x + w - 0x10, y - 2)`); which atlas cell kind `1`
//! selects is not pinned, so the panel draws without it.
//!
//! REF: FUN_801F1B64, FUN_80036888

use crate::*;

/// The notice frame's chrome at `frame` (stage pixels): the window's drawn
/// footprint, its content rect grown by 8 on every side.
///
/// The notice is field-overlay window record 16 (and the art-learned notice
/// record 9), whose config stages style `3` (`+0x06`, copied to the live
/// window's `+0x1D` and staged by the window walker before its box), the
/// class-0 frame ([`crate::battle_hud_chrome::class0_box_draws_for`] over
/// the content rect) - not the reading box's gradient, and not a box grown
/// a second time past the footprint *(capture: `field_kor_incense_notice_retail`)*.
///
/// REF: FUN_80031D00 (the walker's style stage), FUN_80032434 (the config copy)
pub fn incense_notice_sprites_for(
    rects: &SaveMenuAtlasRects,
    frame: (i32, i32, i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let (x, y, w, h) = frame;
    crate::battle_hud_chrome::class0_box_draws_for(
        rects,
        (x + 8, y + 8, w - 16, h - 16),
        stage_origin,
        stage_scale,
    )
}

/// The notice line at `pen`, in stage pixels (the host scales it with
/// [`scale_stage_text_draws`]).
pub fn incense_notice_text_draws_for(
    font: &legaia_font::Font,
    pen: (i32, i32),
    line: &[u8],
) -> Vec<TextDraw> {
    if line.is_empty() {
        return Vec::new();
    }
    text_draws_for(&font.layout(line), pen, MENU_TEXT_WHITE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_starts_at_its_pen_and_an_empty_line_draws_nothing() {
        let f = legaia_font::Font::placeholder();
        let draws = incense_notice_text_draws_for(&f, (46, 32), b"ab");
        assert!(!draws.is_empty());
        assert!(draws.iter().all(|d| d.dst.1 == 32));
        assert!(incense_notice_text_draws_for(&f, (46, 32), b"").is_empty());
    }
}
