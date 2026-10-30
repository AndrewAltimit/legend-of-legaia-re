//! Screen-space ambient occlusion (an enhancement, NOT retail): the numbers
//! and the per-sample law both hosts shade contact shadows with.
//!
//! Retail has no ambient term to occlude - the field path draws each
//! primitive's baked colour word with no light source - so the pass darkens
//! the finished colour of opaque scene geometry where nearby geometry
//! closes over it (a wall's foot, the floor under a table, a character's
//! feet), before the semi-transparent prims, the glow sprites, the fog, the
//! bloom and every screen-space layer draw.
//!
//! The passes are host code (the native renderer's `renderer/ssao.rs`; a
//! later WebGL2 twin on the play page). What they share lives here:
//!
//! * [`SsaoParams`] - the tuned constants;
//! * [`view_scale`] - how a pixel's linear depth becomes a view-space
//!   position in world units, from the frame's view-projection;
//! * [`sample_offset`] / [`spin`] - the sample pattern and its per-pixel
//!   rotation, as formulas a shader evaluates (no table to upload);
//! * [`occlusion_term`] / [`ao_from_sum`] - the per-sample law (the
//!   "Alchemy" estimator, McGuire et al. 2011) and its resolve;
//! * [`blur_weight`] / [`ao_downscale`] - the depth-aware blur and the AO
//!   resolution rule;
//! * [`contact_light`] / [`view_to_ndc`] / [`contact_hit`] /
//!   [`contact_fade`] / [`contact_from_hit`] - the **contact shadows** the
//!   same pass marches toward enhanced lighting's key light (below).
//!
//! # Contact shadows
//!
//! Folded into the AO term, under the same toggle: while enhanced lighting
//! is on, each AO texel also marches a short ray from its surface toward the
//! mood's key light ([`crate::scene_lighting::LightingMood::key_dir`])
//! through the same G-buffer, and darkens where the ray passes behind
//! nearer geometry - the sliver of shadow a character, a crate or an eave
//! casts right where it meets the ground, which a mood light with no shadow
//! map cannot draw. The result multiplies into the AO texel before the blur,
//! so the blur that removes the AO's noise removes the march's jitter too,
//! and the apply that darkens the opaque frame applies both. No extra pass
//! and no extra target: the cost is [`CONTACT_STEPS`] more G-buffer reads
//! per AO texel. With enhanced lighting off there is no key light to cast
//! from, and the strength word is zero ([`contact_light`]).
//!
//! Every function here is the CPU mirror of a WGSL helper of the same name
//! in `engine-render`'s `SSAO_SHADER_SRC`; the tests below pin the shapes
//! that matter (a flat floor is unoccluded, a corner is, far samples fade
//! out, a depth step stops the blur).

/// One frame's SSAO response.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SsaoParams {
    /// Sampling radius in world units (a field tile is 128).
    pub radius: f32,
    /// Depth-proportional bias against self-occlusion on tilted
    /// faces: a sample counts only above `bias * depth` along the normal.
    pub bias: f32,
    /// Gain on the averaged occlusion.
    pub intensity: f32,
    /// Contrast exponent applied to the resolved visibility.
    pub power: f32,
    /// Darkest the multiplier may go - the floor that keeps a crevice from
    /// turning black.
    pub min_ao: f32,
    /// Largest on-screen sampling radius as a fraction of the viewport
    /// height, so a surface right at the lens does not sample half the
    /// screen (and the cost stays bounded).
    pub max_radius_frac: f32,
    /// Smallest on-screen radius in output pixels; below it the pixel is
    /// too far for the radius to resolve and takes no occlusion.
    pub min_radius_px: f32,
    /// Samples per pixel.
    pub samples: u32,
    /// Blur taps on each side of the centre, per axis.
    pub blur_radius: u32,
    /// Relative depth difference at which a blur tap's weight falls to
    /// `1/e`.
    pub blur_depth_sigma: f32,
    /// Exponent on the normals' dot product in the blur weight.
    pub blur_normal_power: f32,
    /// Contact-shadow ray length in world units.
    pub contact_length: f32,
    /// How far behind the G-buffer surface a ray sample may sit and still
    /// count as shadowed, in world units - what keeps a ray that passes far
    /// behind a thin silhouette from shadowing the ground beyond it.
    pub contact_thickness: f32,
    /// Ray steps per AO texel.
    pub contact_steps: u32,
    /// Depth-proportional start offset along the normal and hit bias (as
    /// [`Self::bias`]), against a surface shadowing itself.
    pub contact_bias: f32,
    /// Darkest a full-strength key light's contact shadow multiplies by is
    /// `1 - contact_strength`.
    pub contact_strength: f32,
}

