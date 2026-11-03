//! Per-scene **point lights** for the enhanced-lighting enhancement - the
//! renderer half.
//!
//! The derivation (which prims emit, how samples cluster into lights, the
//! nearest-to-player pick, the attenuation law) is the shared kernel
//! [`legaia_render_kernels::scene_lighting`], which the browser play page
//! runs too. This module is the part only the wgpu renderer has: the
//! per-light **shadow** view-projection.
//!
//! The WGSL consumer is `scene_point_gain` in `shaders.rs` (real variant
//! compiled only into the scene pipelines);
//! [`legaia_render_kernels::scene_lighting::point_attenuation`] /
//! [`legaia_render_kernels::scene_lighting::point_gain`] are its CPU mirror for the analytic
//! (non-shadow) part, asserted in lockstep by the tests below.
//!
//! # This is NOT retail
//!
//! Retail has no light sources at all; everything here is gated behind the
//! dynamic-lighting toggle, and with it off none of it runs.

use glam::{Mat4, Vec3};

use legaia_render_kernels::scene_lighting::ScenePointLight;

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
    let view = glam::camera::rh::view::look_at_mat4(pos, pos + Vec3::Y, Vec3::Z);
    let near = (light.radius * SHADOW_NEAR_FRAC).max(SHADOW_NEAR_MIN);
    let proj = glam::camera::rh::proj::directx::perspective(
        SHADOW_FOV,
        1.0,
        near,
        light.radius.max(near * 2.0),
    );
    proj * view
}

/// Whether a model-space box can reach the light's frustum under `mvp`: false
/// only when all eight corners lie outside one clip plane (depth `0..=w`). The
/// tiny margin keeps a corner on a plane in, against the float error of a matrix
/// recovered through an inverse.
pub fn bounds_touch_frustum(mvp: &Mat4, (lo, hi): &([f32; 3], [f32; 3])) -> bool {
    const MARGIN: f32 = 1e-3;
    let mut outside = [true; 6];
    for i in 0..8u32 {
        let corner = Vec3::new(
            if i & 1 == 0 { lo[0] } else { hi[0] },
            if i & 2 == 0 { lo[1] } else { hi[1] },
            if i & 4 == 0 { lo[2] } else { hi[2] },
        );
        let c = *mvp * corner.extend(1.0);
        let slack = c.w.abs() * MARGIN;
        let planes = [c.x + c.w, c.w - c.x, c.y + c.w, c.w - c.y, c.z, c.w - c.z];
        for (o, d) in outside.iter_mut().zip(planes) {
            *o &= d < -slack;
        }
    }
    !outside.contains(&true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_render_kernels::scene_lighting::MAX_SCENE_LIGHTS;

    /// A box straddling, inside and beyond the frustum: only wholly outside one
    /// plane is out, and a box enclosing the frustum with no corner in it is in.
    #[test]
    fn frustum_test_is_conservative() {
        let m = glam::camera::rh::proj::directx::perspective(1.0, 1.0, 1.0, 100.0);
        let t = |lo, hi| bounds_touch_frustum(&m, &(lo, hi));
        assert!(t([-1.0, -1.0, -20.0], [1.0, 1.0, -10.0]));
        assert!(t([-500.0, -500.0, -500.0], [500.0, 500.0, 500.0]));
        assert!(
            !t([-1.0, -1.0, 10.0], [1.0, 1.0, 20.0]),
            "behind the near plane"
        );
        assert!(
            !t([-1.0, -1.0, -300.0], [1.0, 1.0, -200.0]),
            "past the far plane"
        );
        assert!(
            !t([100.0, -1.0, -20.0], [110.0, 1.0, -10.0]),
            "off to the side"
        );
    }

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
