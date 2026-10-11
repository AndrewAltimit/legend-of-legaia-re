//! `ScreenFxFrame`: one frame's widget output, the ordering-table depths and the quad list a renderer draws.

use super::*;

// ---------------------------------------------------------------------------
// Aggregate widget host (the engine face of the PROT-0900 actor pool)
// ---------------------------------------------------------------------------

/// One frame of widget output, ready for a renderer: solid border/band
/// quads (drawn flat black), the letterbox feather strips, and the
/// textured sprite / panel draws (sampled from PSX VRAM by clut/texpage).
#[derive(Debug, Default, Clone)]
pub struct ScreenFxFrame {
    /// The **mask** widget's four flat black border quads (OT `+0x1c`).
    ///
    /// The letterbox's solid bands are NOT here: retail links them at OT
    /// `+0x4`, at the opposite end of the ordering table, so batching the two
    /// families together draws a letterbox band behind the sprites it is
    /// supposed to cover. See [`Self::band_quads`].
    pub solid_quads: Vec<MaskQuad>,
    /// The letterbox widget's two solid black bands (OT `+0x4` - in FRONT of
    /// every other widget kind).
    pub band_quads: Vec<MaskQuad>,
    /// Letterbox feather strips as `(rect, top_is_white)` (see
    /// [`Letterbox::gradient_bands`]).
    pub gradient_quads: Vec<(MaskQuad, bool)>,
    /// Live sprite-widget draws.
    pub sprites: Vec<SpriteDraw>,
    /// Image-panel quads (1 or 2 per panel - the >256px split).
    pub panels: Vec<PanelQuad>,
}

/// Retail ordering-table slots the four widget kinds link their primitives at
/// (`func_0x8003d2c4(_DAT_1F8003F4 + slot, packet)`), read off the handler
/// disassembly rather than inferred: a **larger** slot is farther and is drawn
/// earlier, so the paint order back-to-front is mask, panel, sprite, letterbox.
///
/// The letterbox sitting in FRONT of the sprites is the load-bearing part: a
/// renderer that batches its bands with the mask's borders (both are flat black
/// quads) draws them behind every sprite the same scene spawns.
///
/// See `ghidra/scripts/funcs/overlay_dance_801f8a34.txt` (`+4`),
/// `..._801f7a9c.txt` (`+0xc`), `..._801f849c.txt` (`+0x10`),
/// `..._801f811c.txt` (`+0x1c`).
pub const OT_LETTERBOX: u32 = 0x04;
/// See [`OT_LETTERBOX`].
pub const OT_SPRITE: u32 = 0x0C;
/// See [`OT_LETTERBOX`].
pub const OT_PANEL: u32 = 0x10;
/// See [`OT_LETTERBOX`].
pub const OT_MASK: u32 = 0x1C;

/// One renderer-ready screen-space primitive of the widget family - the shape
/// every host draws, in the retail GP0 vertex order
/// `(left,top) (right,top) (left,bottom) (right,bottom)`.
///
/// This is deliberately a 1:1 mirror of `legaia_render_kernels::screen_prim`'s two
/// variants without depending on that crate (`engine-core` is below it): each
/// host re-wraps these mechanically, so the geometry, the culling, the UVs and
/// the ordering are decided **once**, here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenFxQuad {
    /// Untextured quad. `gouraud` carries per-corner RGBA (a `POLY_G4`) and
    /// overrides `rgba` when present.
    Flat {
        xy: [(i16, i16); 4],
        rgba: [u8; 4],
        gouraud: Option<[[u8; 4]; 4]>,
        semi_transparent: bool,
        /// PSX ABR blend mode `0..=3`; only consulted when semi-transparent.
        abr_mode: u8,
        ot: u32,
    },
    /// Textured quad sampling PSX VRAM through the usual CLUT/texpage pair.
    Textured {
        xy: [(i16, i16); 4],
        uv: [(u8, u8); 4],
        clut: u16,
        tpage: u16,
        /// 24-bit modulation colour `0x00RRGGBB` (`0x808080` = passthrough).
        color: u32,
        semi_transparent: bool,
        ot: u32,
    },
}

