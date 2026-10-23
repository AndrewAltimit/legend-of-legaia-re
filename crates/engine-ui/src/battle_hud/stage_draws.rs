//! Sprite, text and numeral-cell draws placed on the stage, and the stage-scale pass over text.

use super::*;

/// Convert sprite requests to [`SpriteDraw`]s, applying a screen-space
/// `anchor` translation. The output `dst` width/height match the atlas
/// source rect 1:1 (no scaling - engines that want PSX-native 240px
/// vertical scaling should pre-scale `world_y` before calling this).
pub fn sprite_draws_for(requests: &[SpriteRequest], anchor: (i32, i32)) -> Vec<SpriteDraw> {
    requests
        .iter()
        .map(|r| SpriteDraw {
            dst: (
                anchor.0 + r.world_x,
                anchor.1 + r.world_y,
                r.atlas_src.2,
                r.atlas_src.3,
            ),
            src: r.atlas_src,
            color: r.color,
        })
        .collect()
}

/// Convert a [`legaia_font::Layout`] to a vector of [`TextDraw`]s anchored at
/// `pen` with the supplied tint. Glyph atlas coordinates come from the
/// layout; destination coordinates are pen-relative pixels with one quad per
/// glyph. The returned draws are batchable into a single [`TextOverlay`].
pub fn text_draws_for(
    layout: &legaia_font::Layout,
    pen: (i32, i32),
    color: [f32; 4],
) -> Vec<TextDraw> {
    layout
        .glyphs
        .iter()
        .map(|g| TextDraw {
            dst: (pen.0 + g.dst_x, pen.1 + g.dst_y, g.width, g.height),
            src: (g.atlas_x, g.atlas_y, g.width, g.height),
            // A `0xCE` escape sprite keeps its own colours: `FUN_8002C488`
            // emits it at the neutral `0x808080`, whatever the text pen.
            color: if g.byte == legaia_font::ESCAPE_GLYPH_BYTE {
                [1.0, 1.0, 1.0, color[3]]
            } else {
                color
            },
        })
        .collect()
}

/// One cell of a fixed-width number field (`FUN_80034B78` / `FUN_80034E4C`)
/// at stage `pen`: the 8x12 menu numeral sprite when the font carries the
/// cells ([`legaia_font::Font::with_sprite_cells`]), tinted by `color` the
/// way retail stages the ink's CLUT, else the dialog-font glyph. Retail's
/// menus draw every price, count, stat and purse through these cells - the
/// bold fixed-width digits - and the dialog font only inside strings.
pub fn numeral_cell_draws(
    font: &legaia_font::Font,
    ch: char,
    pen: (i32, i32),
    color: [f32; 4],
) -> Vec<TextDraw> {
    if let Some(d) = ch.to_digit(10)
        && let Some(c) = font.sprite_cell(legaia_font::sprite_cell_ids::NUMERAL + d as u16)
    {
        return vec![TextDraw {
            dst: (pen.0, pen.1, c.w, c.h),
            src: (c.atlas_x, c.atlas_y, c.w, c.h),
            // The cell is decoded under the ink-7 CLUT already.
            color: numeral_cell_tint(color),
        }];
    }
    text_draws_for(&font.layout_ascii(&ch.to_string()), pen, color)
}

/// Map a slice of [`TextDraw`]s whose `dst` coordinates are expressed in
/// **stage pixels** (a virtual 320×240 PSX framebuffer) into surface
/// coordinates: `dst = stage_origin + dst * stage_scale`, with the glyph
/// size scaled to match.
///
/// The menu text builders ([`field_menu_draws_for`],
/// [`status_screen_draws_for_with`], [`spell_menu_draws_for`], …) lay glyphs
/// out at retail-pinned stage-pixel pens. This is the single transform
/// that upscales + centers them into the surface, matching the chrome
/// emitted by [`menu_window_chrome_draws_for`] so text and window frame
/// stay locked together at any window size. Apply it in place after the
/// builder returns, then composite the result.
pub fn scale_stage_text_draws(draws: &mut [TextDraw], stage_origin: (i32, i32), stage_scale: u32) {
    let scale = stage_scale.max(1);
    for d in draws.iter_mut() {
        d.dst = (
            stage_origin.0 + d.dst.0 * scale as i32,
            stage_origin.1 + d.dst.1 * scale as i32,
            d.dst.2 * scale,
            d.dst.3 * scale,
        );
    }
}
