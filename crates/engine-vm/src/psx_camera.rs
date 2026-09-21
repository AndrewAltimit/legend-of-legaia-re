//! The retail PSX GTE camera as one view-projection matrix - the projection
//! half every host shares.
//!
//! Retail projects a world point through
//! `screen = H * (R * (v - focus) + tr_eye) / Ez` with
//! `R = Rx(pitch) * Ry(yaw) * Rz(roll)` (`FUN_8001CF50` post-multiplies the
//! three `RotMatrix*` factors in that order) and the GTE control file's screen
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
//! walk view and [`battle_vp`](crate::battle_cam_script::battle_vp) alike.
//!
//! [`CutsceneCameraInterp`] is the glide between op-`0x45` Configure beats.
//! It lives here rather than in the renderer so both the native window and the
//! browser play page ease the same shot - a host that reads the staged params
//! straight out of the world snaps every `apply > 0` beat.
//!
//! REF: FUN_8001CF50 (the camera-rotation build), FUN_800172C0 (the view
//! composition), FUN_801DE084 (the op-`0x45` Configure apply handler).

use crate::battle_cam_script::{GTE_OFY_NDC_BIAS, PSX_NEAR, SCENE_FAR};

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
/// product `FUN_8001CF50` builds by post-multiplying each axis factor onto the
/// resident rotation. Column-major, rotation-only.
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
/// `glam::Mat4::look_at_rh` and the site's `lookAt` build.
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
/// matrix `glam::Mat4::perspective_rh` builds.
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
/// four values is `legaia_engine_core::world_map::WorldMapController`, live on
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

