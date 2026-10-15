//! The renderer's depth conventions: the depth target format + view, the
//! reversed-Z projection fold and clear value, and the shadow-map
//! resolution and compare bias.

pub(super) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Shadow-map resolution per light (square). One layer of the depth array
/// per possible scene light.
pub(super) const SHADOW_MAP_DIM: u32 = 512;

/// Shader-side depth-compare bias for the shadow test (on top of the
/// pipeline's rasterizer depth bias); staged into `SceneLightsU.params.z`.
pub(super) const SHADOW_COMPARE_BIAS: f32 = 0.0015;

/// The reversed-Z clear value: depth 0.0 is the far plane under the
/// `Greater` / `GreaterEqual` compares every 3D pipeline uses.
pub(super) const DEPTH_CLEAR: f32 = 0.0;

/// Post-multiply a projection (or full MVP) into the **reversed-Z**
/// convention: clip `z' = w - z`, so NDC depth runs 1.0 (near) -> 0.0 (far).
///
/// With a float depth buffer this spends the format's exponent range across
/// the whole view volume - resolution stays proportional to view depth
/// (`~z * 2^-23`) instead of collapsing quadratically toward the far plane,
/// which is what lets the sub-unit coplanar-separation nudges
/// (`legaia_tmd::mesh::coplanar`, `legaia_engine_scene::coplanar_draws`)
/// resolve at every camera distance up to the engine-wide `SCENE_FAR`.
/// Callers never build reversed projections themselves - the renderer's two
/// MVP upload sites apply this centrally, so every camera (orbit, field
/// follow, world map, cutscene, VR fork) inherits it. The clip `w` row is
/// untouched, so CPU-side view-depth keys (`psx_blend::prim_depth_key`) and
/// the shaders' `clip_pos.w` reads are unaffected.
pub(super) fn reverse_z(mvp: glam::Mat4) -> glam::Mat4 {
    // Rows: x, y unchanged; z' = -z + w; w' = w.  (glam is column-major.)
    glam::Mat4::from_cols(
        glam::Vec4::new(1.0, 0.0, 0.0, 0.0),
        glam::Vec4::new(0.0, 1.0, 0.0, 0.0),
        glam::Vec4::new(0.0, 0.0, -1.0, 0.0),
        glam::Vec4::new(0.0, 0.0, 1.0, 1.0),
    ) * mvp
}

/// Fold a constant screen-row offset into a clip matrix: `rows` PSX stage
/// rows down the screen (negative is up), `240` rows to the NDC span of `2`.
///
/// It is the overworld curvature table's build offset
/// (`legaia_render_kernels::overworld_curvature::REBUILD_BIAS`): a constant
/// on every table entry, so where the depth-indexed part is evaluated per
/// vertex (`OVERWORLD_CURVE_WGSL`), the offset is exactly `y' = y - rows *
/// (2 / 240) * w` - linear in clip space - and rides the matrix. A flat
/// table (the map display's, curvature scale `0`) still carries it.
pub(super) fn fold_screen_rows(mvp: glam::Mat4, rows: f32) -> glam::Mat4 {
    if rows == 0.0 {
        return mvp;
    }
    let k = -rows * (2.0 / 240.0);
    glam::Mat4::from_cols(
        glam::Vec4::new(1.0, 0.0, 0.0, 0.0),
        glam::Vec4::new(0.0, 1.0, 0.0, 0.0),
        glam::Vec4::new(0.0, 0.0, 1.0, 0.0),
        glam::Vec4::new(0.0, k, 0.0, 1.0),
    ) * mvp
}

pub(super) fn create_depth_view(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth target"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        // Sampled by the volumetric ground-fog pass's soft intersection
        // (`renderer/fog_volume.rs`), which reads the scene's depth in a
        // pass of its own with the attachment held read-only.
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

#[cfg(test)]
mod reverse_z_tests {
    use super::*;

    #[test]
    fn near_maps_to_one_far_to_zero_and_w_is_preserved() {
        let near = 4.0f32;
        let far = 1_000_000.0f32;
        let proj = glam::Mat4::perspective_rh(1.0, 1.0, near, far);
        let rz = reverse_z(proj);
        for (z_eye, expect) in [(near, 1.0f32), (far, 0.0f32)] {
            let clip = rz * glam::Vec4::new(0.0, 0.0, -z_eye, 1.0);
            let ndc = clip.z / clip.w;
            assert!(
                (ndc - expect).abs() < 1e-3,
                "z_eye {z_eye}: ndc {ndc} != {expect}"
            );
            // w row untouched: still the view depth.
            assert!((clip.w - z_eye).abs() < 1e-2);
        }
        // Nearer geometry must now test GREATER than farther geometry.
        let near_pt = rz * glam::Vec4::new(0.0, 0.0, -10.0, 1.0);
        let far_pt = rz * glam::Vec4::new(0.0, 0.0, -1000.0, 1.0);
        assert!(near_pt.z / near_pt.w > far_pt.z / far_pt.w);
    }

    /// The curvature table's build offset moves every vertex the same number
    /// of stage rows at any depth - exactly what adding it to `SY` does - and
    /// leaves `x`, depth and `w` alone. `-6` (the map display's rebuild) is up.
    #[test]
    fn folded_rows_shift_every_depth_alike() {
        let proj = glam::Mat4::perspective_rh(1.0, 4.0 / 3.0, 4.0, 1_000_000.0);
        let folded = fold_screen_rows(proj, -6.0);
        for z in [10.0f32, 500.0, 40_000.0] {
            let p = glam::Vec4::new(3.0, -2.0, -z, 1.0);
            let (a, b) = (proj * p, folded * p);
            let rows = |c: glam::Vec4| (1.0 - c.y / c.w) * 120.0;
            assert!((rows(b) - rows(a) + 6.0).abs() < 1e-3, "z {z}");
            assert_eq!((a.x, a.z, a.w), (b.x, b.z, b.w));
        }
        assert_eq!(fold_screen_rows(proj, 0.0), proj);
    }
}
