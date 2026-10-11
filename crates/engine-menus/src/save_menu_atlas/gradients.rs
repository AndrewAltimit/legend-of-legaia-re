//! Procedural gradient fills baked into the atlas: panel interior, gauge fill and dialog fill.

use super::*;

/// Pre-bake the gouraud gray gradient retail applies to the panel
/// interior into the atlas. Reads `rect` from `src_rgba` (CLUT-row-2
/// pixels of the marbled-blue source region), multiplies each pixel
/// by a per-row linear gradient between `top_rgb` and `bot_rgb`
/// scaled to `[0, 1]`, and writes the result into `dst` at the same
/// rect coords.
///
/// PSX hardware does this as a per-vertex color modulation in the
/// 0x3C textured-quad primitive (top vertices have rgb 64,64,64;
/// bottom vertices have rgb 136,136,136), so the GPU interpolates
/// linearly across the quad. We bake the same linear interpolation
/// into the atlas so the engine can draw the result as a regular
/// SpriteDraw without needing per-vertex colors.
pub(super) fn bake_panel_interior_gradient(
    dst: &mut [u8],
    src_rgba: &[u8],
    src_w: u32,
    rect: (u32, u32, u32, u32),
    top_rgb: (u8, u8, u8),
    bot_rgb: (u8, u8, u8),
) {
    let (x0, y0, w, h) = rect;
    let dst_stride = (ATLAS_WIDTH * 4) as usize;
    let src_stride = (src_w * 4) as usize;
    // Per-row gradient factor in 0..255 scaled (256 lerp).
    let lerp_chan = |a: u8, b: u8, t_num: u32, t_den: u32| -> u8 {
        // Avoid div-by-zero for single-row interiors.
        if t_den == 0 {
            return a;
        }
        let aa = a as u32;
        let bb = b as u32;
        ((aa * (t_den - t_num) + bb * t_num) / t_den) as u8
    };
    for row in 0..h {
        let t_num = row;
        let t_den = h.saturating_sub(1).max(1);
        let mod_r = lerp_chan(top_rgb.0, bot_rgb.0, t_num, t_den);
        let mod_g = lerp_chan(top_rgb.1, bot_rgb.1, t_num, t_den);
        let mod_b = lerp_chan(top_rgb.2, bot_rgb.2, t_num, t_den);
        let src_off = ((y0 + row) as usize) * src_stride + (x0 as usize) * 4;
        let dst_off = ((y0 + row) as usize) * dst_stride + (x0 as usize) * 4;
        for col in 0..w {
            let o = col as usize * 4;
            // PSX color modulation is `(tex * color) / 128` (i.e.
            // 0x80 = identity, 0xFF = ~2x). Mirror that semantic.
            let modulate = |tex: u8, color: u8| -> u8 {
                let prod = (tex as u32 * color as u32) / 128;
                prod.min(255) as u8
            };
            dst[dst_off + o] = modulate(src_rgba[src_off + o], mod_r);
            dst[dst_off + o + 1] = modulate(src_rgba[src_off + o + 1], mod_g);
            dst[dst_off + o + 2] = modulate(src_rgba[src_off + o + 2], mod_b);
            dst[dst_off + o + 3] = src_rgba[src_off + o + 3];
        }
    }
}

/// Bake the AP-gauge meter-fill gradient column into the atlas.
///
/// Retail (`FUN_8002c0b0`) draws the fill as two 3-row untextured
/// gouraud quads: dark-red `(0x80,0x20,0x10)` at the top edge fading
/// to gold `(0xC0,0xA0,0x40)` at the shared middle edge, then back to
/// dark-red at the (exclusive) bottom edge - so the visible rows are
/// `[dark, 1/3, 2/3, gold, 2/3, 1/3]`. Per-row linear interpolation
/// approximates the GPU's per-pixel DDA (sub-pixel truncation may
/// differ by at most 1/255 until an AP>0 retail capture pins it).
pub(super) fn bake_gauge_fill_gradient(dst: &mut [u8], rect: (u32, u32, u32, u32)) {
    let (dark, gold) = (
        title_pak::OVERLAY_SYSTEM_UI_GAUGE_FILL_DARK_RGB,
        title_pak::OVERLAY_SYSTEM_UI_GAUGE_FILL_GOLD_RGB,
    );
    let lerp3 = |a: u8, b: u8, i: u32| -> u8 { ((a as u32 * (3 - i) + b as u32 * i) / 3) as u8 };
    let (x0, y0, w, h) = rect;
    let stride = (ATLAS_WIDTH * 4) as usize;
    for row in 0..h {
        // Rows 0..2 climb dark->gold; rows 3..5 descend gold->dark
        // (row 3 is the quad-2 top edge = full gold).
        let i = if row < 3 { row } else { 6 - row };
        let (r, g, b) = (
            lerp3(dark.0, gold.0, i),
            lerp3(dark.1, gold.1, i),
            lerp3(dark.2, gold.2, i),
        );
        for col in 0..w {
            let off = ((y0 + row) as usize) * stride + ((x0 + col) as usize) * 4;
            dst[off] = r;
            dst[off + 1] = g;
            dst[off + 2] = b;
            dst[off + 3] = 255;
        }
    }
}

/// Bake the dialog-window interior fill gradient column.
///
/// PORT: FUN_8002C69C (fill body) - the retail emitter's hardcoded
/// interior: two identical semi-transparent (mode 0, `B/2 + F/2`)
/// gouraud `POLY_G4` quads spanning the whole box rect, top vertices
/// `(0x18,0x18,0x28)`, bottom vertices `(0x40,0x40,0xA0)`. Applying
/// mode-0 twice composes to `0.25*back + 0.75*gradient`, which a
/// single source-over sprite with alpha `191` reproduces exactly.
pub(super) fn bake_dialog_fill_gradient(dst: &mut [u8], rect: (u32, u32, u32, u32)) {
    let top = title_pak::OVERLAY_SYSTEM_UI_DIALOG_FILL_TOP_RGB;
    let bot = title_pak::OVERLAY_SYSTEM_UI_DIALOG_FILL_BOT_RGB;
    let (x0, y0, w, h) = rect;
    let span = (h - 1).max(1);
    let lerp = |a: u8, b: u8, row: u32| -> u8 {
        ((a as u32 * (span - row) + b as u32 * row) / span) as u8
    };
    let stride = (ATLAS_WIDTH * 4) as usize;
    for row in 0..h {
        let (r, g, b) = (
            lerp(top.0, bot.0, row),
            lerp(top.1, bot.1, row),
            lerp(top.2, bot.2, row),
        );
        for col in 0..w {
            let off = ((y0 + row) as usize) * stride + ((x0 + col) as usize) * 4;
            dst[off] = r;
            dst[off + 1] = g;
            dst[off + 2] = b;
            dst[off + 3] = title_pak::OVERLAY_SYSTEM_UI_DIALOG_FILL_ALPHA;
        }
    }
}
