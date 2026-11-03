//! The retail PSX GTE camera as one view-projection matrix - the projection
//! half every host shares.
//!
//! Retail projects a world point through
//! `screen = H * (R * (v - focus) + tr_eye) / Ez` with
//! `R = Rx(pitch) * Ry(yaw) * Rz(roll)` (the view build `FUN_800172C0` forms
//! it through the Euler kernel `FUN_80026988`) and the GTE control file's screen
//! centre `(OFX, OFY)`. [`psx_camera_vp`] is that transform written as one
//! column-major 4x4, so a host never re-derives it - it feeds the inputs and
//! uploads the matrix.
//!
//! **Frame convention.** Every matrix this module returns is for the **Y-up**
//! render frame: the caller's model matrices carry the PSX `scale(1,-1,1)`
//! (the browser `placementModelScaledY` convention), and the trailing flip
//! factor here cancels it so the retail chain sees the raw Y-down vertex. A
//! host that instead keeps its world state in raw retail Y-down coordinates -
//! the native window's field frame - post-multiplies one more `scale(1,-1,1)`
//! (`FIELD_WORLD_FLIP`) and lands on the same net transform. That one rule
//! covers the field follow camera, the op-`0x45` cutscene shots, the overworld
//! walk view and [`battle_vp`](crate::projection::battle_vp) alike.
//!
//! [`CutsceneCameraInterp`] is the glide between op-`0x45` Configure beats.
//! It lives here rather than in the renderer so both the native window and the
//! browser play page ease the same shot - a host that reads the staged params
//! straight out of the world snaps every `apply > 0` beat.
//!
//! REF: FUN_80026988 (the camera's Euler kernel), FUN_800172C0 (the view
//! composition), FUN_801DE084 (the op-`0x45` Configure apply handler).

use crate::projection::{GTE_OFY_NDC_BIAS, PSX_NEAR, SCENE_FAR};

/// Column-major 4x4 multiply: `out = a * b` (the layout WebGL `mat4`,
/// `glam::Mat4::to_cols_array` and this crate's own camera kernels all use).
pub fn mat4_mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for c in 0..4 {
        for r in 0..4 {
            let mut s = 0.0;
            for k in 0..4 {
                s += a[k * 4 + r] * b[c * 4 + k];
            }
            out[c * 4 + r] = s;
        }
    }
    out
}

/// Column-major translation matrix.
pub fn mat4_translation(t: [f32; 3]) -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        t[0], t[1], t[2], 1.0,
    ]
}

/// Column-major uniform-scale matrix.
pub fn mat4_scale(s: f32) -> [f32; 16] {
    [
        s, 0.0, 0.0, 0.0, //
        0.0, s, 0.0, 0.0, //
        0.0, 0.0, s, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ]
}

/// The single world-space Y negation that converts between the Y-up render
/// frame every kernel here returns and the raw retail Y-down world frame.
/// Its own inverse.
pub const WORLD_FLIP: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, -1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

/// The retail camera rotation `R = Rx(pitch) * Ry(yaw) * Rz(roll)`, the
/// product the Euler kernel `FUN_80026988` forms when `FUN_800172C0` builds
/// the view. Column-major, rotation-only, in `f32`: retail's q3.12 LUT product
/// (`battle_intro::euler_rot_psx` in `legaia-engine-ui`) differs from it by at
/// most about 2.4/4096 per element, which moves a projected point by a
/// median of under 0.2 px and by up to about 3 px for geometry near the eye
/// of a long cutscene shot (see `docs/subsystems/renderer.md`).
pub fn camera_rotation(pitch_rad: f32, yaw_rad: f32, roll_rad: f32) -> [f32; 16] {
    let (sp, cp) = pitch_rad.sin_cos();
    let (sy, cy) = yaw_rad.sin_cos();
    let (sr, cr) = roll_rad.sin_cos();
    let rx: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, //
        0.0, cp, sp, 0.0, //
        0.0, -sp, cp, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];
    let ry: [f32; 16] = [
        cy, 0.0, -sy, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        sy, 0.0, cy, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];
    let rz: [f32; 16] = [
        cr, sr, 0.0, 0.0, //
        -sr, cr, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];
    mat4_mul(&rx, &mat4_mul(&ry, &rz))
}

