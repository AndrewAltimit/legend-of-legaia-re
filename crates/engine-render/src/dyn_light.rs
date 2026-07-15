//! CPU mirror of the **opt-in dynamic-lighting enhancement** - the
//! `dyn_light` WGSL helper in `shaders.rs`, as a pure kernel.
//!
//! # This is NOT retail
//!
//! Retail field rendering has no light source at all: both TMD renderers
//! issue exactly one GTE colour op (`DPCS`, the depth cue) and never an
//! `NC*` lighting op, so all field shading is baked into the TMD colour
//! words and applied as `texel * colour / 128` (see [`crate::psx_light`]).
//! This module models a deliberate, opt-in *enhancement* layered over that
//! baked shading ([`crate::Renderer::set_dynamic_lighting`], default off).
//! When disabled the shader path is pixel-identical to the faithful render.
//!
//! # The model
//!
//! ```text
//! gain = ambient + (DIFFUSE * |N.L| + POOL * pool(frag)) * warm_tint
//! out  = baked_rgb * min(gain, MAX_GAIN)        (saturating at 1.0)
//! ```
//!
//! * `|N.L|` - a soft directional term off the smoothed per-vertex normals
//!   the VRAM-mesh vertex format already carries (built by
//!   `legaia_tmd::mesh`'s area-weighted per-position accumulation). `abs`
//!   rather than `max(.., 0)` because the corpus' prim winding is mixed -
//!   the accumulated normals have no canonical facing side - and it keeps a
//!   mostly-vertical light invariant under the per-frame Y-flip parity.
//! * `pool(frag)` - a soft screen-space "pool of light" centred a touch
//!   above frame centre, fading toward the corners: the gentle
//!   vignette-of-light gradient over the ground.
//! * `MAX_GAIN` caps the whole gain so nothing exceeds ~1.3x the baked
//!   brightness (no blow-out); textures stay crisp because the gain is a
//!   smooth per-pixel scale, never a blur.
//!
//! The WGSL twin's `DYN_*` constants are asserted in lockstep by the tests
//! below; the light direction / tint / ambient tunables live in
//! [`crate::renderer`]'s state module ([`crate::DYN_LIGHT_DIR`] /
//! [`crate::DYN_LIGHT_TINT`] / [`crate::DYN_LIGHT_AMBIENT`]) since they are
//! staged per frame through `MeshUniforms`.

/// Weight of the orientation (`|N.L|`) term. WGSL twin: `DYN_DIFFUSE`.
pub const DIFFUSE: f32 = 0.55;
/// Weight of the screen-space light pool. WGSL twin: `DYN_POOL`.
pub const POOL: f32 = 0.35;
/// Gain ceiling relative to the baked colour. WGSL twin: `DYN_MAX_GAIN`.
pub const MAX_GAIN: f32 = 1.3;
/// Orientation term used when no normal is available (zero vertex normal
/// and degenerate geometric normal). WGSL twin: `DYN_LAMBERT_FALLBACK`.
pub const LAMBERT_FALLBACK: f32 = 0.6;
/// Pool centre in 0..1 screen fractions (x, y). WGSL twin: `DYN_POOL_CENTER`.
pub const POOL_CENTER: [f32; 2] = [0.5, 0.45];
/// Radius (screen fraction) inside which the pool is full-strength.
pub const POOL_INNER: f32 = 0.15;
/// Radius (screen fraction) beyond which the pool has fully faded.
pub const POOL_OUTER: f32 = 0.75;

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// WGSL `smoothstep` (Hermite) - clamped cubic ramp between `e0` and `e1`.
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The screen-space light-pool factor for a fragment at `frag_px` on a
/// `viewport`-sized surface: 1.0 at [`POOL_CENTER`], fading to 0.0 past
/// [`POOL_OUTER`]. Returns 0.0 for a degenerate viewport (headless slots).
pub fn pool_factor(frag_px: [f32; 2], viewport: [f32; 2]) -> f32 {
    if viewport[0] <= 0.0 || viewport[1] <= 0.0 {
        return 0.0;
    }
    let dx = frag_px[0] / viewport[0] - POOL_CENTER[0];
    let dy = frag_px[1] / viewport[1] - POOL_CENTER[1];
    let d = (dx * dx + dy * dy).sqrt();
    1.0 - smoothstep(POOL_INNER, POOL_OUTER, d)
}

