//! The publisher-logo boot pass's **draw** half: retail's per-logo quads
//! letterboxed onto a surface.
//!
//! The layout itself is not computed here - `legaia_engine_title::publisher_logos`
//! carries `LOGO_QUADS`, retail's own source rects and destinations in the
//! 640x480 stage the boot pass runs in (`FUN_801CE9C0` selects it with
//! `FUN_8001DAF8(0x400)`). This is only the projection of that stage onto a
//! host's surface, and it is shared because both hosts do exactly the same
//! thing with it: the native `--boot-ui` chain and the browser play page's
//! title flow.
//!
//! # Why the inputs are plain tuples
//!
//! `legaia-engine-core` is a sibling of this crate, not a dependency, so the
//! stage size, the quad list and the atlas rect arrive as values rather than
//! as `publisher_logos` types - the same seam `ui_overlay`'s `ValueCellView`
//! uses. Passing the stage in (instead of restating `640x480` here) is what
//! keeps it from becoming a paired constant with nothing over it.

use crate::SpriteDraw;

/// One logo quad: a rect in the logo's own decoded-TIM pixel space and its
/// destination in the boot stage. The mirror of
/// `legaia_engine_title::publisher_logos::LogoQuad`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogoQuadView {
    /// `(x, y, w, h)` in the decoded TIM.
    pub src: (u32, u32, u32, u32),
    /// `(x, y, w, h)` in the boot stage.
    pub dst: (i32, i32, u32, u32),
}

/// Build the sprite draws for one publisher logo.
///
/// * `quads` - that logo's rows of `LOGO_QUADS` (PROKION and SCEA are
///   vertically packed and come back as two quads each; WARNING comes back
///   empty, because no site in PROT 0895 draws it).
/// * `atlas_rect` - where this logo's TIM sits inside the stacked boot atlas
///   (`LogosAtlas::rects[idx]`).
/// * `stage` - the retail stage the `dst` rects are authored in
///   (`publisher_logos::STAGE`).
/// * `alpha` - the session's fade level, `0.0 ..= 1.0`.
///
/// The stage is fitted into the surface **integer-scaled** so the logos stay
/// crisp, then centred; a surface smaller than the stage (the 320x240 native
/// target) takes the exact ratio, so the whole 640x480 boot frame fits it as
/// it fits a 320x240 display. Each source rect is clipped to the decoded TIM -
/// a descriptor's `w`/`h` can name the last row of a strip that is not there.
pub fn publisher_logo_sprite_draws(
    quads: &[LogoQuadView],
    atlas_rect: (u32, u32, u32, u32),
    stage: (u32, u32),
    alpha: f32,
    surface_w: u32,
    surface_h: u32,
) -> Vec<SpriteDraw> {
    let (atlas_x, atlas_y, atlas_w, atlas_h) = atlas_rect;
    if quads.is_empty() || atlas_w == 0 || atlas_h == 0 || stage.0 == 0 || stage.1 == 0 {
        return Vec::new();
    }
    let fit = StageFit::new(stage, surface_w, surface_h);
    let color = [1.0, 1.0, 1.0, alpha.clamp(0.0, 1.0)];
    let mut out = Vec::with_capacity(quads.len());
    for q in quads {
        let (sx, sy, sw, sh) = q.src;
        let sw = sw.min(atlas_w.saturating_sub(sx));
        let sh = sh.min(atlas_h.saturating_sub(sy));
        if sw == 0 || sh == 0 {
            continue;
        }
        let (dx, dy, dw, dh) = q.dst;
        out.push(SpriteDraw {
            dst: fit.rect((dx, dy, dw, dh)),
            src: (atlas_x + sx, atlas_y + sy, sw, sh),
            color,
        });
    }
    out
}

/// How the stage lands on the surface: `v * num / den` from a centred origin.
///
/// A surface at least the stage's size takes the largest **integer** scale,
/// so the logos stay crisp. A smaller one - the 320x240 native target the
/// PSX render mode draws into - takes the exact ratio instead, which is what
/// the 640x480 boot frame is on a 320x240 display: drawn at the stage's own
/// size it would overflow the surface and show only its middle.
#[derive(Debug, Clone, Copy)]
struct StageFit {
    num: i64,
    den: i64,
    x0: i64,
    y0: i64,
}

