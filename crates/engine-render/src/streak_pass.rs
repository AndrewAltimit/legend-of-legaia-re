//! Per-frame move-FX streak emitter - the pass that turns the battle
//! context's projection block into [`crate::afterimage`] quads.
//!
//! REF: FUN_801e1ab0 - the retail per-call emitter this pass drives. The
//! packet arithmetic (jitter, brightness band, UVs, CLUT, texpage) is ported
//! in [`crate::afterimage`]; this module supplies its two inputs and its
//! screen-space corners once per frame.
//!
//! ## What this closes
//!
//! [`crate::afterimage`] was inert for two reasons, and this module is the
//! second half of the answer to both:
//!
//! * **The projection inputs had no producer.** Retail reads the billboard
//!   centre from `ctx[+0x1144]` and the half-width from `ctx[+0x6C6] - 0x200`,
//!   both written by the action effect script's terminator
//!   (`FUN_801DEA50`, `0x801DF290` / `0x801DF2B4`). Those writes now have a
//!   sink: `legaia_engine_core::action_effect_script::MoveFxStreak`, installed
//!   by the live battle tick and read back through `World::move_fx_streak`.
//! * **Nothing emitted a per-frame pass.** `World::active_move_fx_trail_texpage`
//!   was read only as a log line. [`streak_quads`] is the pass; the native
//!   window's screen-FX builder calls it every battle frame and appends the
//!   quads to the same screen-space textured batch the widget overlays ride.
//!
//! ## Projection: the engine camera, not the GTE
//!
//! Retail projects the billboard through `FUN_800195a8` - one GTE `MVMVA`
//! for the centre, four 16-bit corner adds in view space, then `RTPT`. That
//! path is ported as [`crate::billboard::project_billboard`] and needs a GTE
//! rotation matrix + translation vector, which the engine's battle camera
//! does not carry (it is a `glam` MVP). [`project_streak_corners_mvp`]
//! therefore does the same *operation* against the engine camera: it projects
//! the centre, takes the screen-space gradient of the MVP at that point, and
//! fans the four corners out along the screen axes - which is what
//! "camera-facing quad of half-size `(hw, hh)`" means under any projection.
//!
//! So the **placement** is engine-side and the **packet** is retail: corner
//! order, jitter law, brightness band, UVs, CLUT and texpage all come
//! unchanged from [`crate::afterimage::build_afterimage_quad`]. When the
//! battle camera grows a GTE form, swap [`project_streak_corners_mvp`] for
//! [`crate::afterimage::project_streak_corners`] and nothing else moves.
//!
//! ## Which sibling this drives
//!
//! Both. The retail move-FX draw dispatcher reaches two emitters -
//! `0x801E0CA0` calls the single-quad afterimage, `0x801E0CD0` the chained
//! ribbon (`FUN_801E1D98`, [`crate::afterimage::build_streak_ribbon`]) - and
//! the selector is decoded: the streak counter `ctx[+0x6C6]` (walked down 4
//! per frame by the phase driver, `MoveFxStreak::tick_counter` engine-side)
//! schedules the handoff. [`streak_quads_scheduled`] is the ported
//! dispatcher: party afterimage at `counter >= 0x281`, ribbon below
//! `0x201`, monster ribbon always.

use crate::afterimage::{AfterimageQuad, SCREEN_Y_OFFSET, build_afterimage_quad};
use glam::{Mat4, Vec3, Vec4};

/// Retail stage width in pixels - the frame the projected corners live in.
pub const STAGE_W: f32 = 320.0;
/// Retail stage height in pixels.
pub const STAGE_H: f32 = 240.0;

/// Billboard half-height handed to the projector, matching
/// [`crate::afterimage::PROJECTION_HALF_SIZE`].
pub const HALF_HEIGHT: f32 = 256.0;

/// Everything one frame's streak needs, lifted out of the engine world.
///
/// Hosts build this from `World::move_fx_streak()` (the launch point and the
/// half-width) plus `World::active_move_fx_trail_texpage()` (the trail id).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreakSource {
    /// `ctx[+0x1144]` - the world-space launch position the streak centres
    /// on, before the [`SCREEN_Y_OFFSET`] push.
    pub launch: [f32; 3],
    /// `ctx[+0x6C6] - 0x200` - the billboard half-width in world units.
    pub half_width: f32,
    /// The trail texture id - the low byte of the GP0 CLUT word
    /// `0x7700 + id` the move-power record's `+0x0B` field selects.
    pub trail_id: u8,
}