/// The PSX perspective onto the 320x240 frame, column-major:
/// `ndc.x = H*Ex/(160*Ez)`, `ndc.y = -H*Ey/(120*Ez) + `[`GTE_OFY_NDC_BIAS`],
/// `clip.w = Ez`, depth mapped `[PSX_NEAR, SCENE_FAR] -> [0, 1]`. `aspect`
/// corrects X so the 4:3 retail framing holds at any viewport size.
pub fn psx_projection(h: f32, aspect: f32) -> [f32; 16] {
    let (near, far) = (PSX_NEAR, SCENE_FAR);
    let a = far / (far - near);
    let b = -near * far / (far - near);
    let aspect_fix = (4.0 / 3.0) / aspect.max(0.01);
    [
        h / 160.0 * aspect_fix,
        0.0,
        0.0,
        0.0, //
        0.0,
        -h / 120.0,
        0.0,
        0.0, //
        0.0,
        GTE_OFY_NDC_BIAS,
        a,
        1.0, //
        0.0,
        0.0,
        b,
        0.0,
    ]
}

/// The full retail GTE view-projection for one camera pose, column-major, in
/// the **Y-up render frame** (see the module docs).
///
/// `focus` is the world point the camera orbits, in raw retail Y-down
/// coordinates - the form the op-`0x45` focus globals decode to and the form
/// the follow camera's player anchor is already in. `tr` is the eye-space
/// translation trio (op-`0x45` slots 3/4/5), `h` the GTE `H` register.
#[allow(clippy::too_many_arguments)]
pub fn psx_camera_vp(
    pitch_rad: f32,
    yaw_rad: f32,
    roll_rad: f32,
    h: f32,
    tr: [f32; 3],
    focus: [f32; 3],
    aspect: f32,
) -> [f32; 16] {
    let r = camera_rotation(pitch_rad, yaw_rad, roll_rad);
    let t = mat4_translation(tr);
    let neg_focus = mat4_translation([-focus[0], -focus[1], -focus[2]]);
    let proj = psx_projection(h, aspect);
    mat4_mul(
        &proj,
        &mat4_mul(&t, &mat4_mul(&r, &mat4_mul(&neg_focus, &WORLD_FLIP))),
    )
}

/// World-space **eye position** of [`psx_camera_vp`]'s pose, in raw retail
/// Y-down world coordinates.
///
/// The view on raw world coordinates is `R * (v - focus) + tr`, so the eye -
/// the point that maps to the eye-space origin - is `focus - R^T * tr`.
/// Consumed by the camera-occlusion fade's visibility gate, which ray-casts
/// eye->player against the static scene geometry in that same frame.
pub fn psx_camera_eye(
    pitch_rad: f32,
    yaw_rad: f32,
    roll_rad: f32,
    tr: [f32; 3],
    focus: [f32; 3],
) -> [f32; 3] {
    let r = camera_rotation(pitch_rad, yaw_rad, roll_rad);
    // `(R^T * tr)_i = sum_j R[j][i] * tr[j]`; column-major `r[c*4 + row]`
    // stores `R[row][c]`, so `R[j][i] = r[i*4 + j]`.
    let mut out = [0.0f32; 3];
    for (i, o) in out.iter_mut().enumerate() {
        let mut s = 0.0;
        for (j, &t) in tr.iter().enumerate() {
            s += r[i * 4 + j] * t;
        }
        *o = focus[i] - s;
    }
    out
}