/// Glides the cutscene camera between op-`0x45` Camera Configure beats.
///
/// A host that frames whatever the timeline's *current* params decode to
/// re-targets the shot instantly and the camera snaps. Retail stages each
/// Configure's params into a persistent control block and moves the live
/// camera globals toward them over the beat's `apply_trigger` frames; this
/// mirrors that per component.
///
/// Ten components (focus xyz, pitch, yaw, H, eye-trio xyz, roll) each carry
/// their own in-flight glide: when a component's target changes, a glide is
/// armed from the CURRENT pose over the staging beat's `apply` frames
/// (`apply == 0` commits that component immediately - the snap cut).
/// Components whose targets did not change keep their in-flight glide
/// untouched, so a follow-up single-slot poke (opdeene re-stages H alone one
/// frame after arming its 480-frame tableau dolly) cannot cancel the dolly -
/// the earlier whole-tuple ease-rate model snapped the entire shot on exactly
/// that poke, which is what planted the eye inside the crater-rim geometry.
///
/// Motion arrives exactly, advanced in SIM ticks by the caller (`steps`).
/// The mover law is pinned from a per-frame RAM capture of the live camera
/// globals (`0x8007B790` angle trio / `0x800840B8` eye trio /
/// `0x80089118` focus trio) across the whole retail New-Game opening chain:
///
/// - **Duration**: `apply` IS the glide length in retail frames, 1:1
///   (measured arrivals 48/50, 85/90, 239/240, ~965/1000, ~900/900), and
///   the engine's sim tick counts retail frames 1:1 (`WaitFrames` targets
///   drain one per tick), so a glide spans exactly `apply` sim ticks.
///   A long `apply` is a *dolly velocity* spec, not a promise of arrival:
///   opurud stages an `apply 2300` eye glide whose next snap beat lands
///   ~1/4 of the way through - retail never reaches that staged target.
///   (The earlier `apply / 3.5` reading compressed those dollies ~6x, so
///   the engine ARRIVED at extreme staged eye targets retail only drifts
///   toward - parking the camera inside scene geometry.)
/// - **Shape**: the beat's decoded `mode` nibble (`op0 >> 2 & 0xF`) selects
///   the ease curve, and retail applies that ONE curve to **all ten axes** -
///   the angles included. The curves are
///   [`crate::camera_mover::curve_unit`]: `1` (and any unlisted value)
///   linear, `2` quadratic ease-out, `3` quadratic ease-in, `4` ease-in-out
///   built from a quad-in half and a quad-out half. The earlier "`mode 1`
///   eases the angles out while the eye trio runs linear" split is
///   FALSIFIED - it does not exist in the mover, which reads the same curve
///   word once per axis; `mode 4` is likewise the two-half integer curve,
///   not smoothstep, and `mode 3` was missing entirely.
///
/// Angles glide along the shortest arc here, which is a **port convenience**,
/// not retail: the mover lerps the raw 12-bit angle words with no wrap
/// handling, so a beat staging a crossing of the `4096`-unit wrap travels the
/// long way round in retail. No opening-chain beat stages such a crossing.
/// [`Self::reset`] makes the next [`Self::glide`] snap directly to the target -
/// call it when a cutscene (re)starts so the opening shot doesn't sweep in
/// from a stale pose.
///
/// This type still arms glides **per component**, where retail's
/// `FUN_801DD310` re-seeds all ten axes from the live pose and resets the one
/// shared progress counter on every apply beat. See
/// `docs/subsystems/cutscene.md` for that open divergence.
///
/// PORT: FUN_801DC0BC - retail's per-frame camera mover, ported exactly (in
/// its native integer arithmetic) as [`crate::camera_mover`]; this type is the
/// `f32` rendition of the same shapes.
/// REF: FUN_801DD310 - allocates / re-seeds the mover actor's pair block.
/// REF: FUN_801DE084 - the op-`0x45` Configure apply handler (per-slot
/// staging into the persistent control block; `apply == 0` = snap, which also
/// kills any mover in flight).
/// The *dialog* overlay's copy is this same function (the dialog and
/// cutscene_dialogue dumps are instruction-identical); only the menu overlay
/// hosts different code at this VA - see `docs/reference/functions.md`.
#[derive(Debug, Clone, Default)]
pub struct CutsceneCameraInterp {
    /// Current pose, packed `[look_at xyz, pitch, yaw, h, tr_eye xyz, roll]`.
    cur: [f32; 10],
    /// Per-component glide start pose (the pose when the target last changed).
    start: [f32; 10],
    /// Per-component staged target.
    target: [f32; 10],
    /// Per-component glide length in sim frames (0 = committed immediately).
    total: [u32; 10],
    /// Per-component frames elapsed since the glide was armed.
    done: [u32; 10],
    /// Per-component ease curve nibble, latched from the staging beat's mode.
    curve: [u8; 10],
    initialized: bool,
}

impl CutsceneCameraInterp {
    /// Packed indices of the three angle components (shortest-arc glide).
    const PITCH: usize = 3;
    const YAW: usize = 4;
    /// Roll (op-`0x45` slot 2) rides at the end of the packed pose so the
    /// historical index numbering of the other nine is unchanged.
    pub const ROLL: usize = 9;
    /// Packed indices of the look-at trio (op-`0x45` focus slots 6 / 7 / 8).
    const FOCUS_X: usize = 0;
    const FOCUS_Y: usize = 1;
    const FOCUS_Z: usize = 2;
    /// Packed index of GTE `H` (op-`0x45` slot 9).
    const H: usize = 5;
    /// Packed indices of the eye-space translation trio (offset slots 3/4/5).
    const TR_X: usize = 6;
    const TR_Y: usize = 7;
    const TR_Z: usize = 8;

    pub fn new() -> Self {
        Self::default()
    }

    /// Drop the held pose so the next [`Self::glide`] snaps to its target.
    pub fn reset(&mut self) {
        self.initialized = false;
    }

