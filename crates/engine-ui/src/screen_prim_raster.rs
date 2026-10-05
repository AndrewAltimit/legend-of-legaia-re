//! A CPU rasteriser for a [`ScreenPrim`] list - the GPU screen-primitive
//! pass's twin for a surface that has no GPU pass: a 2D canvas.
//!
//! It runs the same per-pixel rules both hosts' screen-prim shaders run
//! (`SCREEN_OVERLAY_SHADER_SRC` in `engine-render`, `FS_SCREEN_PRIM` on the
//! play page): the [`order_primitives`] ordering-table walk, two triangles
//! per quad (`v0 v1 v2` + `v1 v2 v3`), affine UV and colour interpolation,
//! the VRAM texel fetch through the texpage's colour depth and the CLUT, a
//! `0x0000` texel discarded, the 5-bit [`psx_texture_blend`] modulation, and
//! the four ABR equations applied to the whole quad of a semi-transparent
//! run. It is a presentation path for a page, not a parity oracle for the
//! GPU: edge coverage is pixel-centre sampling rather than the PSX fill rule.

use crate::screen_prim::{
    PSX_DISPLAY_H, PSX_DISPLAY_W, ScreenPrim, order_primitives, psx_texture_blend,
};

/// VRAM width in 16-bit words.
const VRAM_W: usize = 1024;
/// VRAM height in rows.
const VRAM_H: usize = 512;

fn vram_word(vram: &[u16], x: usize, y: usize) -> u16 {
    vram.get((y % VRAM_H) * VRAM_W + (x % VRAM_W))
        .copied()
        .unwrap_or(0)
}

/// The texel word at `(u, v)` through `tsb`'s page and depth and `cba`'s
/// CLUT - the shaders' `fetch_vram_word`.
fn fetch(vram: &[u16], u: f32, v: f32, cba: u16, tsb: u16) -> u16 {
    let u_pix = (u.max(0.0) as u32 & 255) as usize;
    let v_pix = (v.max(0.0) as u32 & 255) as usize;
    let page_x = (tsb as usize & 15) * 64;
    let page_y = ((tsb as usize >> 4) & 1) * 256;
    let clut_x = (cba as usize & 63) * 16;
    let clut_y = (cba as usize >> 6) & 511;
    match (tsb >> 7) & 3 {
        0 => {
            let word = vram_word(vram, page_x + (u_pix >> 2), page_y + v_pix);
            let idx = (word >> ((u_pix & 3) * 4)) & 15;
            vram_word(vram, clut_x + idx as usize, clut_y)
        }
        1 => {
            let word = vram_word(vram, page_x + (u_pix >> 1), page_y + v_pix);
            let idx = (word >> ((u_pix & 1) * 8)) & 255;
            vram_word(vram, clut_x + idx as usize, clut_y)
        }
        _ => vram_word(vram, page_x + u_pix, page_y + v_pix),
    }
}

/// One corner as the rasteriser reads it: target-space position, texel
/// coordinates and an 8-bit-per-channel colour (`0x80` = neutral for a
/// textured quad).
#[derive(Clone, Copy)]
struct Corner {
    x: f32,
    y: f32,
    u: f32,
    v: f32,
    c: [f32; 3],
}

fn blend(back: f32, front: f32, abr: u8) -> f32 {
    match abr & 3 {
        0 => 0.5 * back + 0.5 * front,
        1 => back + front,
        2 => back - front,
        _ => back + 0.25 * front,
    }
    .clamp(0.0, 1.0)
}

/// Rasterise `prims` (authored in the 320x240 display space) onto a
/// `w x h` RGBA8 surface cleared to black, sampling textures out of `vram`
/// (1024x512 BGR555 words).
pub fn rasterize_rgba(prims: &[ScreenPrim], vram: &[u16], w: u32, h: u32) -> Vec<u8> {
    rasterize(prims, vram, w, h, false)
}

/// [`rasterize_rgba`] for an **overlay**: a pixel no primitive drew comes back
/// fully transparent instead of black, so a page can composite the list over
/// a frame it drew itself (the minigames page's fishing HUD over its pond).
/// A drawn pixel is opaque; a blended primitive is blended against black,
/// which is exact for the additive (ABR 1) sprites and an approximation for
/// the other rates.
pub fn rasterize_rgba_overlay(prims: &[ScreenPrim], vram: &[u16], w: u32, h: u32) -> Vec<u8> {
    rasterize(prims, vram, w, h, true)
}