impl StageFit {
    fn new(stage: (u32, u32), surface_w: u32, surface_h: u32) -> Self {
        let (st_w, st_h) = (i64::from(stage.0.max(1)), i64::from(stage.1.max(1)));
        let (sw, sh) = (i64::from(surface_w), i64::from(surface_h));
        let (num, den) = if sw >= st_w && sh >= st_h {
            ((sw / st_w).min(sh / st_h), 1)
        } else if sw * st_h <= sh * st_w {
            (sw.max(1), st_w)
        } else {
            (sh.max(1), st_h)
        };
        Self {
            num,
            den,
            x0: (sw - st_w * num / den) / 2,
            y0: (sh - st_h * num / den) / 2,
        }
    }

    fn at(&self, v: i64) -> i64 {
        (v * self.num).div_euclid(self.den)
    }

    /// A stage rect on the surface; the far edge maps on its own, so
    /// abutting rects stay abutting.
    fn rect(&self, (x, y, w, h): (i32, i32, u32, u32)) -> (i32, i32, u32, u32) {
        let (x, y) = (i64::from(x), i64::from(y));
        let (x1, y1) = (x + i64::from(w), y + i64::from(h));
        (
            (self.x0 + self.at(x)) as i32,
            (self.y0 + self.at(y)) as i32,
            (self.at(x1) - self.at(x)) as u32,
            (self.at(y1) - self.at(y)) as u32,
        )
    }
}

/// Where the publisher pass's screen wash lands on the surface: `rect`
/// (`publisher_logos::WASH_RECT`, in the stage) through the same fit as the
/// logo quads.
pub fn publisher_logo_wash_rect(
    rect: (i32, i32, u32, u32),
    stage: (u32, u32),
    surface_w: u32,
    surface_h: u32,
) -> (i32, i32, u32, u32) {
    StageFit::new(stage, surface_w, surface_h).rect(rect)
}