    /// Decode one `apply == 0` Configure beat's op-`0x45` params into the
    /// packed-component snaps [`Self::snap_components`] takes.
    ///
    /// The slot decode mirrors `camera_view::cutscene_view` exactly - same
    /// negations (retail stores the focus X / Z negated), same 12-bit angle
    /// scale, same 6x world-scale reduction on the eye-space trio, same
    /// degenerate-value filters on the eye depth and `H` - so a snapped
    /// component's value equals the glide target that builder computes for
    /// it and the follow-up glide does not read it as a re-stage.
    ///
    /// Shared because both hosts own a [`CutsceneCameraInterp`] and a host
    /// that spells the decode out locally can get a negation right on one
    /// side and wrong on the other, which reads as a camera bug rather than
    /// as drift.
    ///
    /// REF: FUN_801DE084
    pub fn snap_components_for(params: &[(u8, u16)]) -> Vec<(usize, f32)> {
        use std::f32::consts::TAU;
        /// The 6x world scale retail folds into its camera rotation; the
        /// engine renders at 1x. Spelled here rather than imported so this
        /// leaf crate keeps no dependency on `engine-core`.
        const WORLD_SCALE: f32 = 6.0;
        let mut out: Vec<(usize, f32)> = Vec::with_capacity(params.len());
        for &(slot, raw) in params {
            let v = raw as i16 as f32;
            match slot {
                0 => out.push((Self::PITCH, v / 4096.0 * TAU)),
                1 => out.push((Self::YAW, v / 4096.0 * TAU)),
                2 => out.push((Self::ROLL, v / 4096.0 * TAU)),
                3 => out.push((Self::TR_X, v / WORLD_SCALE)),
                4 => out.push((Self::TR_Y, v / WORLD_SCALE)),
                5 if v.abs() > 1.0 => out.push((Self::TR_Z, v / WORLD_SCALE)),
                6 => out.push((Self::FOCUS_X, -v)),
                7 => out.push((Self::FOCUS_Y, v)),
                8 => out.push((Self::FOCUS_Z, -v)),
                9 if v > 1.0 => out.push((Self::H, v)),
                _ => {}
            }
        }
        out
    }

    /// Snap individual packed components (0..2 look_at, 3 pitch, 4 yaw, 5 H,
    /// 6..8 tr_eye, 9 roll) to `value` immediately - an `apply == 0` Configure
    /// beat.
    ///
    /// Needed for **same-tick beat pairs**: the field VM runs until yield, so
    /// a snap beat immediately followed by a glide beat (map01's fly-in: the
    /// aerial snap at `+0x109`, then the `apply 900` descent at `+0x11E` with
    /// no yield between) commits both in ONE world tick - the merged
    /// `camera_state` the caller reads only shows the glide beat's targets.
    /// Retail's mover snaps the live globals to the first beat and glides
    /// from there (the captured fly-in trajectory starts exactly at the
    /// aerial pose); replaying the drained beat events' snaps through this
    /// before arming the glide reproduces that. A snapped component's target
    /// equals its current value, so the follow-up [`Self::glide`] arms the
    /// glide FROM the snapped pose rather than seeing it as a re-stage.
    /// No-op before the first glide (which snaps the whole pose anyway).
    pub fn snap_components(&mut self, components: &[(usize, f32)]) {
        if !self.initialized {
            return;
        }
        for &(i, v) in components {
            if i >= 10 {
                continue;
            }
            self.cur[i] = v;
            self.start[i] = v;
            self.target[i] = v;
            self.total[i] = 0;
            self.done[i] = 0;
        }
    }

