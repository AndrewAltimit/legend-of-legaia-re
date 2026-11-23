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
//! (`FUN_8002B994(1, 1, x + w - 0x10, y - 2)`): kind `1` is the dialogue
//! pager's two-frame page mark, animated by the shared cursor primitive
//! state (`engine-core` steps it and hands the frame here). The art-learned
//! notice's painter `FUN_801E733C` draws the same mark at its lower-right
//! rim.
//!
//! REF: FUN_801F1B64, FUN_801E733C, FUN_80036888, FUN_8002B994

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

/// The painter's page mark: the strip cell `icon`
/// (`ATLAS_RECT_ADVANCE_ICON[frame]`) at the stage position `pos` the
/// painter passes `FUN_8002B994`, drawn untinted (the primitive's colour word
/// is the neutral `0x808080`).
pub fn notice_mark_sprite(
    icon: (u32, u32, u32, u32),
    pos: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> SpriteDraw {
    let scale = stage_scale.max(1) as i32;
    let (_, _, w, h) = icon;
    SpriteDraw {
        dst: (
            stage_origin.0 + pos.0 * scale,
            stage_origin.1 + pos.1 * scale,
            w * stage_scale,
            h * stage_scale,
        ),
        src: icon,
        color: [1.0, 1.0, 1.0, 1.0],
    }
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