/// The publisher pass's screen wash as one sprite over the atlas's white
/// block (`LogosAtlas::white`), for a host that draws it through its sprite
/// overlay under an `abr = 1` (`B + F`) blend span
/// (`publisher_logos::WASH_ABR`).
///
/// Retail's quad is untextured, so `F` is its grey `level` itself. The tint
/// that reproduces it over a white texel is the grey's 5-bit value over 31:
/// the overlay's 5-bit modulation (`ui_tint`) then lands on exactly
/// `level >> 3`, the colour the GPU blends.
pub fn publisher_logo_wash_draw(
    rect: (i32, i32, u32, u32),
    stage: (u32, u32),
    white: (u32, u32, u32, u32),
    level: u8,
    surface_w: u32,
    surface_h: u32,
) -> SpriteDraw {
    let g = f32::from(level >> 3) / 31.0;
    let (wx, wy, ww, wh) = white;
    // The block's interior texel, so no filter reaches past it.
    let src = (wx + ww / 2 - 1, wy + wh / 2 - 1, 2, 2);
    SpriteDraw {
        dst: publisher_logo_wash_rect(rect, stage, surface_w, surface_h),
        src,
        color: [g, g, g, 1.0],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAGE: (u32, u32) = (640, 480);

    fn scea() -> Vec<LogoQuadView> {
        vec![
            LogoQuadView {
                src: (0, 0, 253, 64),
                dst: (68, 192, 252, 64),
            },
            LogoQuadView {
                src: (0, 64, 252, 64),
                dst: (320, 192, 252, 64),
            },
        ]
    }

    /// At exactly the stage size the draws are 1:1 and un-offset, so a
    /// retail-pinned destination lands on the pixel it names.
    #[test]
    fn a_stage_sized_surface_draws_one_to_one() {
        let d = publisher_logo_sprite_draws(&scea(), (0, 128, 253, 128), STAGE, 1.0, 640, 480);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].dst, (68, 192, 252, 64));
        assert_eq!(d[0].src, (0, 128, 253, 64));
        assert_eq!(d[1].src, (0, 192, 252, 64));
    }

    /// A double-size surface scales the destinations and centres nothing (the
    /// stage fills it); a surface between multiples letterboxes.
    #[test]
    fn the_stage_is_integer_scaled_and_centred() {
        let d = publisher_logo_sprite_draws(&scea(), (0, 0, 253, 128), STAGE, 1.0, 1280, 960);
        assert_eq!(d[0].dst, (68 * 2, 192 * 2, 252 * 2, 64 * 2));
        let d = publisher_logo_sprite_draws(&scea(), (0, 0, 253, 128), STAGE, 1.0, 1000, 700);
        // scale 1, so the 640x480 stage is centred in 1000x700.
        assert_eq!(d[0].dst.0, (1000 - 640) / 2 + 68);
        assert_eq!(d[0].dst.1, (700 - 480) / 2 + 192);
    }

    /// A surface smaller than the stage - the 320x240 native target - takes
    /// the whole stage at its exact ratio rather than showing the middle of
    /// it at native size; the pair still meets on the centre line.
    #[test]
    fn a_sub_stage_surface_takes_the_whole_stage() {
        let d = publisher_logo_sprite_draws(&scea(), (0, 0, 253, 128), STAGE, 1.0, 320, 240);
        assert_eq!(d[0].dst, (34, 96, 126, 32));
        assert_eq!(d[1].dst, (160, 96, 126, 32));
        // A sub-stage surface of another aspect letterboxes the ratio.
        let d = publisher_logo_sprite_draws(&scea(), (0, 0, 253, 128), STAGE, 1.0, 320, 400);
        assert_eq!(d[0].dst.1, (400 - 240) / 2 + 96);
        assert_eq!(
            publisher_logo_wash_rect((0, 24, 640, 436), STAGE, 320, 240),
            (0, 12, 320, 218)
        );
    }

    /// The source rect is clipped to the decoded TIM: a descriptor naming a
    /// row past the atlas rect loses it rather than sampling the next logo.
    #[test]
    fn source_rects_clip_to_the_atlas_rect() {
        // The atlas rect is only 100 rows tall, so the second quad (v = 64,
        // h = 64) keeps 36 and the first keeps all 64.
        let d = publisher_logo_sprite_draws(&scea(), (0, 0, 253, 100), STAGE, 1.0, 640, 480);
        assert_eq!(d[0].src.3, 64);
        assert_eq!(d[1].src.3, 36);
    }

    /// The wash rides the logo quads' fit, and its tint is the grey's
    /// 5-bit step.
    #[test]
    fn the_wash_takes_the_stage_fit_and_the_5_bit_grey() {
        let rect = (0, 24, 640, 436);
        assert_eq!(publisher_logo_wash_rect(rect, STAGE, 640, 480), rect);
        assert_eq!(
            publisher_logo_wash_rect(rect, STAGE, 1280, 960),
            (0, 48, 1280, 872)
        );
        assert_eq!(
            publisher_logo_wash_rect(rect, STAGE, 1000, 700).1,
            (700 - 480) / 2 + 24
        );
        let d = publisher_logo_wash_draw(rect, STAGE, (0, 500, 4, 4), 0xFF, 640, 480);
        assert_eq!(d.color, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(d.src, (1, 501, 2, 2));
        let d = publisher_logo_wash_draw(rect, STAGE, (0, 500, 4, 4), 0x14, 640, 480);
        assert_eq!(d.color[0], 2.0 / 31.0);
    }

    /// The fade level becomes the tint alpha and is clamped, and an empty
    /// quad list (WARNING) draws nothing at all.
    #[test]
    fn alpha_is_clamped_and_an_empty_logo_draws_nothing() {
        let d = publisher_logo_sprite_draws(&scea(), (0, 0, 253, 128), STAGE, 1.7, 640, 480);
        assert_eq!(d[0].color[3], 1.0);
        let d = publisher_logo_sprite_draws(&scea(), (0, 0, 253, 128), STAGE, -0.5, 640, 480);
        assert_eq!(d[0].color[3], 0.0);
        assert!(
            publisher_logo_sprite_draws(&[], (0, 0, 253, 128), STAGE, 1.0, 640, 480).is_empty()
        );
    }
}
