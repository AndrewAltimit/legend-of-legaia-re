//! The fishing HUD's **sprite quads**: the overlay's shared emitter
//! `FUN_801D63B0` and its two digit wrappers, as screen-space PSX primitives
//! sampling the venue's own HUD page.
//!
//! Every fishing HUD glyph - the HI SCORE / POINT plates, the gauge labels and
//! caps, the hook / catch / miss banners, the score digits - is one record of
//! the overlay's sprite table ([`legaia_asset::fishing_sprites`]) drawn as a
//! `POLY_GT4` out of the 4bpp page at `(832, 0)` with a palette of the
//! `(0, 503)` strip. That page is the first TIM of the venue bundle's texture
//! list (`other1`), so it is already resident wherever the pond is: the native
//! window and the play page draw these primitives through the screen-primitive
//! pass over the pond VRAM, and the minigames page rasterises them over its
//! own pond VRAM ([`crate::screen_prim_raster`]).
//!
//! [`fishing_hud_sprite_prims`] takes the same [`HudDraw`] list the text
//! consumer ([`crate::fishing_hud_draws_for`]) does and covers the items that
//! are sprites in retail: glyphs, numbers and the bar frames. A host drawing
//! these sets [`crate::FishingHudAtlas::sprites_drawn`] so the text consumer
//! leaves those items to them.

use crate::screen_prim::{ScreenPrim, ScreenQuad};
use crate::{BarAxis, HudDraw, number_digit_cells};
use legaia_asset::fishing_sprites::FishingSprite;

/// The ordering-table bucket every HUD sprite links at: `_DAT_801D9158`,
/// which the driver's setup state stores as `3` (`0x801CF114`) and the digit
/// field restores after each number (`0x801D77F0`).
pub const FISHING_SPRITE_OT: u32 = 3;

/// The CLUT a mode-`2` id substitutes (`li v0, 0x7DCF` at `0x801D6778`):
/// palette `0x0F` of the `(0, 503)` strip, all white - the shadow layer of the
/// large digits.
pub const FISHING_MODE2_CLUT: u16 = 0x7DCF;

/// Record the small-digit emitter `FUN_801D7DD8` draws, after patching its
/// `u` to `digit * 8 + 0x28` (`0x801D7DEC..0x801D7E00`).
pub const FISHING_SMALL_DIGIT_ID: u32 = 6;
/// Record the large-digit emitter `FUN_801D7D44` draws twice (ids `0x418`
/// and `0x818`, modes 1 and 2), after patching its `u` to `digit << 4`.
pub const FISHING_LARGE_DIGIT_ID: u32 = 0x18;

/// The MIPS `mult; bgez; addiu (2^n - 1); sra n` idiom: a product shifted
/// right with rounding toward zero.
fn scale_toward_zero(value: i32, factor: i32, shift: u32) -> i32 {
    let p = value.wrapping_mul(factor);
    let p = if p < 0 { p + ((1 << shift) - 1) } else { p };
    p >> shift
}

/// One call of `FUN_801D63B0(anchor, x, y, id, brightness, scale_x,
/// scale_y)` against `rec`, the record `id & 0x3FF` names (a digit emitter
/// passes its patched copy).
///
/// - `id >> 10` is a blend **mode**: `0` keeps the record's own `semi` bit
///   and ABR rate; any other mode forces semi-transparency on with the mode
///   itself as the rate, and mode `2` swaps in [`FISHING_MODE2_CLUT`]. The
///   texpage word is `tpage + rate * 0x20` (`0x801D6760..0x801D676C`).
/// - The quad is `w * scale >> 12` then `* scale_x >> 12` wide (and likewise
///   tall), both shifts rounding toward zero (`0x801D65F4..0x801D666C`).
/// - A non-zero `anchor` puts the quad's top-left corner on `(x, y)`; `0`
///   centres it, half-extents rounded toward zero (`0x801D6670..0x801D66DC`).
/// - Vertices 0 / 1 carry the record's top colour and 2 / 3 its bottom one,
///   each channel `* brightness >> 8`; the UVs span the record's `w x h`
///   texels from `(u, v)` regardless of the drawn size.
///
/// PORT: overlay_fishing_0972_801d63b0
#[allow(clippy::too_many_arguments)]
pub fn fishing_sprite_quad(
    rec: &FishingSprite,
    anchor: i32,
    x: i32,
    y: i32,
    id: u32,
    brightness: i32,
    scale_x: i32,
    scale_y: i32,
) -> ScreenPrim {
    let mode = (id >> 10) as u8;
    let (semi, rate) = if mode == 0 {
        (rec.semi, rec.abr)
    } else {
        (true, mode)
    };
    let tint = |c: [u8; 3]| {
        let ch = |v: u8| (scale_toward_zero(i32::from(v), brightness, 8) & 0xFF) as u32;
        (ch(c[0]) << 16) | (ch(c[1]) << 8) | ch(c[2])
    };
    let (top, bottom) = (tint(rec.rgb_top), tint(rec.rgb_bottom));
    let w = scale_toward_zero(
        scale_toward_zero(i32::from(rec.w), rec.scale, 12),
        scale_x,
        12,
    );
    let h = scale_toward_zero(
        scale_toward_zero(i32::from(rec.h), rec.scale, 12),
        scale_y,
        12,
    );
    let (x0, x1, y0, y1) = if anchor != 0 {
        (x, x + w, y, y + h)
    } else {
        let (hw, hh) = (w / 2, h / 2);
        (x - hw, x + hw, y - hh, y + hh)
    };
    let (u0, v0) = (rec.u, rec.v);
    let (u1, v1) = (rec.u.wrapping_add(rec.w), rec.v.wrapping_add(rec.h));
    let p = |a: i32, b: i32| (a as i16, b as i16);
    ScreenPrim::Textured(ScreenQuad {
        xy: [p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1)],
        uv: [(u0, v0), (u1, v0), (u0, v1), (u1, v1)],
        clut: if mode == 2 {
            FISHING_MODE2_CLUT
        } else {
            rec.clut
        },
        tpage: rec.tpage.wrapping_add(u16::from(rate) * 0x20),
        color: top,
        gouraud: Some([top, top, bottom, bottom]),
        semi_transparent: semi,
        ot_index: FISHING_SPRITE_OT,
        depth: None,
    })
}