/// The full per-fragment kernel - mirrors the `dyn_light` WGSL helper.
///
/// * `rgb` - the baked shaded colour (post `psx_modulate`), 0..=1.
/// * `normal` - the smoothed per-vertex normal; pass zero to fall back to
///   `geo_normal` (the shader's screen-space-derivative face normal).
/// * `light_dir` - `[x, y, z, enable]`; `enable < 0.5` returns `rgb`
///   unchanged (the default-off identity the parity oracles rely on).
/// * `light_color` - `[tint_r, tint_g, tint_b, ambient]`.
pub fn shade(
    rgb: [f32; 3],
    normal: [f32; 3],
    geo_normal: [f32; 3],
    frag_px: [f32; 2],
    viewport: [f32; 2],
    light_dir: [f32; 4],
    light_color: [f32; 4],
) -> [f32; 3] {
    if light_dir[3] < 0.5 {
        return rgb;
    }
    let mut lambert = LAMBERT_FALLBACK;
    let n = if dot3(normal, normal) < 1e-8 {
        geo_normal
    } else {
        normal
    };
    let n_len = dot3(n, n).sqrt();
    if n_len > 1e-6 {
        let l = [light_dir[0], light_dir[1], light_dir[2]];
        let l_len = dot3(l, l).sqrt();
        if l_len > 0.0 {
            lambert = (dot3(n, l) / (n_len * l_len)).abs();
        }
    }
    let pool = pool_factor(frag_px, viewport);
    let mut out = [0.0f32; 3];
    for i in 0..3 {
        let gain = light_color[3] + (DIFFUSE * lambert + POOL * pool) * light_color[i];
        out[i] = (rgb[i] * gain.min(MAX_GAIN)).clamp(0.0, 1.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DYN_LIGHT_AMBIENT, DYN_LIGHT_DIR, DYN_LIGHT_TINT};

    fn light_dir_on() -> [f32; 4] {
        [DYN_LIGHT_DIR[0], DYN_LIGHT_DIR[1], DYN_LIGHT_DIR[2], 1.0]
    }

    fn light_color() -> [f32; 4] {
        [
            DYN_LIGHT_TINT[0],
            DYN_LIGHT_TINT[1],
            DYN_LIGHT_TINT[2],
            DYN_LIGHT_AMBIENT,
        ]
    }

    /// The load-bearing invariant: with the enable at zero (the staged
    /// default) the kernel is an EXACT identity, so the default render path
    /// stays pixel-identical and the vram-oracle / determinism suites are
    /// untouched.
    #[test]
    fn disabled_is_exact_identity() {
        let rgb = [0.123, 0.456, 0.789];
        let off = [DYN_LIGHT_DIR[0], DYN_LIGHT_DIR[1], DYN_LIGHT_DIR[2], 0.0];
        let out = shade(
            rgb,
            [0.0, -1.0, 0.0],
            [0.3, 0.4, 0.5],
            [100.0, 100.0],
            [960.0, 720.0],
            off,
            light_color(),
        );
        assert_eq!(out, rgb, "disabled dyn_light must not touch the colour");
    }

    /// Nothing ever exceeds MAX_GAIN x the baked colour: an up-facing ground
    /// fragment dead-centre in the pool (the maximum-gain case) still lands
    /// at `rgb * 1.3` (saturating).
    #[test]
    fn gain_is_capped() {
        let rgb = [0.5, 0.5, 0.5];
        let centre = [960.0 * POOL_CENTER[0], 720.0 * POOL_CENTER[1]];
        let out = shade(
            rgb,
            [0.0, -1.0, 0.0], // ground normal (TMD space is Y-down)
            [0.0; 3],
            centre,
            [960.0, 720.0],
            light_dir_on(),
            light_color(),
        );
        for (i, c) in out.iter().enumerate() {
            assert!(
                *c <= rgb[i] * MAX_GAIN + 1e-6,
                "channel {i} exceeded the {MAX_GAIN}x cap: {out:?}"
            );
        }
        // And the red channel actually reaches the cap (the warm tint keeps
        // green/blue slightly below it) - the pool + diffuse are strong
        // enough at centre-field that the clamp is live.
        assert!((out[0] - rgb[0] * MAX_GAIN).abs() < 1e-5, "{out:?}");
    }

    /// Orientation shading: a wall (horizontal normal, near-perpendicular to
    /// the mostly-vertical light) reads darker than the ground under the
    /// same pool factor - the "gate pillars darker than the front face"
    /// read of the reference look.
    #[test]
    fn walls_shade_darker_than_ground() {
        let rgb = [0.5, 0.5, 0.5];
        let at = [480.0, 324.0];
        let vp = [960.0, 720.0];
        let ground = shade(
            rgb,
            [0.0, -1.0, 0.0],
            [0.0; 3],
            at,
            vp,
            light_dir_on(),
            light_color(),
        );
        let wall = shade(
            rgb,
            [0.0, 0.0, 1.0],
            [0.0; 3],
            at,
            vp,
            light_dir_on(),
            light_color(),
        );
        assert!(
            wall[0] < ground[0],
            "wall {wall:?} should be darker than ground {ground:?}"
        );
        // ...but never below the ambient floor: no blackout.
        assert!(wall[0] >= rgb[0] * DYN_LIGHT_AMBIENT - 1e-6, "{wall:?}");
    }

    /// The winding tolerance: a flipped ground normal (+Y instead of -Y)
    /// lights identically, because the orientation term is |N.L|.
    #[test]
    fn orientation_term_is_sign_tolerant() {
        let rgb = [0.4, 0.4, 0.4];
        let at = [200.0, 500.0];
        let vp = [960.0, 720.0];
        let up = shade(
            rgb,
            [0.0, -1.0, 0.0],
            [0.0; 3],
            at,
            vp,
            light_dir_on(),
            light_color(),
        );
        let down = shade(
            rgb,
            [0.0, 1.0, 0.0],
            [0.0; 3],
            at,
            vp,
            light_dir_on(),
            light_color(),
        );
        assert_eq!(up, down);
    }

    /// The pool is the soft vignette-of-light: full near the centre, gone
    /// past the outer radius, monotonically fading between.
    #[test]
    fn pool_fades_from_centre_to_corner() {
        let vp = [960.0, 720.0];
        let centre = pool_factor([480.0, 324.0], vp);
        let mid = pool_factor([720.0, 500.0], vp);
        let corner = pool_factor([959.0, 719.0], vp);
        assert!((centre - 1.0).abs() < 1e-6, "{centre}");
        assert!(mid > 0.0 && mid < centre, "{mid}");
        assert!(corner < mid, "{corner} vs {mid}");
    }

    /// The WGSL twin must carry the same tunables - the shader is the
    /// production path, this module is the documented mirror.
    #[test]
    fn wgsl_constants_match_the_mirror() {
        let src = crate::shaders::PSX_DITHER_WGSL;
        for needle in [
            "const DYN_DIFFUSE: f32 = 0.55;",
            "const DYN_POOL: f32 = 0.35;",
            "const DYN_MAX_GAIN: f32 = 1.3;",
            "const DYN_LAMBERT_FALLBACK: f32 = 0.6;",
            "const DYN_POOL_CENTER: vec2<f32> = vec2<f32>(0.5, 0.45);",
            "const DYN_POOL_INNER: f32 = 0.15;",
            "const DYN_POOL_OUTER: f32 = 0.75;",
        ] {
            assert!(
                src.contains(needle),
                "WGSL drifted from the mirror: {needle}"
            );
        }
        assert!(src.contains("fn dyn_light"));
    }
}