impl StreakSource {
    /// Build from the engine's context block and the active move's trail
    /// texpage word (`0x7700 + id`, as `World::active_move_fx_trail_texpage`
    /// reports it). Returns `None` when the block is not armed - i.e. no
    /// terminator has staged a launch point this action.
    pub fn from_block(
        launch: Option<(i32, i32, i32)>,
        half_width: i16,
        trail_texpage: Option<u16>,
    ) -> Option<Self> {
        let (x, y, z) = launch?;
        Some(Self {
            launch: [x as f32, y as f32, z as f32],
            half_width: f32::from(half_width),
            // The CLUT base is `0x7700`; the id is what
            // `build_afterimage_quad` adds back. A missing texpage means the
            // move staged no 3D scene, so the trail falls back to column 0.
            trail_id: trail_texpage.map(|w| (w & 0xFF) as u8).unwrap_or(0),
        })
    }
}

/// Project the four streak-quad corners into the 320x240 stage under an
/// arbitrary MVP, camera-facing, in the retail vertex order
/// (`TL, TR, BL, BR` - the order `FUN_801E1AB0` wires straight into the
/// `POLY_FT4`'s `xy0..xy3` slots).
///
/// `centre` is the world point **before** the retail `+0x120` Y push; this
/// function applies it, exactly as
/// [`crate::afterimage::project_streak_corners`] does for the GTE path.
///
/// Returns `None` when the centre is at or behind the near plane (clip
/// `w <= 0`) - retail smears such a quad across the screen, which is a PSX
/// artifact worth not reproducing in a pass that draws over a clean scene.
pub fn project_streak_corners_mvp(
    mvp: &Mat4,
    centre: [f32; 3],
    half_w: f32,
    half_h: f32,
) -> Option<[(i16, i16); 4]> {
    project_corners_mvp(mvp, centre, half_w, half_h, f32::from(SCREEN_Y_OFFSET))
}

/// The ribbon caller's projection (`FUN_801E1D98` via `0x801E0CD4`): the
/// same camera-facing fan at the **constant** half sizes
/// ([`crate::afterimage::RIBBON_PROJECTION_HALF_WIDTH`] /
/// [`crate::afterimage::RIBBON_PROJECTION_HALF_HEIGHT`]) and **no** `+0x120`
/// Y push - the ribbon anchors on the launch point itself.
pub fn project_ribbon_corners_mvp(mvp: &Mat4, centre: [f32; 3]) -> Option<[(i16, i16); 4]> {
    project_corners_mvp(
        mvp,
        centre,
        f32::from(crate::afterimage::RIBBON_PROJECTION_HALF_WIDTH),
        f32::from(crate::afterimage::RIBBON_PROJECTION_HALF_HEIGHT),
        0.0,
    )
}

fn project_corners_mvp(
    mvp: &Mat4,
    centre: [f32; 3],
    half_w: f32,
    half_h: f32,
    y_push: f32,
) -> Option<[(i16, i16); 4]> {
    let p = Vec3::new(centre[0], centre[1] + y_push, centre[2]);
    let clip = *mvp * Vec4::new(p.x, p.y, p.z, 1.0);
    if clip.w <= 1e-4 {
        return None;
    }
    let ndc_x = clip.x / clip.w;
    let ndc_y = clip.y / clip.w;

    // Screen-space gradient of the projection at `p`. For
    // `x_ndc = (r0 . p + t0) / (rw . p + tw)`, `d x_ndc / d p` is
    // `(r0 - x_ndc * rw) / w`; its magnitude is the NDC displacement a unit
    // world offset produces along the screen-X axis. That is precisely the
    // "camera-facing half-extent" the GTE gets for free by fanning the
    // corners out in view space before the divide.
    let m = mvp.to_cols_array_2d();
    let row = |r: usize| Vec3::new(m[0][r], m[1][r], m[2][r]);
    let (r0, r1, rw) = (row(0), row(1), row(3));
    let gx = (r0 - ndc_x * rw) / clip.w;
    let gy = (r1 - ndc_y * rw) / clip.w;

    let cx = (ndc_x * 0.5 + 0.5) * STAGE_W;
    let cy = (0.5 - ndc_y * 0.5) * STAGE_H;
    let hw_px = gx.length() * half_w.abs() * 0.5 * STAGE_W;
    let hh_px = gy.length() * half_h.abs() * 0.5 * STAGE_H;

    let clamp = |v: f32| v.clamp(-4096.0, 4096.0) as i16;
    let (l, r) = (clamp(cx - hw_px), clamp(cx + hw_px));
    let (t, b) = (clamp(cy - hh_px), clamp(cy + hh_px));
    Some([(l, t), (r, t), (l, b), (r, b)])
}