/// `FUN_801D7DD8(x, y, digit, brightness, scale)` - one small digit: record
/// [`FISHING_SMALL_DIGIT_ID`] with `u = digit * 8 + 0x28`, top-left anchored.
///
/// PORT: overlay_fishing_0972_801d7dd8
pub fn fishing_small_digit(
    table: &[FishingSprite],
    x: i32,
    y: i32,
    digit: i32,
    brightness: i32,
    scale: i32,
) -> Option<ScreenPrim> {
    let mut rec = *table.get(FISHING_SMALL_DIGIT_ID as usize)?;
    rec.u = (((digit & 0x3FF) << 3) + 0x28) as u8;
    Some(fishing_sprite_quad(
        &rec,
        1,
        x,
        y,
        FISHING_SMALL_DIGIT_ID,
        brightness,
        scale,
        scale,
    ))
}

/// `FUN_801D7D44(x, y, digit, brightness, scale)` - one large digit: record
/// [`FISHING_LARGE_DIGIT_ID`] with `u = digit << 4`, drawn twice, as id
/// `0x418` (mode 1, additive) and then `0x818` (mode 2, the white palette
/// subtracted), both top-left anchored.
///
/// PORT: overlay_fishing_0972_801d7d44
///
/// Retail's one large-digit field is the landed catch's points -
/// `FUN_801D76E0(1, 0x20, 0x88, points, brightness)` at `0x801D5640` inside
/// the result actor's tick `FUN_801D5298` - which reaches this through
/// [`crate::HudDraw::LargeNumber`] in [`fishing_hud_sprite_prims`].
pub fn fishing_large_digit(
    table: &[FishingSprite],
    x: i32,
    y: i32,
    digit: i32,
    brightness: i32,
    scale: i32,
) -> Vec<ScreenPrim> {
    let Some(base) = table.get(FISHING_LARGE_DIGIT_ID as usize) else {
        return Vec::new();
    };
    let mut rec = *base;
    rec.u = ((digit & 0x3FF) << 4) as u8;
    [0x418u32, 0x818]
        .iter()
        .map(|&id| fishing_sprite_quad(&rec, 1, x, y, id, brightness, scale, scale))
        .collect()
}

