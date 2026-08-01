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
//!    * its framebuffer position lies within [`OCCL_RADIUS_FRAC`] of the
//!      viewport height around the player's projected centre (so distant
//!      parts of the same wall stay solid - only the patch actually
//!      covering the character opens up).
//! 3. The fade is a **screen-door discard** against a 4x4 Bayer threshold
//!    matrix ([`bayer_threshold`], the WGSL twin is `occl_bayer`): the keep
//!    probability ramps from 1.0 at the circle's rim down to
//!    [`OCCL_MIN_KEEP`] at the centre over an [`OCCL_FEATHER_FRAC`]
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

/// Fade-circle radius as a fraction of the viewport height. WGSL twin:
/// staged into `occl_params.x` in pixels.
pub const OCCL_RADIUS_FRAC: f32 = 0.30;

/// Width of the rim feather (keep probability ramps 1.0 -> [`OCCL_MIN_KEEP`]
/// over this band), as a fraction of the viewport height. WGSL twin:
/// staged into `occl_params.w` in pixels.
pub const OCCL_FEATHER_FRAC: f32 = 0.15;

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
/// the player's projected framebuffer pixel; `radius_px` / `feather_px`
/// are [`OCCL_RADIUS_FRAC`] / [`OCCL_FEATHER_FRAC`] times the viewport
/// height. `strength` (0..1) is the host's eased visibility-gate output:
/// the geometric keep blends toward the identity by it, so the
/// screen-door dissolves in and out instead of popping.
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

    const R: f32 = 216.0; // 0.30 * 720
    const F: f32 = 108.0; // 0.15 * 720
    const FOCUS: [f32; 2] = [480.0, 360.0];
    const FOCUS_Z: f32 = 1800.0;

    fn keep_at(frag_px: [f32; 2], frag_z: f32) -> f32 {
        keep_probability(
            frag_px,
            frag_z,
            FOCUS,
            FOCUS_Z,
            R,
            F,
            OCCL_MIN_KEEP,
            OCCL_DEPTH_MARGIN,
            1.0,
        )
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
                R,
                F,
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
        let far_px = [FOCUS[0] + R, FOCUS[1]];
        assert_eq!(keep_at(far_px, FOCUS_Z - 500.0), 1.0);
        let outside = [FOCUS[0], FOCUS[1] + R + 1.0];
        assert_eq!(keep_at(outside, 100.0), 1.0);
    }

    /// The feather is monotonic: keep rises from `OCCL_MIN_KEEP` at the
    /// centre to 1.0 at the rim, never overshooting either bound.
    #[test]
    fn feather_ramps_monotonically() {
        let z = FOCUS_Z - 500.0;
        let mut last = 0.0f32;
        for i in 0..=32 {
            let d = R * (i as f32) / 32.0;
            let k = keep_at([FOCUS[0] + d, FOCUS[1]], z);
            assert!(
                (OCCL_MIN_KEEP..=1.0).contains(&k),
                "keep {k} out of range at d={d}"
            );
            assert!(k >= last - 1e-6, "keep not monotonic at d={d}");
            last = k;
        }
        // Flat inner disc: the full-density hole.
        assert_eq!(keep_at([FOCUS[0] + (R - F), FOCUS[1]], z), OCCL_MIN_KEEP);
    }
}