/// A tiny deterministic uniform source for the packet jitter.
///
/// Retail draws from the BIOS `rand()` LCG (`FUN_80056798`), whose stream is
/// shared with the rest of the frame and is not reproducible outside a
/// capture. The pass seeds a private generator per frame instead, so the
/// shimmer is stable for a given frame number and the pass stays a pure
/// function of world state + frame - which is what the determinism replay
/// harness needs.
fn frame_rng(seed: u32) -> impl FnMut() -> u32 {
    // BIOS `rand()` returns 0..0x7FFF; the ported jitter only consumes low
    // bits and small moduli, so any uniform source of that width works.
    let mut s = seed.wrapping_mul(0x9E37_79B9).wrapping_add(0x1234_5678) | 1;
    move || {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        s >> 17
    }
}

/// Emit this frame's streak quads.
///
/// Empty when the source does not project (behind the camera) - the same
/// "nothing to link" outcome retail reaches when its projector returns a
/// degenerate quad.
pub fn streak_quads(src: &StreakSource, mvp: &Mat4, frame: u32) -> Vec<AfterimageQuad> {
    let Some(corners) = project_streak_corners_mvp(mvp, src.launch, src.half_width, HALF_HEIGHT)
    else {
        return Vec::new();
    };
    vec![build_afterimage_quad(
        corners,
        src.trail_id,
        frame_rng(frame),
    )]
}

/// The single-quad afterimage draws while the streak counter `ctx[+0x6C6]`
/// is at least this (`slti 0x281` at `0x801E0C8C`) - party actors only.
pub const AFTERIMAGE_COUNTER_MIN: u16 = 0x281;
/// The chained ribbon takes over once the counter falls below this
/// (`slti 0x201` at `0x801E0CBC`); a monster actor draws the ribbon at any
/// counter (the `0x801E0C7C` branch).
pub const RIBBON_COUNTER_MAX: u16 = 0x201;