/// Sampling radius in world units.
pub const RADIUS: f32 = 160.0;
/// See [`SsaoParams::bias`].
pub const BIAS: f32 = 0.004;
/// See [`SsaoParams::intensity`].
pub const INTENSITY: f32 = 1.2;
/// See [`SsaoParams::power`].
pub const POWER: f32 = 1.3;
/// See [`SsaoParams::min_ao`].
pub const MIN_AO: f32 = 0.35;
/// See [`SsaoParams::max_radius_frac`].
pub const MAX_RADIUS_FRAC: f32 = 0.07;
/// See [`SsaoParams::min_radius_px`].
pub const MIN_RADIUS_PX: f32 = 1.0;
/// See [`SsaoParams::samples`].
pub const SAMPLES: u32 = 12;
/// See [`SsaoParams::blur_radius`].
pub const BLUR_RADIUS: u32 = 4;
/// See [`SsaoParams::blur_depth_sigma`].
pub const BLUR_DEPTH_SIGMA: f32 = 0.03;
/// See [`SsaoParams::blur_normal_power`].
pub const BLUR_NORMAL_POWER: f32 = 8.0;
/// See [`SsaoParams::contact_length`].
pub const CONTACT_LENGTH: f32 = 72.0;
/// See [`SsaoParams::contact_thickness`].
pub const CONTACT_THICKNESS: f32 = 56.0;
/// See [`SsaoParams::contact_steps`].
pub const CONTACT_STEPS: u32 = 8;
/// See [`SsaoParams::contact_bias`].
pub const CONTACT_BIAS: f32 = 0.006;
/// See [`SsaoParams::contact_strength`].
pub const CONTACT_STRENGTH: f32 = 0.5;
/// The key-light luminance a mood casts contact shadows at full strength
/// from (daylight's); a dimmer key casts proportionally fainter ones.
pub const CONTACT_FULL_KEY_LUMA: f32 = 0.42;
/// Gaussian width of the blur, in AO texels.
pub const BLUR_SIGMA: f32 = 2.5;
/// Golden angle in radians, the spiral step between samples.
pub const GOLDEN_ANGLE: f32 = 2.399_963_2;
/// Output heights at or below this run AO at full resolution (the native
/// 320 x 240 target and small windows); above it AO runs at half
/// resolution on each axis.
pub const FULL_RES_MAX_HEIGHT: u32 = 540;

impl Default for SsaoParams {
    fn default() -> Self {
        Self {
            radius: RADIUS,
            bias: BIAS,
            intensity: INTENSITY,
            power: POWER,
            min_ao: MIN_AO,
            max_radius_frac: MAX_RADIUS_FRAC,
            min_radius_px: MIN_RADIUS_PX,
            samples: SAMPLES,
            blur_radius: BLUR_RADIUS,
            blur_depth_sigma: BLUR_DEPTH_SIGMA,
            blur_normal_power: BLUR_NORMAL_POWER,
            contact_length: CONTACT_LENGTH,
            contact_thickness: CONTACT_THICKNESS,
            contact_steps: CONTACT_STEPS,
            contact_bias: CONTACT_BIAS,
            contact_strength: CONTACT_STRENGTH,
        }
    }
}

/// How much smaller than the output the AO targets are on each axis: `1`
/// up to [`FULL_RES_MAX_HEIGHT`], `2` above. Full resolution on the native
/// target matters: a half-resolution AO texel there spans two whole stage
/// pixels, and PSX mode's snapped edges walk across it frame to frame,
/// which reads as shimmer.
pub fn ao_downscale(_width: u32, height: u32) -> u32 {
    if height <= FULL_RES_MAX_HEIGHT { 1 } else { 2 }
}