/// The inputs one frame of a retail-model camera resolves to: the ten
/// op-`0x45` camera globals reduced to the six the projection reads.
///
/// Both hosts build this from the same `engine-core` resolvers and then do
/// exactly two things with it - [`Self::vp`] for the matrix they upload and
/// [`Self::eye`] for the world-space lens the occlusion gate ray-casts from.
/// Nothing host-side re-derives an angle, a depth or a focal length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldCameraView {
    /// The world point the camera orbits, raw retail Y-down.
    pub focus: [f32; 3],
    /// `_DAT_8007B790`, radians.
    pub pitch: f32,
    /// `_DAT_8007B792`, radians.
    pub yaw: f32,
    /// `_DAT_8007B794`, radians.
    pub roll: f32,
    /// GTE `H` (`_DAT_8007B6F4`).
    pub h: f32,
    /// The eye-space translation trio (`_DAT_800840B8/BC/C0`).
    pub tr_eye: [f32; 3],
}

impl FieldCameraView {
    /// This pose's view-projection, column-major, Y-up render frame.
    pub fn vp(&self, aspect: f32) -> [f32; 16] {
        psx_camera_vp(
            self.pitch,
            self.yaw,
            self.roll,
            self.h,
            self.tr_eye,
            self.focus,
            aspect,
        )
    }

    /// This pose's world-space eye, raw retail Y-down.
    pub fn eye(&self) -> [f32; 3] {
        psx_camera_eye(self.pitch, self.yaw, self.roll, self.tr_eye, self.focus)
    }

    /// A raw-world point's **eye-space** position under this pose,
    /// `R * (p - focus) + tr` - the vector the GTE divides by to place it on
    /// screen (`H * xy / z`). The screen point of `p` is a function of this
    /// vector's direction only, which is what lets a user knob re-pivot the
    /// pose about `p` (the character) without moving `p` on screen.
    pub fn eye_space(&self, p: [f32; 3]) -> [f32; 3] {
        let r = camera_rotation(self.pitch, self.yaw, self.roll);
        let d = [
            p[0] - self.focus[0],
            p[1] - self.focus[1],
            p[2] - self.focus[2],
        ];
        let mut out = self.tr_eye;
        // Column-major `r[c*4 + row]`: `(R d)_row = sum_c R[row][c] d[c]`.
        for (row, o) in out.iter_mut().enumerate() {
            for (c, &dc) in d.iter().enumerate() {
                *o += r[c * 4 + row] * dc;
            }
        }
        out
    }
}

/// Near / far planes for the synthetic orbit-family cameras (the debug
/// vantages), from the framing distance. The near plane is the only clip that
/// can make close geometry vanish, so it stays at a few units and clamps into
/// `[0.05, 8.0]`.
pub fn orbit_clip_planes(distance: f32) -> (f32, f32) {
    ((distance * 0.005).clamp(0.05, 8.0), SCENE_FAR)
}

