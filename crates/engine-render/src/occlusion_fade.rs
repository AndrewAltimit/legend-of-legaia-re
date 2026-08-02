//! Camera-occlusion fade - the opt-in **see-through-walls enhancement**
//! (NON-RETAIL): scene fragments that sit between the camera and the
//! player-controlled character dissolve to a screen-door dither so the
//! character stays visible behind walls, roofs and props.
//!
//! Retail has no such pass - the field follow camera is authored so the
//! player is usually framed clear, but plenty of scenes still put geometry
//! between the lens and the character. The enhancement is a pure
//! presentation knob: it never feeds the simulation, and with the enable
//! off every mesh path is bit-identical to the faithful render.
//!
//! Mechanism (per fragment, in the scene VRAM / colour mesh shaders):
//!
//! 1. The host stages the player's clip-space position under the frame's
//!    scene camera ([`crate::Renderer::set_occlusion_focus`]); the renderer
//!    projects it to framebuffer pixels + view depth into the per-frame
//!    scene-lights uniform (group 2, always staged).
//! 2. A fragment fades only when **both** hold:
//!    * it is nearer the camera than the player by more than
//!      [`OCCL_DEPTH_MARGIN`] view units (so the floor at the player's feet,
//!      the player mesh itself, and geometry behind it never fade), and
//!    * its framebuffer position lies inside the fade circle around the
//!      player's projected centre, so distant parts of the same wall stay
//!      solid. The circle's radius is [`OCCL_RADIUS_WORLD`] **world units**,
//!      projected through the frame's own camera ([`radius_px`]) - it is
//!      sized in the space the character lives in, not in screen space, so
//!      it keeps hugging them as the camera pushes in and pulls out.
//! 3. The fade is a **screen-door discard** against a 4x4 Bayer threshold
//!    matrix ([`bayer_threshold`], the WGSL twin is `occl_bayer`): the keep
//!    probability ramps from 1.0 at the circle's rim down to
//!    [`OCCL_MIN_KEEP`] at the centre over an [`OCCL_FEATHER_FRAC_OF_RADIUS`]
//!    feather. Discarding in the opaque pass keeps depth writes and the
//!    per-ABR blend pass untouched - no new pipelines, no sorting, and the
//!    dithered look sits naturally next to the PSX ordered dither.
//!
//! A per-fragment test on purpose: the browser play page once shipped a
//! per-BODY occluder cull (lens->player segment vs whole-placement AABBs)
//! and had to disable it - terrain tiles / walls span whole-scene boxes, so
//! neighbouring bodies blinked out as the camera orbited
//! (`site/js/play-app.js` `OCCLUDER_CULL`, docs/subsystems/renderer.md).
//! Fragment granularity has no such failure mode.
//!
//! This module is the CPU mirror of the WGSL (`occl_bayer` in the shader
//! prelude, `occl_keep` in the scene-lights layer), kept in lockstep like
//! [`crate::psx_dither`] / [`crate::dyn_light`].

/// Fade-circle radius in **world units** around the focus point - the
/// quantity the hole is really made of. [`radius_px`] projects it through
/// the frame's own camera, so the opening covers the same amount of *world*
/// at every camera distance.
///
/// A fraction of the viewport is the obvious alternative and it is wrong:
/// being screen-relative it is zoom-invariant by construction, so one value
/// cannot serve both framings. Tuned close up it dissolves half the scene
/// when the camera pulls back; tuned far away - which is how it was tuned -
/// it shrinks to a peephole barely wider than the character's head as the
/// camera pushes in, because the character grows on screen and the hole
/// does not.
///
/// `250.0` is about two character heights (the field character is ~130 units
/// tall), i.e. an opening roughly four characters wide. One character height
/// is what the old 0.12-of-viewport-height tuning worked out to at the
/// distance it was tuned at, and it read as too tight in play - the wall
/// opened around the character but not around what they were walking toward.
/// GLSL/WGSL twin: staged into `occl_params.x` already in pixels - the
/// projection happens host-side, so neither shader changes.
pub const OCCL_RADIUS_WORLD: f32 = 250.0;