/// The sprite half of the fishing HUD: every [`HudDraw::Glyph`], every
/// [`HudDraw::Number`] digit (the small-digit emitter at 1.0, the style the
/// HUD rows use) and every bar frame (start cap, body stretched by
/// [`crate::BarFrame::body_scale`] along the bar's axis, end cap, at
/// [`crate::BAR_FRAME_BRIGHTNESS`]), in emission order. Captions, counts and
/// the bar fills stay with the text consumer. Ids the table does not hold are
/// skipped.
pub fn fishing_hud_sprite_prims(items: &[HudDraw], table: &[FishingSprite]) -> Vec<ScreenPrim> {
    const ONE: i32 = 0x1000;
    let rec_for = |id: u32| table.get((id & 0x3FF) as usize);
    let mut out = Vec::new();
    for item in items {
        match *item {
            HudDraw::Glyph {
                layer,
                id,
                x,
                y,
                brightness,
            } => {
                if let Some(rec) = rec_for(id) {
                    out.push(fishing_sprite_quad(
                        rec, layer, x, y, id, brightness, ONE, ONE,
                    ));
                }
            }
            HudDraw::Number {
                x,
                y,
                value,
                brightness,
            } => {
                for cell in number_digit_cells(0, x, y, value) {
                    out.extend(fishing_small_digit(
                        table, cell.x, cell.y, cell.digit, brightness, ONE,
                    ));
                }
            }
            HudDraw::LargeNumber {
                x,
                y,
                value,
                brightness,
            } => {
                for cell in number_digit_cells(1, x, y, value) {
                    out.extend(fishing_large_digit(
                        table, cell.x, cell.y, cell.digit, brightness, ONE,
                    ));
                }
            }
            HudDraw::Bar { .. } | HudDraw::PowerBar { .. } => {
                let Some(frame) = item.resolve_bar() else {
                    continue;
                };
                for (i, (&id, &(gx, gy))) in
                    frame.glyphs.iter().zip(frame.positions.iter()).enumerate()
                {
                    let Some(rec) = rec_for(id) else {
                        continue;
                    };
                    let (sx, sy) = match (i, frame.axis) {
                        (1, BarAxis::Horizontal) => (frame.body_scale, ONE),
                        (1, BarAxis::Vertical) => (ONE, frame.body_scale),
                        _ => (ONE, ONE),
                    };
                    out.push(fishing_sprite_quad(
                        rec,
                        1,
                        gx,
                        gy,
                        id,
                        crate::BAR_FRAME_BRIGHTNESS,
                        sx,
                        sy,
                    ));
                }
            }
            HudDraw::Count { .. } | HudDraw::Caption { .. } => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec() -> FishingSprite {
        FishingSprite {
            scale: 0x1000,
            tpage: 0x0D,
            clut: 0x7DC1,
            u: 88,
            v: 224,
            w: 104,
            h: 16,
            rgb_top: [255, 255, 255],
            semi: false,
            rgb_bottom: [128, 64, 32],
            abr: 1,
        }
    }

    fn quad(p: ScreenPrim) -> ScreenQuad {
        match p {
            ScreenPrim::Textured(q) => q,
            ScreenPrim::Flat(_) => panic!("textured"),
        }
    }

    #[test]
    fn an_anchored_mode0_record_is_its_own_cell_at_x_y() {
        let q = quad(fishing_sprite_quad(
            &rec(),
            1,
            0x10,
            0x08,
            0x1A,
            0x80,
            0x1000,
            0x1000,
        ));
        assert_eq!(
            q.xy,
            [(0x10, 0x08), (0x78, 0x08), (0x10, 0x18), (0x78, 0x18)]
        );
        assert_eq!(q.uv, [(88, 224), (192, 224), (88, 240), (192, 240)]);
        assert_eq!(
            (q.clut, q.tpage, q.semi_transparent),
            (0x7DC1, 0x0D + 0x20, false)
        );
        // `255 * 0x80 >> 8` = 127; the bottom row halves too.
        assert_eq!(q.gouraud, Some([0x7F7F7F, 0x7F7F7F, 0x402010, 0x402010]));
        assert_eq!(q.ot_index, FISHING_SPRITE_OT);
    }

    #[test]
    fn a_centred_sprite_splits_its_extent_and_a_mode_overrides_the_record() {
        let q = quad(fishing_sprite_quad(
            &rec(),
            0,
            160,
            120,
            0x81A,
            0xFF,
            0x1000,
            0x1000,
        ));
        assert_eq!(q.xy[0], (160 - 52, 120 - 8));
        assert_eq!(q.xy[3], (160 + 52, 120 + 8));
        assert!(q.semi_transparent);
        assert_eq!((q.clut, q.tpage), (FISHING_MODE2_CLUT, 0x0D + 2 * 0x20));
    }

    #[test]
    fn scale_stretches_the_quad_but_not_its_texels() {
        let q = quad(fishing_sprite_quad(
            &rec(),
            1,
            0,
            0,
            4,
            0x80,
            0x3000,
            0x1000,
        ));
        assert_eq!(q.xy[1].0, 104 * 3);
        assert_eq!(q.uv[1].0, 192);
    }

    #[test]
    fn digits_patch_their_record_u() {
        let mut table = vec![rec(); 0x19];
        table[6].w = 8;
        let q = quad(fishing_small_digit(&table, 0, 0, 7, 0x80, 0x1000).unwrap());
        assert_eq!(q.uv[0].0, 7 * 8 + 0x28);
        let big = fishing_large_digit(&table, 0, 0, 3, 0x80, 0x1000);
        assert_eq!(big.len(), 2);
        let (a, b) = (quad(big[0]), quad(big[1]));
        assert_eq!((a.uv[0].0, b.uv[0].0), (48, 48));
        assert_eq!((a.clut, b.clut), (0x7DC1, FISHING_MODE2_CLUT));
    }

    #[test]
    fn the_hud_rows_become_sprites() {
        let table = vec![rec(); 29];
        let items = [
            HudDraw::Glyph {
                layer: 1,
                id: 0x1A,
                x: 0x10,
                y: 8,
                brightness: 0x80,
            },
            HudDraw::Number {
                x: 0x32,
                y: 8,
                value: 42,
                brightness: 0x80,
            },
        ];
        // One plate and two digits.
        assert_eq!(fishing_hud_sprite_prims(&items, &table).len(), 3);
        assert!(fishing_hud_sprite_prims(&items, &[]).is_empty());
    }
}