/// The view-space reconstruction factors `(kx, ky, kw)` of a world-to-clip
/// matrix (column-major, as `glam::Mat4::to_cols_array` lays it out).
///
/// For a camera that is a rotation, a uniform scale and a translation under
/// a centred perspective projection, clip `x`, `y` and `w` are the view
/// position scaled by `|row0.xyz|`, `|row1.xyz|` and `|row3.xyz|`. So a
/// pixel at NDC `(x, y)` with clip `w` sits at
/// `(x * w * kx, y * w * ky, w * kw)` in view space, in world units, with
/// each `k` the reciprocal of its row's length. Zero for a degenerate row.
pub fn view_scale(view_proj: &[f32; 16]) -> [f32; 3] {
    let row = |r: usize| {
        let (a, b, c) = (view_proj[r], view_proj[4 + r], view_proj[8 + r]);
        let len = (a * a + b * b + c * c).sqrt();
        if len > 1e-12 { 1.0 / len } else { 0.0 }
    };
    [row(0), row(1), row(3)]
}

/// The view-space position of a pixel: NDC `(x, y)`, clip `w`, and the
/// frame's [`view_scale`].
pub fn view_position(ndc: [f32; 2], w: f32, k: [f32; 3]) -> [f32; 3] {
    [ndc[0] * w * k[0], ndc[1] * w * k[1], w * k[2]]
}

/// On-screen sampling radius in output pixels for a surface at view depth
/// `view_z` (world units): the world radius projected, clamped to
/// [`SsaoParams::max_radius_frac`] of the viewport height. `k` is the
/// frame's [`view_scale`]; one world unit at depth `z` spans
/// `kw / (ky * z)` in NDC, and NDC's `2` covers `viewport_h` pixels.
pub fn radius_px(p: &SsaoParams, view_z: f32, k: [f32; 3], viewport_h: f32) -> f32 {
    if view_z <= 0.0 || k[1] <= 0.0 {
        return 0.0;
    }
    let px = p.radius * 0.5 * viewport_h * k[2] / (k[1] * view_z);
    px.min(p.max_radius_frac * viewport_h)
}

/// Per-pixel rotation of the sample spiral, `0..1` turns: interleaved
/// gradient noise (Jimenez 2014) over the integer pixel. The blur removes
/// the pattern it leaves.
pub fn spin(px: [f32; 2]) -> f32 {
    let x = (0.067_110_56 * px[0] + 0.005_837_15 * px[1]).fract();
    (52.982_918 * x).fract()
}