/// Width of the rim feather (keep probability ramps 1.0 -> [`OCCL_MIN_KEEP`]
/// over this band), as a fraction of the **radius**. Relative to the radius
/// rather than to the viewport so the feather can never grow wider than the
/// hole it is feathering - which would invert the ramp in
/// [`keep_probability`] once the camera pulled far enough back.
pub const OCCL_FEATHER_FRAC_OF_RADIUS: f32 = 0.42;

/// Lower clamp on [`radius_px`], as a fraction of the viewport height.
/// A guard, not tuning: the projected radius vanishes as the focus recedes,
/// and a hole smaller than this is indistinguishable from no fade at all.
pub const OCCL_RADIUS_MIN_FRAC: f32 = 0.04;

/// Upper clamp on [`radius_px`], as a fraction of the viewport height.
/// The other guard: the `1/z` projection diverges as the camera approaches
/// the focus, and a lens practically inside the character would otherwise
/// dissolve the entire frame. Deliberately loose - the play page's tightest
/// zoom projects to ~0.57 of the viewport height, so a clamp near that value
/// would quietly become tuning, capping the close-up hole and re-introducing
/// the zoom dependence the world-space radius exists to remove. Both clamps
/// bite only on cameras no follow rig produces.
pub const OCCL_RADIUS_MAX_FRAC: f32 = 0.9;

/// Keep probability at the circle centre - the "mostly transparent" floor.
/// `0.25` keeps 4 of every 16 screen-door pixels. WGSL twin: `occl_params.y`.
pub const OCCL_MIN_KEEP: f32 = 0.25;

/// A fragment must be nearer the camera than the player by more than this
/// many view-space units to fade. The player's own mesh (and every other
/// actor) is protected by the per-draw watermark
/// (`Renderer::set_occlusion_env_draws` -> `MeshUniforms.flags[2]`), NOT by
/// this margin - so it only needs to guard environment geometry AT the
/// focus depth (the floor tier and coplanar decals around the character's
/// feet), and an occluder hugging the character (a plant stalk or inner
/// wall right in front of them) still opens up. Earlier values (150, then
/// 70) doubled as the player-mesh shield and protected hugging walls as if
/// they were the player - the "only the nearest of several stacked
/// occluders fades" report. WGSL twin: `occl_params.z`.
pub const OCCL_DEPTH_MARGIN: f32 = 16.0;

/// The projection's vertical scale `P[1][1]` (`1 / tan(fov_y / 2)`),
/// recovered from a **column-major** view-projection - glam's
/// `Mat4::to_cols_array`, and the same flat layout `site/js/webgl-math.js`
/// builds.
///
/// `view_proj = P * V` with `V` a rigid transform, so the product's second
/// row is `P[1][1]` times the view's *unit* up axis: its length is the
/// scale. That identity is what lets both hosts derive the scale from the
/// matrix they already hold, instead of threading a camera basis down to
/// the renderer. It survives the browser's retail screen-X mirror
/// (`P[0] = -P[0]`), which touches only the first row.
///
/// JS twin: `occlProjScaleY` in `site/js/webgl-math.js`.
pub fn view_proj_scale_y(view_proj: &[f32; 16]) -> f32 {
    // Column-major index = col * 4 + row, so row 1 is 1, 5, 9 (13 is the
    // translation, which the scale does not live in).
    let (a, b, c) = (view_proj[1], view_proj[5], view_proj[9]);
    (a * a + b * b + c * c).sqrt()
}

