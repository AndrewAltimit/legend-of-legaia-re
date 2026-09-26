//! Slot-machine **payline** segments as screen-space primitives.
//!
//! Retail draws each of the five paylines as one GP0 `0x43` packet - a flat,
//! semi-transparent two-point line - after `RTPS`-projecting both endpoints
//! (`FUN_801D3380`, ported as `legaia_engine_core::slot_machine::payline_prims`
//! with the projection in `slot_machine::projected_paylines`). The overlay
//! framebuffer this module feeds carries quads, not lines, so each segment
//! becomes a one-pixel-thick flat quad along the line: the same pixels a PSX
//! line rasterises, give or take the end caps.
//!
//! Input coordinates are on the machine's own **640x240** framebuffer (the
//! slot overlay sets video mode `0x280`, see
//! `legaia_asset::minigame_slot_scene::SCREEN_W`); the output is in the
//! `PSX_DISPLAY_W` x `PSX_DISPLAY_H` space every screen primitive is authored
//! in, so x is halved on the way through.

use crate::screen_prim::{FlatQuad, PSX_DISPLAY_W, ScreenPrim};

/// Ordering-table bucket the payline quads link at: over the reels, under
/// every UI overlay the host composites later.
pub const PAYLINE_OT: u32 = 2;

/// Width of the framebuffer the segment endpoints are expressed on.
pub const SLOT_FRAMEBUFFER_W: f32 = 640.0;

/// One payline segment, projected.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaylineSegment {
    /// Endpoints on the 640x240 slot framebuffer.
    pub a: [f32; 2],
    pub b: [f32; 2],
    /// Packet colour.
    pub rgb: [u8; 3],
    /// GP0 `0x43` carries the semi-transparency bit, so every payline blends.
    pub semi: bool,
}

/// Turn projected payline segments into flat one-pixel quads in display
/// space. A zero-length segment produces nothing.
pub fn payline_screen_prims(segments: &[PaylineSegment]) -> Vec<ScreenPrim> {
    let sx = PSX_DISPLAY_W as f32 / SLOT_FRAMEBUFFER_W;
    segments
        .iter()
        .filter_map(|s| {
            let (ax, ay) = (s.a[0] * sx, s.a[1]);
            let (bx, by) = (s.b[0] * sx, s.b[1]);
            let (dx, dy) = (bx - ax, by - ay);
            let len = (dx * dx + dy * dy).sqrt();
            if len < f32::EPSILON {
                return None;
            }
            // Half-pixel normal: the quad spans one display pixel across.
            let (nx, ny) = (-dy / len * 0.5, dx / len * 0.5);
            let p = |x: f32, y: f32| (x.round() as i16, y.round() as i16);
            // Keep the quad at least one pixel tall/wide after rounding, the
            // way the PSX rasteriser always lights one pixel per step.
            let (mut a0, mut a1) = (p(ax + nx, ay + ny), p(ax - nx, ay - ny));
            let (mut b0, mut b1) = (p(bx + nx, by + ny), p(bx - nx, by - ny));
            if a0 == a1 {
                if dx.abs() >= dy.abs() {
                    a1.1 += 1;
                    b1.1 += 1;
                } else {
                    a1.0 += 1;
                    b1.0 += 1;
                }
            }
            if a0.1 > a1.1 || (a0.1 == a1.1 && a0.0 > a1.0) {
                std::mem::swap(&mut a0, &mut a1);
                std::mem::swap(&mut b0, &mut b1);
            }
            Some(ScreenPrim::Flat(FlatQuad {
                xy: [a0, b0, a1, b1],
                color: [s.rgb[0], s.rgb[1], s.rgb[2], 0xFF],
                gouraud: None,
                semi_transparent: s.semi,
                // ABR 0: `0.5 * back + 0.5 * front`, the texpage default the
                // slot overlay's draw environment leaves in place.
                abr_mode: 0,
                ot_index: PAYLINE_OT,
                depth: None,
            }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_horizontal_segment_is_one_display_pixel_tall_and_half_as_wide() {
        let prims = payline_screen_prims(&[PaylineSegment {
            a: [100.0, 50.0],
            b: [300.0, 50.0],
            rgb: [0x80, 0x80, 0x80],
            semi: true,
        }]);
        assert_eq!(prims.len(), 1);
        let ScreenPrim::Flat(q) = prims[0] else {
            panic!("flat quad");
        };
        let xs: Vec<i16> = q.xy.iter().map(|c| c.0).collect();
        let ys: Vec<i16> = q.xy.iter().map(|c| c.1).collect();
        assert_eq!(*xs.iter().min().unwrap(), 50);
        assert_eq!(*xs.iter().max().unwrap(), 150);
        assert_eq!(ys.iter().max().unwrap() - ys.iter().min().unwrap(), 1);
        assert!(q.semi_transparent);
        assert_eq!(q.abr_mode, 0);
        assert_eq!(q.ot_index, PAYLINE_OT);
    }

    #[test]
    fn a_diagonal_keeps_its_endpoints_and_a_degenerate_one_draws_nothing() {
        let prims = payline_screen_prims(&[
            PaylineSegment {
                a: [0.0, 0.0],
                b: [200.0, 100.0],
                rgb: [0xFF, 0xFF, 0x80],
                semi: true,
            },
            PaylineSegment {
                a: [7.0, 7.0],
                b: [7.0, 7.0],
                rgb: [0; 3],
                semi: true,
            },
        ]);
        assert_eq!(prims.len(), 1);
        let ScreenPrim::Flat(q) = prims[0] else {
            panic!("flat quad");
        };
        assert_eq!(q.color, [0xFF, 0xFF, 0x80, 0xFF]);
        let far = q.xy.iter().map(|c| c.0).max().unwrap();
        assert_eq!(far, 100, "x halves onto the 320-wide display");
    }
}
