//! The **enhanced-lighting** shading law's WGSL twin, held in lockstep with
//! its CPU mirror.
//!
//! # This is NOT retail
//!
//! Retail field rendering has no light source at all: both TMD renderers
//! issue exactly one GTE colour op (`DPCS`, the depth cue) and never an
//! `NC*` lighting op, so all field shading is baked into the TMD colour
//! words and applied as `texel * colour / 128` (see [`crate::psx_light`]).
//! The `dyn_light` WGSL helper in `shaders.rs` layers a deliberate, opt-in
//! enhancement over that baked shading ([`crate::Renderer::set_dynamic_lighting`],
//! default off). When disabled the shader path is pixel-identical to the
//! faithful render.
//!
//! # The model
//!
//! ```text
//! base = min(ambient + (DIFFUSE * |N.L| + pool_w * pool(frag)) * key, MAX_GAIN)
//! out  = baked_rgb * min(base + point_gain, TOTAL_MAX_GAIN)   (saturating)
//! out  = baked_rgb * min(emissive_gain + point_gain, TOTAL_MAX_GAIN)  (emissive)
//! ```
//!
//! `ambient`, `key`, `pool_w` and `emissive_gain` are the staged
//! [`crate::scene_lighting::LightingMood`]; `point_gain` is the per-scene
//! point-light layer ([`crate::scene_lights`]). The CPU mirror is
//! [`crate::scene_lighting::shade`] - shared with the browser play page's
//! GLSL twin, whose constants `check-ui-host-drift.py` pairs with these.

pub use crate::scene_lighting::{
    DIFFUSE, LAMBERT_FALLBACK, MAX_GAIN, POOL_CENTER, POOL_INNER, POOL_OUTER, TOTAL_MAX_GAIN,
    pool_factor, shade,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_lighting::LightingMood;

    /// The WGSL twin must carry the same tunables as the shared CPU mirror
    /// - the shader is the production path.
    #[test]
    fn wgsl_constants_match_the_mirror() {
        let src = crate::shaders::PSX_DITHER_WGSL;
        for needle in [
            format!("const DYN_DIFFUSE: f32 = {DIFFUSE};"),
            format!("const DYN_MAX_GAIN: f32 = {MAX_GAIN};"),
            format!("const DYN_TOTAL_MAX_GAIN: f32 = {TOTAL_MAX_GAIN};"),
            format!("const DYN_LAMBERT_FALLBACK: f32 = {LAMBERT_FALLBACK};"),
            format!(
                "const DYN_POOL_CENTER: vec2<f32> = vec2<f32>({}, {});",
                POOL_CENTER[0], POOL_CENTER[1]
            ),
            format!("const DYN_POOL_INNER: f32 = {POOL_INNER};"),
            format!("const DYN_POOL_OUTER: f32 = {POOL_OUTER};"),
        ] {
            assert!(
                src.contains(&needle),
                "WGSL drifted from the mirror: {needle}"
            );
        }
        use crate::scene_lighting::{
            WINDOW_FLOOR, WINDOW_GLASS_BLACK_MAX, WINDOW_GLASS_MIN_BLUE, WINDOW_RGB,
        };
        for needle in [
            format!("const WIN_GLASS_MIN_BLUE: f32 = {WINDOW_GLASS_MIN_BLUE};"),
            format!(
                "const WIN_RGB: vec3<f32> = vec3<f32>({:?}, {:?}, {:?});",
                WINDOW_RGB[0], WINDOW_RGB[1], WINDOW_RGB[2]
            ),
            format!("const WIN_FLOOR: f32 = {WINDOW_FLOOR};"),
            format!("const WIN_GLASS_BLACK_MAX: f32 = {WINDOW_GLASS_BLACK_MAX};"),
        ] {
            assert!(
                src.contains(&needle),
                "WGSL drifted from the mirror: {needle}"
            );
        }
        assert!(src.contains("fn dyn_window"));
        assert!(src.contains("fn dyn_light"));
        assert!(src.contains(
            "let g = min(amb.rgb + 0.5 * DYN_DIFFUSE * light_color.xyz, vec3<f32>(DYN_MAX_GAIN));"
        ));
        assert!(src.contains(
            "let eg = min(vec3<f32>(amb.w) + point_gain, vec3<f32>(DYN_TOTAL_MAX_GAIN));"
        ));
    }

    /// The single-mesh pipelines' mood stub is the daylight mood's ambient
    /// word exactly.
    #[test]
    fn stub_mood_is_daylight() {
        let a = LightingMood::DAY.uniforms(true)[2];
        let stub = crate::shaders::SCENE_LIGHTS_STUB_WGSL;
        let want = format!(
            "return vec4<f32>({:?}, {:?}, {:?}, {:?});",
            a[0], a[1], a[2], a[3]
        );
        assert!(stub.contains(&want), "stub mood drifted: {want}");
    }

    /// Disabled is the exact identity (what the parity oracles rely on).
    #[test]
    fn disabled_is_exact_identity() {
        let rgb = [0.123, 0.456, 0.789];
        let out = shade(
            rgb,
            [0.0, -1.0, 0.0],
            [100.0, 100.0],
            [960.0, 720.0],
            LightingMood::DAY.uniforms(false),
            [0.5; 3],
            true,
        );
        assert_eq!(out, rgb);
    }
}