/// Fade-circle radius in framebuffer pixels: [`OCCL_RADIUS_WORLD`] projected
/// at the focus's view depth, clamped to [`OCCL_RADIUS_MIN_FRAC`] ..
/// [`OCCL_RADIUS_MAX_FRAC`] of the viewport height.
///
/// A world length `L` perpendicular to the view axis at view depth `z` spans
/// `L * proj_scale_y / z` in NDC, and NDC's `-1..1` covers `viewport_h`
/// pixels - hence the halved viewport factor. `focus_view_z` is the staged
/// focus's clip `w`; `proj_scale_y` comes from [`view_proj_scale_y`].
/// Degenerate inputs (focus at or behind the lens, a non-finite depth, an
/// orthographic-shaped scale of zero) fall back to the floor rather than to
/// a NaN that would propagate into the uniform.
///
/// JS twin: `occlRadiusPx` in `site/js/webgl-shaders.js`.
pub fn radius_px(focus_view_z: f32, proj_scale_y: f32, viewport_h: f32) -> f32 {
    let lo = OCCL_RADIUS_MIN_FRAC * viewport_h;
    let hi = OCCL_RADIUS_MAX_FRAC * viewport_h;
    if !focus_view_z.is_finite() || focus_view_z <= 1e-3 || proj_scale_y <= 0.0 {
        return lo;
    }
    (OCCL_RADIUS_WORLD * proj_scale_y * viewport_h / (2.0 * focus_view_z)).clamp(lo, hi)
}

/// Rim-feather width in pixels for a circle of [`radius_px`] - the band the
/// keep probability ramps across. Always a proper fraction of the radius, at
/// every camera distance.
pub fn feather_px(radius_px: f32) -> f32 {
    radius_px * OCCL_FEATHER_FRAC_OF_RADIUS
}

/// 4x4 Bayer threshold for the screen-door pattern, in `[0, 1)`:
/// `(bayer[y&3][x&3] + 0.5) / 16`. WGSL twin: `occl_bayer` in the shader
/// prelude (kept in lockstep by the tests below).
pub fn bayer_threshold(x: u32, y: u32) -> f32 {
    const BAYER: [[f32; 4]; 4] = [
        [0.0, 8.0, 2.0, 10.0],
        [12.0, 4.0, 14.0, 6.0],
        [3.0, 11.0, 1.0, 9.0],
        [15.0, 7.0, 13.0, 5.0],
    ];
    (BAYER[(y & 3) as usize][(x & 3) as usize] + 0.5) / 16.0
}

/// Keep probability for one fragment - the CPU mirror of the WGSL
/// `occl_keep`. `1.0` = never discard (the identity); the fragment is
/// discarded when `bayer_threshold(x, y) >= keep`.
///
/// `frag_view_z` / `focus_view_z` are view-space depths (the fragment
/// shader recovers its own as `1 / @builtin(position).w`); `focus_px` is
/// the player's projected framebuffer pixel; the `radius_px` /
/// `feather_px` arguments come from the functions of the same names.
/// `strength` (0..1) is the host's eased visibility-gate output: the
/// geometric keep blends toward the identity by it, so the screen-door
/// dissolves in and out instead of popping.
#[allow(clippy::too_many_arguments)]
pub fn keep_probability(
    frag_px: [f32; 2],
    frag_view_z: f32,
    focus_px: [f32; 2],
    focus_view_z: f32,
    radius_px: f32,
    feather_px: f32,
    min_keep: f32,
    depth_margin: f32,
    strength: f32,
) -> f32 {
    if strength < 0.004 {
        return 1.0;
    }
    // Only fragments in front of the player (nearer the camera by more
    // than the margin) can be occluding it.
    if frag_view_z >= focus_view_z - depth_margin {
        return 1.0;
    }
    let dx = frag_px[0] - focus_px[0];
    let dy = frag_px[1] - focus_px[1];
    let d = (dx * dx + dy * dy).sqrt();
    if d >= radius_px {
        return 1.0;
    }
    // Radial feather: min_keep at the centre, 1.0 at the rim; then the
    // strength blend toward the identity.
    let t = smoothstep(radius_px - feather_px, radius_px, d);
    let k = min_keep + (1.0 - min_keep) * t;
    1.0 - strength * (1.0 - k)
}

