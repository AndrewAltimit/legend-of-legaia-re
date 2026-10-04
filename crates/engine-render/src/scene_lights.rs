//! Per-scene **point lights** for the enhanced-lighting enhancement - the
//! renderer half.
//!
//! The derivation (which prims emit, how samples cluster into lights, the
//! nearest-to-player pick, the attenuation law) is the shared kernel
//! [`crate::scene_lighting`] (`legaia_engine_ui::scene_lighting`), which the
//! browser play page runs too; it is re-exported here under the module path
//! the native host has always used. What stays here is the part only the
//! wgpu renderer has: the per-light **shadow** view-projection.
//!
//! The WGSL consumer is `scene_point_gain` in `shaders.rs` (real variant
//! compiled only into the scene pipelines);
//! [`crate::scene_lighting::point_attenuation`] /
//! [`crate::scene_lighting::point_gain`] are its CPU mirror for the analytic
//! (non-shadow) part, asserted in lockstep by the tests below.
//!
//! # This is NOT retail
//!
//! Retail has no light sources at all; everything here is gated behind the
//! dynamic-lighting toggle, and with it off none of it runs.

use glam::{Mat4, Vec3};

pub use crate::scene_lighting::{
    CLUSTER_MAX_EXTENT, CLUSTER_MERGE_DIST, EMIT_MAX_TRI_AREA, EMIT_MIN_BRIGHT, EmitterSample,
    MAX_SCENE_LIGHTS, RADIUS_MAX, RADIUS_MIN, RADIUS_SCALE, ScenePointLight,
    cluster_all_scene_lights, cluster_scene_lights, color_mesh_emitters, nearest_lights,
    point_attenuation, point_gain, transform_samples, vram_mesh_emitters,
};

/// Shadow-cone vertical field of view (radians). Wide, because the cone
/// approximates a point source lighting the floor and walls around it.
pub const SHADOW_FOV: f32 = 2.1;

/// Near-plane fraction of the light radius. Geometry closer to the light
/// than this (the emitting flame quad itself) casts no shadow.
pub const SHADOW_NEAR_FRAC: f32 = 0.04;
/// Absolute near-plane floor (world units).
pub const SHADOW_NEAR_MIN: f32 = 24.0;

/// The light's shadow view-projection: a downward cone (retail field
/// space is Y-down, so "down toward the floor" is `+Y`) with a wgpu 0..1
/// depth range. Fragments outside the cone sample as unshadowed.
pub fn light_view_proj(light: &ScenePointLight) -> Mat4 {
    let pos = Vec3::from(light.pos);
    let view = Mat4::look_at_rh(pos, pos + Vec3::Y, Vec3::Z);
    let near = (light.radius * SHADOW_NEAR_FRAC).max(SHADOW_NEAR_MIN);
    let proj = Mat4::perspective_rh(SHADOW_FOV, 1.0, near, light.radius.max(near * 2.0));
    proj * view
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shadow view-projection maps a floor point under the light into
    /// the depth range and NDC square.
    #[test]
    fn shadow_view_proj_covers_floor() {
        let l = ScenePointLight {
            pos: [500.0, -200.0, 300.0],
            color: [1.0; 3],
            radius: 1000.0,
        };
        let vp = light_view_proj(&l);
        let clip = vp * glam::Vec4::new(500.0, 200.0, 300.0, 1.0);
        assert!(clip.w > 0.0);
        let ndc = clip / clip.w;
        assert!(ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0, "{ndc:?}");
        assert!(ndc.z > 0.0 && ndc.z < 1.0, "{ndc:?}");
    }

    /// The WGSL twin carries the same array length, attenuation shape and
    /// half-Lambert wrap as the shared CPU mirror.
    #[test]
    fn wgsl_constants_match_the_mirror() {
        let src = crate::shaders::scene_lights_wgsl_for_tests();
        for needle in [
            "const SCENE_LIGHT_MAX: u32 = 8u;",
            "att = att * att;",
            "fn scene_point_gain(",
            "lam = abs(dot(n / n_len, to_l / dist)) * 0.5 + 0.5;",
            "return sl.ambient;",
        ] {
            assert!(
                src.contains(needle),
                "WGSL drifted from the mirror: {needle}"
            );
        }
        assert_eq!(MAX_SCENE_LIGHTS, 8);
    }
}