/// ABR mode 2 - **subtractive** blending, which is what the letterbox feather
/// strips are drawn with (`FUN_80059010(.., 0x55, ..)`), so their white edge
/// darkens to black instead of brightening.
pub const ABR_SUBTRACTIVE: u8 = 2;

/// Corner order of every quad here: `(l,t) (r,t) (l,b) (r,b)`.
fn rect_corners(l: i16, t: i16, r: i16, b: i16) -> [(i16, i16); 4] {
    [(l, t), (r, t), (l, b), (r, b)]
}

impl ScreenFxFrame {
    /// `true` when the frame draws nothing.
    pub fn is_empty(&self) -> bool {
        self.solid_quads.is_empty()
            && self.band_quads.is_empty()
            && self.gradient_quads.is_empty()
            && self.sprites.is_empty()
            && self.panels.is_empty()
    }

    /// The frame as renderer-ready primitives, back to front.
    ///
    /// Degenerate rects (a tween that has closed an edge past its opposite) are
    /// dropped here rather than in each host, which is what keeps a
    /// zero-or-negative-area quad from reaching a rasteriser as a stray
    /// triangle pair.
    pub fn draw_quads(&self) -> Vec<ScreenFxQuad> {
        let mut out = Vec::new();
        let flat = |q: &MaskQuad, ot: u32| -> Option<ScreenFxQuad> {
            (q.right > q.left && q.bottom > q.top).then(|| ScreenFxQuad::Flat {
                xy: rect_corners(q.left, q.top, q.right, q.bottom),
                rgba: [0, 0, 0, 255],
                gouraud: None,
                semi_transparent: false,
                abr_mode: 0,
                ot,
            })
        };
        // Mask borders - farthest.
        out.extend(self.solid_quads.iter().filter_map(|q| flat(q, OT_MASK)));
        // Image panels.
        for p in &self.panels {
            if p.right <= p.left || p.bottom <= p.top {
                continue;
            }
            out.push(ScreenFxQuad::Textured {
                xy: rect_corners(p.left, p.top, p.right, p.bottom),
                uv: [(p.u0, p.v0), (p.u1, p.v0), (p.u0, p.v1), (p.u1, p.v1)],
                // 15bpp direct-colour page: the spawn ORs `0x100` into the page
                // selector, so there is no CLUT to bind.
                clut: 0,
                tpage: p.texpage,
                color: 0x0088_8888,
                semi_transparent: false,
                ot: OT_PANEL,
            });
        }
        // Scripted sprites.
        for s in &self.sprites {
            if s.w <= 0 || s.h <= 0 {
                continue;
            }
            let u1 = (i32::from(s.u) + i32::from(s.w) - 1).min(255) as u8;
            let v1 = (i32::from(s.v) + i32::from(s.h) - 1).min(255) as u8;
            out.push(ScreenFxQuad::Textured {
                xy: rect_corners(s.x, s.y, s.x + s.w, s.y + s.h),
                uv: [(s.u, s.v), (u1, s.v), (s.u, v1), (u1, v1)],
                clut: s.clut as u16,
                tpage: s.texpage as u16,
                color: (u32::from(s.rgb[0]) << 16)
                    | (u32::from(s.rgb[1]) << 8)
                    | u32::from(s.rgb[2]),
                semi_transparent: false,
                ot: OT_SPRITE,
            });
        }
        // Letterbox - nearest of all. Bands first, then their feather strips.
        out.extend(self.band_quads.iter().filter_map(|q| flat(q, OT_LETTERBOX)));
        for (q, top_is_white) in &self.gradient_quads {
            if q.right <= q.left || q.bottom <= q.top {
                continue;
            }
            let white = [255u8, 255, 255, 255];
            let black = [0u8, 0, 0, 255];
            let (top, bottom) = if *top_is_white {
                (white, black)
            } else {
                (black, white)
            };
            out.push(ScreenFxQuad::Flat {
                xy: rect_corners(q.left, q.top, q.right, q.bottom),
                rgba: black,
                gouraud: Some([top, top, bottom, bottom]),
                semi_transparent: true,
                abr_mode: ABR_SUBTRACTIVE,
                ot: OT_LETTERBOX,
            });
        }
        out
    }
}