/// Right-handed look-at, column-major - the same basis
/// `glam::camera::rh::view::look_at_mat4` and the site's `lookAt` build.
pub fn look_at_rh(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [f32; 16] {
    let mut f = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
    let fl = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt().max(1e-6);
    for v in &mut f {
        *v /= fl;
    }
    let mut s = [
        f[1] * up[2] - f[2] * up[1],
        f[2] * up[0] - f[0] * up[2],
        f[0] * up[1] - f[1] * up[0],
    ];
    let sl = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt().max(1e-6);
    for v in &mut s {
        *v /= sl;
    }
    let u = [
        s[1] * f[2] - s[2] * f[1],
        s[2] * f[0] - s[0] * f[2],
        s[0] * f[1] - s[1] * f[0],
    ];
    [
        s[0],
        u[0],
        -f[0],
        0.0,
        s[1],
        u[1],
        -f[1],
        0.0,
        s[2],
        u[2],
        -f[2],
        0.0,
        -(s[0] * eye[0] + s[1] * eye[1] + s[2] * eye[2]),
        -(u[0] * eye[0] + u[1] * eye[1] + u[2] * eye[2]),
        f[0] * eye[0] + f[1] * eye[1] + f[2] * eye[2],
        1.0,
    ]
}

/// Right-handed perspective with depth in `[0, 1]`, column-major - the same
/// matrix `glam::camera::rh::proj::directx::perspective` builds.
pub fn perspective_rh(fov_y: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
    let (sin_fov, cos_fov) = (0.5 * fov_y).sin_cos();
    let h = cos_fov / sin_fov;
    let w = h / aspect.max(0.01);
    let r = far / (near - far);
    [
        w,
        0.0,
        0.0,
        0.0, //
        0.0,
        h,
        0.0,
        0.0, //
        0.0,
        0.0,
        r,
        -1.0, //
        0.0,
        0.0,
        r * near,
        0.0,
    ]
}

/// The **top-view debug camera** of the world map, column-major, Y-up render
/// frame.
///
/// Not a retail GTE camera: retail's top-view block re-purposes the same four
/// globals (`DAT_801F2B94` mode, `_DAT_80089118/20` scroll, `_DAT_8007B794`
/// azimuth, `_DAT_8007B6F4` zoom) as a survey vantage, and the port frames it
/// as an elevated orbit around the loaded pack. The controller that owns those
/// four values is `legaia_engine_field::world_map::WorldMapController`, live on
/// every host, so both hosts pass the same four numbers here.
///
/// - `aabb_lo` / `aabb_hi` - the scene's **world-space** bounding box (the
///   union of the static env draws under their placement transforms).
/// - `azimuth` - PSX angle units (`4096` = full turn).
/// - `zoom` - positive pulls the camera in, negative pushes it out.
/// - `pan_x` / `pan_z` - the top-view scroll, in world units.
pub fn world_map_top_view_vp(
    aabb_lo: [f32; 3],
    aabb_hi: [f32; 3],
    azimuth: i32,
    zoom: i32,
    pan_x: i32,
    pan_z: i32,
    aspect: f32,
) -> [f32; 16] {
    // The pack draws Y-flipped, so its drawn Y-range is `[-hi.y, -lo.y]`;
    // frame that flipped centre, offset by the top-view pan. (Framing the raw
    // box puts the eye on the opposite Y side and renders the map from
    // underneath.)
    let center = [
        (aabb_lo[0] + aabb_hi[0]) * 0.5 + pan_x as f32,
        -(aabb_lo[1] + aabb_hi[1]) * 0.5,
        (aabb_lo[2] + aabb_hi[2]) * 0.5 + pan_z as f32,
    ];
    let d = [
        aabb_hi[0] - aabb_lo[0],
        aabb_hi[1] - aabb_lo[1],
        aabb_hi[2] - aabb_lo[2],
    ];
    let radius = (((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()) * 0.5).max(1.0);
    let base_distance = radius / 30f32.to_radians().tan() * 1.6;
    // `zoom` accrues in steps of 4, so 512 is a wide usable band; clamp so the
    // player can neither invert the view nor fly infinitely far out.
    let zoom_mult = (1.0 - (zoom as f32) / 512.0).clamp(0.25, 3.0);
    let distance = base_distance * zoom_mult;
    let angle = (azimuth as f32) / 4096.0 * std::f32::consts::TAU;
    let eye = [
        center[0] + distance * angle.cos(),
        center[1] + distance * 0.7,
        center[2] + distance * angle.sin(),
    ];
    let view = look_at_rh(eye, center, [0.0, 1.0, 0.0]);
    let (near, far) = orbit_clip_planes(distance);
    let proj = perspective_rh(60f32.to_radians(), aspect, near, far);
    mat4_mul(&proj, &view)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The projection is the retail GTE transform, not an approximation of
    /// one: project a handful of points through the matrix and compare with
    /// `screen = H*(R*(v - focus) + tr)/Ez + (OFX, OFY)` computed by hand.
    #[test]
    fn psx_camera_vp_matches_the_handrolled_retail_projection() {
        use crate::projection::{GTE_OFX, GTE_OFY};
        let (pitch, yaw, roll) = (0.69f32, -0.245f32, 0.0f32);
        let tr = [0.0f32, 0.0, 1200.0];
        let focus = [820.0f32, -160.0, 2400.0];
        let h = 512.0f32;
        let vp = psx_camera_vp(pitch, yaw, roll, h, tr, focus, 4.0 / 3.0);
        for v in [
            [900.0f32, -60.0, 2600.0],
            [-300.0, 0.0, 1000.0],
            [820.0, -160.0, 2400.0],
        ] {
            // Hand-rolled retail chain on the raw Y-down vertex.
            let p = [v[0] - focus[0], v[1] - focus[1], v[2] - focus[2]];
            let (sy, cy) = yaw.sin_cos();
            let (sp, cp) = pitch.sin_cos();
            let ry = [cy * p[0] + sy * p[2], p[1], -sy * p[0] + cy * p[2]];
            let e = [ry[0], cp * ry[1] - sp * ry[2], sp * ry[1] + cp * ry[2]];
            let ez = e[2] + tr[2];
            assert!(ez > 1.0);
            let want = (
                h * (e[0] + tr[0]) / ez + GTE_OFX,
                h * (e[1] + tr[1]) / ez + GTE_OFY,
            );
            // Through the matrix, on the Y-UP vertex (the caller's model
            // matrices carry the flip this vp cancels).
            let m = vp;
            let vu = [v[0], -v[1], v[2]];
            let cx = m[0] * vu[0] + m[4] * vu[1] + m[8] * vu[2] + m[12];
            let cyc = m[1] * vu[0] + m[5] * vu[1] + m[9] * vu[2] + m[13];
            let cw = m[3] * vu[0] + m[7] * vu[1] + m[11] * vu[2] + m[15];
            let got = (160.0 * (1.0 + cx / cw), 120.0 * (1.0 - cyc / cw));
            assert!(
                (got.0 - want.0).abs() < 0.01 && (got.1 - want.1).abs() < 0.01,
                "v={v:?} got={got:?} want={want:?}"
            );
        }
    }

    /// The analytic eye is the point the view maps to the eye-space origin.
    #[test]
    fn psx_camera_eye_inverts_the_view_composition() {
        let (pitch, yaw, roll) = (0.69f32, -0.245f32, 0.31f32);
        let tr = [40.0f32, -90.0, 1200.0];
        let focus = [820.0f32, -160.0, 2400.0];
        let eye = psx_camera_eye(pitch, yaw, roll, tr, focus);
        // R*(eye - focus) + tr should be the origin.
        let r = camera_rotation(pitch, yaw, roll);
        let d = [eye[0] - focus[0], eye[1] - focus[1], eye[2] - focus[2]];
        for row in 0..3 {
            let mut s = tr[row];
            for (c, &dc) in d.iter().enumerate() {
                s += r[c * 4 + row] * dc;
            }
            assert!(s.abs() < 1e-2, "row {row} = {s}");
        }
    }

    /// `WORLD_FLIP` is its own inverse, which is what lets one kernel serve a
    /// Y-up host and a raw-Y-down host.
    #[test]
    fn world_flip_is_an_involution() {
        let m = mat4_mul(&WORLD_FLIP, &WORLD_FLIP);
        for (i, v) in m.iter().enumerate() {
            let want = if i % 5 == 0 { 1.0 } else { 0.0 };
            assert_eq!(*v, want, "element {i}");
        }
    }

    #[test]
    fn top_view_zoom_and_pan_move_the_frame() {
        let lo = [-4000.0f32, -300.0, -4000.0];
        let hi = [4000.0f32, 300.0, 4000.0];
        let a = world_map_top_view_vp(lo, hi, 0, 0, 0, 0, 4.0 / 3.0);
        let b = world_map_top_view_vp(lo, hi, 0, 64, 0, 0, 4.0 / 3.0);
        let c = world_map_top_view_vp(lo, hi, 0, 0, 500, 0, 4.0 / 3.0);
        assert!(a.iter().all(|v| v.is_finite()));
        assert!(a != b, "zoom must change the framing");
        assert!(a != c, "pan must change the framing");
    }
}