/// Emit this frame's streak per the retail emitter schedule - the decoded
/// move-FX draw dispatcher (`FUN_801E09F8` phase 1, `0x801E0C64..0x801E0CE8`):
///
/// * a **party** acting actor draws the single-billboard afterimage while
///   `counter >= 0x281` (its half-width is `counter - 0x200`, shrinking 4
///   per frame), nothing through `0x280..0x201`, and the chained ribbon
///   once `counter < 0x201`;
/// * a **monster** acting actor draws the ribbon at every counter value.
///
/// The ribbon is `FUN_801E1D98` ported as
/// [`crate::afterimage::build_streak_ribbon`], projected at its constant
/// half sizes with no Y push.
pub fn streak_quads_scheduled(
    src: &StreakSource,
    mvp: &Mat4,
    frame: u32,
    counter: u16,
    party: bool,
) -> Vec<AfterimageQuad> {
    if party && counter >= AFTERIMAGE_COUNTER_MIN {
        return streak_quads(src, mvp, frame);
    }
    if party && counter >= RIBBON_COUNTER_MAX {
        // The retail dead band between the two emitters: nothing draws.
        return Vec::new();
    }
    let Some(corners) = project_ribbon_corners_mvp(mvp, src.launch) else {
        return Vec::new();
    };
    crate::afterimage::build_streak_ribbon(corners, src.trail_id, frame_rng(frame))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain perspective * look-at facing -Z from `+z`, the shape the
    /// battle camera has.
    fn cam() -> Mat4 {
        Mat4::perspective_rh(1.0, 4.0 / 3.0, 1.0, 10_000.0)
            * Mat4::look_at_rh(
                Vec3::new(0.0, 0.0, 1000.0),
                Vec3::ZERO,
                Vec3::new(0.0, 1.0, 0.0),
            )
    }

    #[test]
    fn source_needs_an_armed_block() {
        assert!(StreakSource::from_block(None, 0x100, Some(0x7703)).is_none());
        let s = StreakSource::from_block(Some((1, 2, 3)), 0x100, Some(0x7703)).unwrap();
        assert_eq!(s.launch, [1.0, 2.0, 3.0]);
        assert_eq!(s.half_width, 256.0);
        // The trail id is the CLUT word's low byte - the same id
        // `build_afterimage_quad` adds back onto `CLUT_BASE`.
        assert_eq!(s.trail_id, 3);
        // No staged scene: column 0.
        let bare = StreakSource::from_block(Some((0, 0, 0)), 0x100, None).unwrap();
        assert_eq!(bare.trail_id, 0);
    }

    #[test]
    fn corners_are_in_the_retail_tl_tr_bl_br_order() {
        let c =
            project_streak_corners_mvp(&cam(), [0.0, -f32::from(SCREEN_Y_OFFSET), 0.0], 64.0, 64.0)
                .expect("in front of the camera");
        // TL / TR share a Y, BL / BR share a Y, TL / BL share an X.
        assert_eq!(c[0].1, c[1].1);
        assert_eq!(c[2].1, c[3].1);
        assert_eq!(c[0].0, c[2].0);
        assert_eq!(c[1].0, c[3].0);
        // Top above bottom, left of right (stage Y grows downward).
        assert!(c[0].1 < c[2].1);
        assert!(c[0].0 < c[1].0);
    }

    #[test]
    fn the_y_push_displaces_the_quad_along_screen_y_only() {
        // Retail adds 0x120 to the centre's world Y before projecting
        // (`0x801E1AF8`). The direction on screen is the camera's business -
        // this fixture's look-at is Y-up while the engine's battle frame is
        // retail's Y-down - so what is asserted is that the push lands, that
        // it is purely vertical, and that it is worth the whole `0x120`.
        let m = cam();
        let pushed = project_streak_corners_mvp(&m, [0.0, 0.0, 0.0], 32.0, 32.0).unwrap();
        let unpushed =
            project_streak_corners_mvp(&m, [0.0, -f32::from(SCREEN_Y_OFFSET), 0.0], 32.0, 32.0)
                .unwrap();
        assert_ne!(pushed, unpushed, "the +0x120 push did nothing");
        for (p, u) in pushed.iter().zip(unpushed.iter()) {
            assert_eq!(p.0, u.0, "the push moved the quad sideways");
        }
        let dy = (pushed[0].1 - unpushed[0].1).abs();
        assert!(dy > 40, "push displaced only {dy} px");
    }

    #[test]
    fn half_width_scales_the_quad_and_a_negative_word_still_draws() {
        let m = cam();
        let narrow = project_streak_corners_mvp(&m, [0.0, 0.0, 0.0], 32.0, 64.0).unwrap();
        let wide = project_streak_corners_mvp(&m, [0.0, 0.0, 0.0], 256.0, 64.0).unwrap();
        assert!(wide[1].0 - wide[0].0 > narrow[1].0 - narrow[0].0);
        // `ctx[+0x6C6] - 0x200` is a signed subtract, so a small counter
        // yields a negative half-width; the quad keeps its extent.
        let neg = project_streak_corners_mvp(&m, [0.0, 0.0, 0.0], -256.0, 64.0).unwrap();
        assert_eq!(neg, wide);
    }

    #[test]
    fn a_centre_behind_the_camera_emits_nothing() {
        let m = cam();
        // The camera sits at z = +1000 looking toward -Z; put the point well
        // behind it.
        assert!(project_streak_corners_mvp(&m, [0.0, 0.0, 5000.0], 64.0, 64.0).is_none());
        let src = StreakSource {
            launch: [0.0, 0.0, 5000.0],
            half_width: 64.0,
            trail_id: 0,
        };
        assert!(streak_quads(&src, &m, 0).is_empty());
    }

    #[test]
    fn the_pass_emits_one_retail_packet_per_frame() {
        let src = StreakSource {
            launch: [0.0, -f32::from(SCREEN_Y_OFFSET), 0.0],
            half_width: 128.0,
            trail_id: 0x0B,
        };
        let q = streak_quads(&src, &cam(), 7);
        assert_eq!(q.len(), 1);
        // The packet fields are the ported retail ones, unchanged.
        assert_eq!(q[0].clut, crate::afterimage::CLUT_BASE + 0x0B);
        assert_eq!(q[0].tpage, crate::afterimage::TEXPAGE);
        assert_eq!(q[0].color, crate::afterimage::MODULATION_COLOR);
        assert!(q[0].semi_transparent);
        // The brightness band picks one of the four 0x20-wide sub-columns.
        let band = q[0].uv[2].0;
        assert!(
            matches!(band, 0x00 | 0x20 | 0x40 | 0x60),
            "band {band:#04x}"
        );
        assert_eq!(q[0].uv[0].0, band | 0x1f);
    }

    #[test]
    fn the_shimmer_is_deterministic_per_frame_and_moves_between_frames() {
        let src = StreakSource {
            launch: [0.0, -f32::from(SCREEN_Y_OFFSET), 0.0],
            half_width: 128.0,
            trail_id: 0,
        };
        let m = cam();
        assert_eq!(streak_quads(&src, &m, 42), streak_quads(&src, &m, 42));
        // Over a short run the jitter has to actually vary, or the "streak"
        // is a static quad.
        let frames: Vec<_> = (0..16).map(|f| streak_quads(&src, &m, f)).collect();
        assert!(frames.windows(2).any(|w| w[0] != w[1]));
    }
}
