//! The publisher-logo boot pass's **draw** half: retail's per-logo quads
//! letterboxed onto a surface.
//!
//! The layout itself is not computed here - `legaia_engine_core::publisher_logos`
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
/// `legaia_engine_core::publisher_logos::LogoQuad`.
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
/// crisp, then centred; `max(1)` keeps a surface smaller than the stage
/// rendering at native size. Each source rect is clipped to the decoded TIM -
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
    let scale = (surface_w / stage.0).min(surface_h / stage.1).max(1);
    let stage_x0 = (surface_w as i32 - (stage.0 * scale) as i32) / 2;
    let stage_y0 = (surface_h as i32 - (stage.1 * scale) as i32) / 2;
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
            dst: (
                stage_x0 + dx * scale as i32,
                stage_y0 + dy * scale as i32,
                dw * scale,
                dh * scale,
            ),
            src: (atlas_x + sx, atlas_y + sy, sw, sh),
            color,
        });
    }
    out
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

    /// A surface smaller than the stage still renders at native size rather
    /// than collapsing to a zero scale.
    #[test]
    fn a_sub_stage_surface_keeps_native_size() {
        let d = publisher_logo_sprite_draws(&scea(), (0, 0, 253, 128), STAGE, 1.0, 320, 240);
        assert_eq!(d[0].dst.2, 252);
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