fn rasterize(prims: &[ScreenPrim], vram: &[u16], w: u32, h: u32, overlay: bool) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let mut px = vec![[0.0f32; 3]; w * h];
    let mut covered = vec![false; w * h];
    let sx = w as f32 / PSX_DISPLAY_W as f32;
    let sy = h as f32 / PSX_DISPLAY_H as f32;
    for i in order_primitives(prims) {
        let (corners, textured, cba, tsb, semi, abr) = match &prims[i] {
            ScreenPrim::Textured(q) => {
                let col = |c: u32| {
                    [
                        ((c >> 16) & 0xFF) as f32,
                        ((c >> 8) & 0xFF) as f32,
                        (c & 0xFF) as f32,
                    ]
                };
                let cs = q.gouraud.unwrap_or([q.color; 4]);
                let corners: [Corner; 4] = core::array::from_fn(|k| Corner {
                    x: q.xy[k].0 as f32 * sx,
                    y: q.xy[k].1 as f32 * sy,
                    u: q.uv[k].0 as f32,
                    v: q.uv[k].1 as f32,
                    c: col(cs[k]),
                });
                (
                    corners,
                    true,
                    q.clut,
                    q.tpage,
                    q.semi_transparent,
                    q.abr_mode(),
                )
            }
            ScreenPrim::Flat(q) => {
                let cs = q.gouraud.unwrap_or([q.color; 4]);
                let corners: [Corner; 4] = core::array::from_fn(|k| Corner {
                    x: q.xy[k].0 as f32 * sx,
                    y: q.xy[k].1 as f32 * sy,
                    u: 0.0,
                    v: 0.0,
                    c: [cs[k][0] as f32, cs[k][1] as f32, cs[k][2] as f32],
                });
                (corners, false, 0, 0, q.semi_transparent, q.abr_mode & 3)
            }
        };
        for tri in [[0usize, 1, 2], [1, 2, 3]] {
            let [a, b, c] = tri.map(|k| corners[k]);
            let area = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
            if area.abs() < 1e-6 {
                continue;
            }
            let x0 = a.x.min(b.x).min(c.x).floor().max(0.0) as usize;
            let x1 = (a.x.max(b.x).max(c.x).ceil() as usize).min(w);
            let y0 = a.y.min(b.y).min(c.y).floor().max(0.0) as usize;
            let y1 = (a.y.max(b.y).max(c.y).ceil() as usize).min(h);
            for y in y0..y1 {
                for x in x0..x1 {
                    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                    let wa = ((b.x - fx) * (c.y - fy) - (c.x - fx) * (b.y - fy)) / area;
                    let wb = ((c.x - fx) * (a.y - fy) - (a.x - fx) * (c.y - fy)) / area;
                    let wc = 1.0 - wa - wb;
                    if wa < 0.0 || wb < 0.0 || wc < 0.0 {
                        continue;
                    }
                    let lerp = |p: f32, q: f32, r: f32| wa * p + wb * q + wc * r;
                    let col: [f32; 3] = core::array::from_fn(|k| lerp(a.c[k], b.c[k], c.c[k]));
                    let front = if textured {
                        let word = fetch(vram, lerp(a.u, b.u, c.u), lerp(a.v, b.v, c.v), cba, tsb);
                        if word == 0 {
                            continue;
                        }
                        let t5 = [word & 31, (word >> 5) & 31, (word >> 10) & 31];
                        core::array::from_fn(|k| {
                            let c8 = col[k].round().clamp(0.0, 255.0) as u8;
                            psx_texture_blend(t5[k] as u8, c8) as f32 / 31.0
                        })
                    } else {
                        col.map(|v| (v / 255.0).clamp(0.0, 1.0))
                    };
                    covered[y * w + x] = true;
                    let dst = &mut px[y * w + x];
                    *dst = if semi {
                        core::array::from_fn(|k| blend(dst[k], front[k], abr))
                    } else {
                        front
                    };
                }
            }
        }
    }
    let mut out = Vec::with_capacity(w * h * 4);
    for (p, &hit) in px.iter().zip(covered.iter()) {
        out.extend(p.map(|v| (v * 255.0).round() as u8));
        out.push(if overlay && !hit { 0 } else { 0xFF });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen_prim::{FlatQuad, ScreenQuad};

    #[test]
    fn a_flat_quad_fills_its_rect_and_a_nearer_one_draws_over_it() {
        let quad = |x0: i16, x1: i16, rgb: [u8; 3], ot: u32| {
            ScreenPrim::Flat(FlatQuad {
                xy: [(x0, 0), (x1, 0), (x0, 10), (x1, 10)],
                color: [rgb[0], rgb[1], rgb[2], 255],
                gouraud: None,
                semi_transparent: false,
                abr_mode: 0,
                ot_index: ot,
                depth: None,
            })
        };
        let vram = vec![0u16; VRAM_W * VRAM_H];
        let img = rasterize_rgba(
            &[quad(0, 20, [255, 0, 0], 1), quad(10, 30, [0, 0, 255], 9)],
            &vram,
            320,
            240,
        );
        let at = |x: usize, y: usize| &img[(y * 320 + x) * 4..(y * 320 + x) * 4 + 3];
        assert_eq!(at(5, 5), &[255, 0, 0]);
        // Overlap: bucket 1 is nearer than bucket 9, so red wins.
        assert_eq!(at(15, 5), &[255, 0, 0]);
        assert_eq!(at(25, 5), &[0, 0, 255]);
        assert_eq!(at(35, 5), &[0, 0, 0]);
    }

    #[test]
    fn a_textured_quad_samples_its_clut_and_drops_zero_texels() {
        let mut vram = vec![0u16; VRAM_W * VRAM_H];
        // 15bpp page at (0, 0): texel (0,0) white, texel (1,0) transparent.
        vram[0] = 0x7FFF;
        let q = ScreenPrim::Textured(ScreenQuad {
            xy: [(0, 0), (2, 0), (0, 1), (2, 1)],
            uv: [(0, 0), (2, 0), (0, 1), (2, 1)],
            clut: 0,
            tpage: 2 << 7,
            color: 0x0080_8080,
            gouraud: None,
            semi_transparent: false,
            ot_index: 0,
            depth: None,
        });
        let img = rasterize_rgba(&[q], &vram, 320, 240);
        assert_eq!(&img[0..3], &[255, 255, 255]);
        assert_eq!(&img[4..7], &[0, 0, 0]);
    }
}