/// Sample `i` of `n` on the unit disc: a golden-angle spiral rotated by
/// `spin` turns, its radius `sqrt((i + 0.5) / n)` so the samples cover the
/// disc evenly.
pub fn sample_offset(i: u32, n: u32, spin: f32) -> [f32; 2] {
    let t = (i as f32 + 0.5) / n.max(1) as f32;
    let r = t.sqrt();
    let a = i as f32 * GOLDEN_ANGLE + spin * std::f32::consts::TAU;
    [r * a.cos(), r * a.sin()]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// One sample's occlusion: `v` the vector from the shaded point to the
/// sample's view position, `n` the shaded point's unit view normal,
/// `view_z` its depth. The Alchemy term `max(0, v.n - bias*z) / (v.v + e)`,
/// scaled by the radius so it is unit-free, faded to zero as `|v|` reaches
/// the radius (what keeps a far wall behind a silhouette from haloing it).
pub fn occlusion_term(p: &SsaoParams, v: [f32; 3], n: [f32; 3], view_z: f32) -> f32 {
    let vv = dot(v, v);
    let r2 = p.radius * p.radius;
    let falloff = (1.0 - vv / r2).max(0.0);
    let num = (dot(v, n) - p.bias * view_z).max(0.0);
    p.radius * num / (vv + 0.01 * r2) * falloff
}

/// The visibility multiplier from the summed [`occlusion_term`]s of `n`
/// samples: `1 - intensity * mean`, raised to `power`, floored at
/// `min_ao`. `1.0` is unoccluded.
pub fn ao_from_sum(p: &SsaoParams, sum: f32, n: u32) -> f32 {
    let a = (1.0 - p.intensity * sum / n.max(1) as f32).clamp(0.0, 1.0);
    a.powf(p.power).max(p.min_ao)
}

/// One blur tap's weight: Gaussian in the tap's offset `t` (AO texels),
/// times a relative-depth term and a normal-agreement term, so the blur
/// stops at silhouettes and creases instead of bleeding across them.
/// `z0` / `n0` are the centre's depth and normal, `z` / `n` the tap's.
pub fn blur_weight(p: &SsaoParams, t: f32, z0: f32, z: f32, n0: [f32; 3], n: [f32; 3]) -> f32 {
    if z <= 0.0 {
        return 0.0;
    }
    let g = (-(t * t) / (2.0 * BLUR_SIGMA * BLUR_SIGMA)).exp();
    let dz = (z - z0).abs() / (z0.max(1e-3) * p.blur_depth_sigma);
    let dw = (-(dz * dz)).exp();
    let nw = dot(n0, n).max(0.0).powf(p.blur_normal_power);
    g * dw * nw
}

/// As the four `vec4`s the shaders read: `(radius, bias, intensity,
/// power)`, `(min_ao, max_radius_frac, min_radius_px, samples)`,
/// `(blur_radius, blur_depth_sigma, blur_normal_power, blur_sigma)`,
/// `(contact_length, contact_thickness, contact_steps, contact_bias)`.
pub fn shader_words(p: &SsaoParams) -> [[f32; 4]; 4] {
    [
        [p.radius, p.bias, p.intensity, p.power],
        [
            p.min_ao,
            p.max_radius_frac,
            p.min_radius_px,
            p.samples as f32,
        ],
        [
            p.blur_radius as f32,
            p.blur_depth_sigma,
            p.blur_normal_power,
            BLUR_SIGMA,
        ],
        [
            p.contact_length,
            p.contact_thickness,
            p.contact_steps as f32,
            p.contact_bias,
        ],
    ]
}

/// The frame's contact-shadow word `(light_view.xyz, strength)`.
///
/// `key_dir` is the mood's key light (the direction *toward* the light, in
/// the retail Y-down frame, so "from above" is `-Y`). `camera_y_down` says
/// which frame the staged camera takes points in: the field and world-map
/// cameras take the Y-up render frame, a battle's camera composed with the
/// battle stage model takes retail's Y-down one. The direction goes to view
/// space through the same rows [`view_scale`] measures, so it lands in the
/// frame [`view_position`] reconstructs. `strength` is
/// [`SsaoParams::contact_strength`] scaled by the key light's luminance
/// against [`CONTACT_FULL_KEY_LUMA`]; zero while `enhanced` (enhanced
/// lighting) is off, which turns the march off.
pub fn contact_light(
    p: &SsaoParams,
    view_proj: &[f32; 16],
    key_dir: [f32; 3],
    key_rgb: [f32; 3],
    camera_y_down: bool,
    enhanced: bool,
) -> [f32; 4] {
    let k = view_scale(view_proj);
    let d = [
        key_dir[0],
        if camera_y_down {
            key_dir[1]
        } else {
            -key_dir[1]
        },
        key_dir[2],
    ];
    let row = |r: usize| view_proj[r] * d[0] + view_proj[4 + r] * d[1] + view_proj[8 + r] * d[2];
    let v = [row(0) * k[0], row(1) * k[1], row(3) * k[2]];
    let len = dot(v, v).sqrt();
    if !enhanced || len < 1e-6 {
        return [0.0; 4];
    }
    let luma = 0.299 * key_rgb[0] + 0.587 * key_rgb[1] + 0.114 * key_rgb[2];
    let strength = p.contact_strength * (luma / CONTACT_FULL_KEY_LUMA).clamp(0.0, 1.0);
    [v[0] / len, v[1] / len, v[2] / len, strength]
}

/// The NDC of a view-space point: the inverse of [`view_position`].
pub fn view_to_ndc(q: [f32; 3], k: [f32; 3]) -> [f32; 2] {
    let w = q[2] / k[2];
    [q[0] / (w * k[0]), q[1] / (w * k[1])]
}

/// Whether a ray sample at view depth `ray_z` sits behind the G-buffer
/// surface at `scene_z` (both world units along the view axis): further
/// than the bias and nearer than the thickness. `1.0` shadowed, `0.0` not.
pub fn contact_hit(p: &SsaoParams, ray_z: f32, scene_z: f32) -> f32 {
    let dz = ray_z - scene_z;
    if dz > p.contact_bias * ray_z && dz < p.contact_thickness {
        1.0
    } else {
        0.0
    }
}

/// A hit's weight at distance `t` along the ray: full at the surface,
/// fading to zero at [`SsaoParams::contact_length`], so the shadow is a
/// soft-ended contact sliver rather than a hard-edged cast shadow.
pub fn contact_fade(p: &SsaoParams, t: f32) -> f32 {
    (1.0 - t / p.contact_length.max(1e-3)).clamp(0.0, 1.0)
}

/// The multiplier a march leaves: `hit` the strongest faded hit, `ndl` the
/// surface normal's dot product with the light (a face turned from the
/// light is the baked shading's, and takes none), `strength` the frame's
/// [`contact_light`] strength.
pub fn contact_from_hit(hit: f32, ndl: f32, strength: f32) -> f32 {
    let facing = (ndl * 4.0).clamp(0.0, 1.0);
    1.0 - strength * hit.clamp(0.0, 1.0) * facing
}

#[cfg(test)]
mod tests {
    use super::*;

    const UP: [f32; 3] = [0.0, 1.0, 0.0];

    fn ao_of(points: &[[f32; 3]], n: [f32; 3], z: f32) -> f32 {
        let p = SsaoParams::default();
        let sum: f32 = points.iter().map(|&v| occlusion_term(&p, v, n, z)).sum();
        ao_from_sum(&p, sum, points.len() as u32)
    }

    #[test]
    fn a_flat_floor_is_unoccluded() {
        let pts: Vec<[f32; 3]> = (0..12)
            .map(|i| {
                let o = sample_offset(i, 12, 0.3);
                [o[0] * 40.0, 0.0, o[1] * 40.0]
            })
            .collect();
        assert_eq!(ao_of(&pts, UP, 500.0), 1.0);
    }

    #[test]
    fn a_corner_darkens_and_floors_at_min_ao() {
        // Half the samples on a wall rising beside the point.
        let pts: Vec<[f32; 3]> = (0..12)
            .map(|i| {
                let o = sample_offset(i, 12, 0.0);
                if i % 2 == 0 {
                    [o[0] * 40.0, 0.0, o[1] * 40.0]
                } else {
                    [10.0, 20.0 + o[1].abs() * 20.0, o[0] * 20.0]
                }
            })
            .collect();
        let a = ao_of(&pts, UP, 500.0);
        assert!(a < 0.9, "corner ao {a}");
        assert!(a >= MIN_AO);
    }

    #[test]
    fn samples_past_the_radius_do_not_occlude() {
        let p = SsaoParams::default();
        let far = [0.0, RADIUS * 1.01, 0.0];
        assert_eq!(occlusion_term(&p, far, UP, 500.0), 0.0);
        let near = [0.0, RADIUS * 0.3, 0.0];
        assert!(occlusion_term(&p, near, UP, 500.0) > 0.0);
    }

    #[test]
    fn the_spiral_stays_on_the_unit_disc_and_spin_is_a_fraction() {
        for i in 0..SAMPLES {
            let o = sample_offset(i, SAMPLES, 0.77);
            assert!(o[0].hypot(o[1]) <= 1.0 + 1e-6);
        }
        for (x, y) in [(0.0, 0.0), (319.0, 239.0), (1919.0, 1079.0)] {
            let s = spin([x, y]);
            assert!((0.0..1.0).contains(&s));
        }
    }

    #[test]
    fn view_scale_recovers_world_units() {
        use glam::{Mat4, Vec3, Vec4};
        let fov = 50f32.to_radians();
        let proj = glam::camera::rh::proj::directx::perspective(fov, 4.0 / 3.0, 1.0, 10_000.0);
        let view = glam::camera::rh::view::look_at_mat4(
            Vec3::new(300.0, -400.0, 900.0),
            Vec3::ZERO,
            Vec3::Y,
        );
        // A uniformly scaled world (the field frame carries one).
        let vp = proj * Mat4::from_scale(Vec3::splat(1.0 / 6.0)) * view;
        let k = view_scale(&vp.to_cols_array());
        let a = Vec4::new(10.0, 20.0, 30.0, 1.0);
        let b = Vec4::new(70.0, -5.0, 12.0, 1.0);
        let to_view = |p: Vec4| {
            let c = vp * p;
            Vec3::from(view_position([c.x / c.w, c.y / c.w], c.w, k))
        };
        let d_view = to_view(a).distance(to_view(b));
        let d_world = a.truncate().distance(b.truncate());
        assert!(
            (d_view - d_world).abs() < 1e-2 * d_world,
            "{d_view} vs {d_world}"
        );
    }

    #[test]
    fn the_radius_projects_and_clamps() {
        let p = SsaoParams::default();
        let k = [1.0 / 2.0, 1.0 / 2.0, 1.0];
        let near = radius_px(&p, 1.0, k, 240.0);
        assert_eq!(near, MAX_RADIUS_FRAC * 240.0);
        let far = radius_px(&p, 100_000.0, k, 240.0);
        assert!(far < 1.0);
        assert_eq!(radius_px(&p, 0.0, k, 240.0), 0.0);
    }

    #[test]
    fn the_blur_stops_at_a_depth_step_and_a_crease() {
        let p = SsaoParams::default();
        let same = blur_weight(&p, 2.0, 500.0, 501.0, UP, UP);
        let step = blur_weight(&p, 2.0, 500.0, 600.0, UP, UP);
        let crease = blur_weight(&p, 2.0, 500.0, 501.0, UP, [1.0, 0.0, 0.0]);
        assert!(same > 0.5);
        assert!(step < 1e-6);
        assert_eq!(crease, 0.0);
        assert_eq!(blur_weight(&p, 0.0, 500.0, 0.0, UP, UP), 0.0);
    }

    #[test]
    fn contact_light_points_up_in_view_space_and_is_off_without_lighting() {
        use glam::{Mat4, Vec3};
        let proj = glam::camera::rh::proj::directx::perspective(0.9, 4.0 / 3.0, 1.0, 10_000.0);
        // A Y-up camera looking down at the origin from above and behind.
        let view =
            glam::camera::rh::view::look_at_mat4(Vec3::new(0.0, 800.0, 900.0), Vec3::ZERO, Vec3::Y);
        let vp = (proj * view).to_cols_array();
        let p = SsaoParams::default();
        let sun = [0.0, -1.0, 0.0];
        let l = contact_light(&p, &vp, sun, [0.45, 0.42, 0.36], false, true);
        // Straight up in the world is up on screen (+y) and toward the
        // camera's eye (-z, since view z grows away from the eye).
        assert!(l[1] > 0.5 && l[2] < 0.0, "{l:?}");
        assert!((l[3] - CONTACT_STRENGTH).abs() < 0.02, "{l:?}");
        // The same light through a Y-down camera input (a battle's camera
        // composed with the stage model's Y flip) is the same view vector.
        let flip = (proj * view * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))).to_cols_array();
        let l2 = contact_light(&p, &flip, sun, [0.45, 0.42, 0.36], true, true);
        for i in 0..3 {
            assert!((l[i] - l2[i]).abs() < 1e-5);
        }
        assert_eq!(
            contact_light(&p, &vp, sun, [0.45; 3], false, false),
            [0.0; 4]
        );
    }

    #[test]
    fn view_to_ndc_inverts_view_position() {
        let k = [0.0021, 0.0027, 0.166];
        let q = view_position([0.3, -0.6], 900.0, k);
        let n = view_to_ndc(q, k);
        assert!((n[0] - 0.3).abs() < 1e-5 && (n[1] + 0.6).abs() < 1e-5);
    }

    #[test]
    fn contact_hits_only_inside_the_thickness_and_fade() {
        let p = SsaoParams::default();
        assert_eq!(
            contact_hit(&p, 500.0, 520.0),
            0.0,
            "in front of the surface"
        );
        assert_eq!(contact_hit(&p, 520.0, 500.0), 1.0, "just behind it");
        assert_eq!(contact_hit(&p, 500.0 + CONTACT_THICKNESS * 2.0, 500.0), 0.0);
        assert_eq!(contact_fade(&p, 0.0), 1.0);
        assert_eq!(contact_fade(&p, CONTACT_LENGTH), 0.0);
        assert_eq!(contact_from_hit(1.0, -0.5, 0.5), 1.0, "a face turned away");
        assert_eq!(contact_from_hit(1.0, 1.0, 0.5), 0.5);
        assert_eq!(contact_from_hit(0.0, 1.0, 0.5), 1.0);
    }

    #[test]
    fn full_resolution_on_the_native_target() {
        assert_eq!(ao_downscale(320, 240), 1);
        assert_eq!(ao_downscale(960, 720), 2);
    }
}