/// WGSL-equivalent `smoothstep` (identical clamped Hermite).
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The WGSL prelude's `occl_bayer` array, flattened row-major - the
    /// lockstep source of truth for [`bayer_threshold`].
    #[test]
    fn bayer_matches_wgsl_table() {
        let wgsl: [f32; 16] = [
            0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0,
        ];
        for y in 0..4u32 {
            for x in 0..4u32 {
                let expect = (wgsl[(y * 4 + x) as usize] + 0.5) / 16.0;
                assert_eq!(bayer_threshold(x, y), expect);
            }
        }
        // Tiling: coordinates repeat mod 4.
        assert_eq!(bayer_threshold(0, 0), bayer_threshold(4, 8));
    }

    /// Every threshold is unique and strictly inside (0, 1): keep = 1.0
    /// never discards a pixel, keep = 0.0 discards all 16.
    #[test]
    fn bayer_is_a_16_level_uniform_lattice() {
        let mut seen: Vec<f32> = (0..16).map(|i| bayer_threshold(i % 4, i / 4)).collect();
        seen.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for (i, v) in seen.iter().enumerate() {
            let expect = (i as f32 + 0.5) / 16.0;
            assert!((v - expect).abs() < 1e-6, "level {i}: {v} != {expect}");
        }
        assert!(seen[15] < 1.0);
        assert!(seen[0] > 0.0);
    }

    /// A keep probability of k keeps exactly round(16k) of the 16 lattice
    /// pixels - the screen-door density really is the probability.
    #[test]
    fn keep_probability_sets_screen_door_density() {
        for (keep, expect_kept) in [(1.0f32, 16), (0.25, 4), (0.5, 8), (0.0, 0)] {
            let kept = (0..16)
                .filter(|i| bayer_threshold(i % 4, i / 4) < keep)
                .count();
            assert_eq!(kept, expect_kept, "keep {keep}");
        }
    }

    // The reference framing the world radius is calibrated against: the
    // field follow camera's 60-degree vertical FOV, ~900 units out, into a
    // 720-pixel-tall viewport - where a 130-unit character stands ~12.5% of
    // the frame. Derived, not restated: a hard-coded pixel copy silently
    // stops testing the shipped value the moment a constant is retuned.
    const VH: f32 = 720.0;
    const REF_SCALE_Y: f32 = 1.732_050_8; // 1 / tan(30 deg)
    const REF_DEPTH: f32 = 900.6;
    const FOCUS: [f32; 2] = [480.0, 360.0];
    const FOCUS_Z: f32 = 1800.0;

    fn r() -> f32 {
        radius_px(FOCUS_Z, REF_SCALE_Y, VH)
    }
    fn f() -> f32 {
        feather_px(r())
    }

    fn keep_at(frag_px: [f32; 2], frag_z: f32) -> f32 {
        keep_probability(
            frag_px,
            frag_z,
            FOCUS,
            FOCUS_Z,
            r(),
            f(),
            OCCL_MIN_KEEP,
            OCCL_DEPTH_MARGIN,
            1.0,
        )
    }

    /// What the world radius comes out to at ordinary follow distance. The
    /// screen-fraction model this replaced was tuned to `0.12` there and
    /// played too tight, so the world radius is deliberately twice that -
    /// this test is what says so, and what catches a future edit to
    /// [`OCCL_RADIUS_WORLD`] that changes how the scene reads at the one
    /// distance most play happens at.
    #[test]
    fn world_radius_doubles_the_old_tuning_at_the_reference_framing() {
        let frac = radius_px(REF_DEPTH, REF_SCALE_Y, VH) / VH;
        assert!(
            (frac - 0.24).abs() < 0.004,
            "radius {frac} of viewport height at the reference framing, want 0.24"
        );
    }

    /// The defect this model fixes: the hole must grow as the camera pushes
    /// in. Halving the distance to the focus doubles both the character's
    /// projected size and the radius, so the opening stays the same multiple
    /// of the character at every zoom - which a viewport fraction, constant
    /// here by construction, could never do.
    #[test]
    fn radius_tracks_camera_distance() {
        let far = radius_px(REF_DEPTH, REF_SCALE_Y, VH);
        let near = radius_px(REF_DEPTH / 2.0, REF_SCALE_Y, VH);
        assert!(
            (near - 2.0 * far).abs() < 1e-3,
            "close-up radius {near} is not twice the far radius {far}"
        );
        // Both sit inside the clamps, so the doubling above is the formula
        // and not two saturated values happening to differ.
        assert!(near < OCCL_RADIUS_MAX_FRAC * VH);
        assert!(far > OCCL_RADIUS_MIN_FRAC * VH);
    }

    /// Clamps bound both ends, and degenerate cameras land on the floor
    /// rather than pushing a NaN into the uniform.
    #[test]
    fn radius_is_clamped_at_both_ends() {
        let lo = OCCL_RADIUS_MIN_FRAC * VH;
        assert_eq!(radius_px(0.5, REF_SCALE_Y, VH), OCCL_RADIUS_MAX_FRAC * VH);
        assert_eq!(radius_px(1.0e6, REF_SCALE_Y, VH), lo);
        assert_eq!(radius_px(0.0, REF_SCALE_Y, VH), lo);
        assert_eq!(radius_px(-10.0, REF_SCALE_Y, VH), lo);
        assert_eq!(radius_px(f32::NAN, REF_SCALE_Y, VH), lo);
        assert_eq!(radius_px(REF_DEPTH, 0.0, VH), lo);
    }

    /// The feather is a proper sub-band of the radius at every distance. If
    /// it could reach or exceed the radius, `keep_probability`'s smoothstep
    /// would invert and the rim would harden instead of feathering.
    #[test]
    fn feather_stays_inside_the_radius() {
        for z in [1.0f32, 100.0, REF_DEPTH, 5_000.0, 1.0e6] {
            let rad = radius_px(z, REF_SCALE_Y, VH);
            let fea = feather_px(rad);
            assert!(
                fea > 0.0 && fea < rad,
                "view depth {z}: feather {fea} vs radius {rad}"
            );
        }
    }

    /// `view_proj_scale_y` recovers `1 / tan(fov_y / 2)` from a real
    /// `projection * view` product at any camera orientation. Both hosts
    /// lean on this identity to size the circle from the matrix they already
    /// have, so it is worth pinning against glam rather than asserting the
    /// arithmetic against itself.
    #[test]
    fn view_proj_scale_y_recovers_the_projection() {
        use glam::{Mat4, Vec3};
        let fov = 60f32.to_radians();
        let want = 1.0 / (fov / 2.0).tan();
        let proj = Mat4::perspective_rh(fov, 4.0 / 3.0, 1.0, 10_000.0);
        let views = [
            (Vec3::new(0.0, 0.0, 900.0), Vec3::Y),
            (Vec3::new(600.0, 700.0, -300.0), Vec3::Y),
            (
                Vec3::new(-120.0, 40.0, 55.0),
                Vec3::new(0.2, 0.9, -0.1).normalize(),
            ),
        ];
        for (eye, up) in views {
            let vp = proj * Mat4::look_at_rh(eye, Vec3::ZERO, up);
            let got = view_proj_scale_y(&vp.to_cols_array());
            assert!((got - want).abs() < 1e-4, "eye {eye:?}: {got} != {want}");
        }
        // A different FOV must read back differently - otherwise the test
        // above would pass on a function that returned a constant.
        let narrow = Mat4::perspective_rh(0.9, 4.0 / 3.0, 1.0, 10_000.0)
            * Mat4::look_at_rh(Vec3::new(0.0, 500.0, 900.0), Vec3::ZERO, Vec3::Y);
        let narrow_scale = view_proj_scale_y(&narrow.to_cols_array());
        assert!((narrow_scale - 1.0 / (0.45f32).tan()).abs() < 1e-4);
        assert!(narrow_scale > want, "a narrower FOV must scale up");

        // The browser mirrors screen X on top of the projection
        // (`P[0] = -P[0]`); the recovery reads only the second row.
        let mut m = (proj * Mat4::look_at_rh(Vec3::new(0.0, 500.0, 900.0), Vec3::ZERO, Vec3::Y))
            .to_cols_array();
        m[0] = -m[0];
        assert!((view_proj_scale_y(&m) - want).abs() < 1e-4);
    }

    /// The strength blend: 0 is the identity everywhere, fractions move
    /// the centre keep proportionally toward the full-fade floor.
    #[test]
    fn strength_blends_toward_identity() {
        let deep = FOCUS_Z - 500.0;
        let at = |s: f32| {
            keep_probability(
                FOCUS,
                deep,
                FOCUS,
                FOCUS_Z,
                r(),
                f(),
                OCCL_MIN_KEEP,
                OCCL_DEPTH_MARGIN,
                s,
            )
        };
        assert_eq!(at(0.0), 1.0);
        assert_eq!(at(1.0), OCCL_MIN_KEEP);
        let half = at(0.5);
        assert!((half - (1.0 - 0.5 * (1.0 - OCCL_MIN_KEEP))).abs() < 1e-6);
    }

    /// Geometry at or behind the player's depth (minus the small margin)
    /// never fades - the floor tier and coplanar decals at the focus depth,
    /// and everything behind the character. (The player mesh itself is
    /// protected by the per-draw watermark, not by this margin.)
    #[test]
    fn depth_margin_shields_the_focus_depth() {
        // Dead centre on screen but AT the player depth: kept.
        assert_eq!(keep_at(FOCUS, FOCUS_Z), 1.0);
        // Just inside the margin: kept.
        assert_eq!(keep_at(FOCUS, FOCUS_Z - OCCL_DEPTH_MARGIN), 1.0);
        // Behind the player: kept.
        assert_eq!(keep_at(FOCUS, FOCUS_Z + 300.0), 1.0);
        // An occluder hugging the character (just past the margin) fades -
        // the multi-layer / point-blank case the margin used to swallow.
        assert_eq!(
            keep_at(FOCUS, FOCUS_Z - OCCL_DEPTH_MARGIN - 1.0),
            OCCL_MIN_KEEP
        );
        // A wall well in front: faded to the floor value at the centre.
        assert_eq!(keep_at(FOCUS, FOCUS_Z - 500.0), OCCL_MIN_KEEP);
    }

    /// Outside the screen-space radius nothing fades, whatever the depth.
    #[test]
    fn radius_bounds_the_fade() {
        let far_px = [FOCUS[0] + r(), FOCUS[1]];
        assert_eq!(keep_at(far_px, FOCUS_Z - 500.0), 1.0);
        let outside = [FOCUS[0], FOCUS[1] + r() + 1.0];
        assert_eq!(keep_at(outside, 100.0), 1.0);
    }

    /// The feather is monotonic: keep rises from `OCCL_MIN_KEEP` at the
    /// centre to 1.0 at the rim, never overshooting either bound.
    #[test]
    fn feather_ramps_monotonically() {
        let z = FOCUS_Z - 500.0;
        let mut last = 0.0f32;
        for i in 0..=32 {
            let d = r() * (i as f32) / 32.0;
            let k = keep_at([FOCUS[0] + d, FOCUS[1]], z);
            assert!(
                (OCCL_MIN_KEEP..=1.0).contains(&k),
                "keep {k} out of range at d={d}"
            );
            assert!(k >= last - 1e-6, "keep not monotonic at d={d}");
            last = k;
        }
        // Flat inner disc: the full-density hole.
        assert_eq!(
            keep_at([FOCUS[0] + (r() - f()), FOCUS[1]], z),
            OCCL_MIN_KEEP
        );
    }
}