    /// Advance the camera `steps` sim frames toward the staged `target`
    /// pose and return the current view.
    ///
    /// `apply` is the staging beat's op-`0x45` `apply_trigger` (= the glide
    /// length in frames, 1:1) and `mode` its decoded mode nibble (`op0 >> 2`;
    /// the ease-curve selector - see the type doc). Any component whose
    /// target changed this call re-arms its glide from the current pose over
    /// `apply` frames (`0` = snap that component) with the beat's curve.
    /// Unchanged components keep their in-flight glide. The first call after
    /// a reset (or construction) snaps the whole pose to the target.
    pub fn glide_view(
        &mut self,
        target: FieldCameraView,
        apply: u32,
        mode: u8,
        steps: u32,
    ) -> FieldCameraView {
        let (focus, pitch, yaw, roll, h, tr_eye) = self.glide(
            target.focus,
            target.pitch,
            target.yaw,
            target.roll,
            target.h,
            target.tr_eye,
            apply,
            mode,
            steps,
        );
        FieldCameraView {
            focus,
            pitch,
            yaw,
            roll,
            h,
            tr_eye,
        }
    }

    /// The tuple-shaped rendition of [`Self::glide_view`], kept for the
    /// native window's decoded-component call sites.
    #[allow(clippy::too_many_arguments)]
    pub fn glide(
        &mut self,
        target_look_at: [f32; 3],
        target_pitch: f32,
        target_yaw: f32,
        target_roll: f32,
        target_h: f32,
        target_tr_eye: [f32; 3],
        apply: u32,
        mode: u8,
        steps: u32,
    ) -> ([f32; 3], f32, f32, f32, f32, [f32; 3]) {
        let packed = [
            target_look_at[0],
            target_look_at[1],
            target_look_at[2],
            target_pitch,
            target_yaw,
            target_h,
            target_tr_eye[0],
            target_tr_eye[1],
            target_tr_eye[2],
            target_roll,
        ];
        if !self.initialized {
            self.cur = packed;
            self.start = packed;
            self.target = packed;
            self.total = [0; 10];
            self.done = [0; 10];
            self.curve = [1; 10];
            self.initialized = true;
        } else {
            for (i, &target_i) in packed.iter().enumerate() {
                // Exact compare is intentional: targets decode from the same
                // op-0x45 integer params every frame, so a change is a real
                // re-stage, not float drift.
                if target_i != self.target[i] {
                    self.target[i] = target_i;
                    self.start[i] = self.cur[i];
                    // `apply` is the glide length in retail frames = sim
                    // ticks, 1:1 (capture-pinned; see the type doc).
                    self.total[i] = apply;
                    self.done[i] = 0;
                    // Retail applies ONE curve to all ten axes - the mover
                    // re-reads the same `actor[+0x50]` per axis. There is no
                    // angle/eye split ([`crate::camera_mover`]).
                    self.curve[i] = mode;
                }
                self.done[i] = self.done[i].saturating_add(steps).min(self.total[i]);
                let s = if self.total[i] == 0 {
                    1.0
                } else {
                    self.done[i] as f32 / self.total[i] as f32
                };
                let s = crate::camera_mover::curve_unit(s, self.curve[i]);
                let angle = i == Self::PITCH || i == Self::YAW || i == Self::ROLL;
                let delta = if angle {
                    wrap_pi(self.target[i] - self.start[i])
                } else {
                    self.target[i] - self.start[i]
                };
                self.cur[i] = self.start[i] + delta * s;
                if angle {
                    self.cur[i] = wrap_pi(self.cur[i]);
                }
            }
        }
        (
            [self.cur[0], self.cur[1], self.cur[2]],
            self.cur[3],
            self.cur[4],
            self.cur[Self::ROLL],
            self.cur[5],
            [self.cur[6], self.cur[7], self.cur[8]],
        )
    }
}

/// Wrap an angle (radians) into `(-pi, pi]`.
fn wrap_pi(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let mut a = a % TAU;
    if a > PI {
        a -= TAU;
    } else if a <= -PI {
        a += TAU;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The projection is the retail GTE transform, not an approximation of
    /// one: project a handful of points through the matrix and compare with
    /// `screen = H*(R*(v - focus) + tr)/Ez + (OFX, OFY)` computed by hand.
    #[test]
    fn psx_camera_vp_matches_the_handrolled_retail_projection() {
        use crate::battle_cam_script::{GTE_OFX, GTE_OFY};
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
