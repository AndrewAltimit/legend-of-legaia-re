//! Camera-relative part placement: what a render node whose `+0x52` word
//! carries a bit of `0x780` looks like under a host camera that always
//! applies the **full** view rotation.
//!
//! Retail's render dispatcher `FUN_8001ADA4` tests `node+0x52 & 0x780` at
//! `0x8001B374`. Clear, it folds the node matrix into the camera matrix
//! (`FUN_8005B3A8(0x1F8003C8, 0x1F8002D4)`, which leaves `a0 * a1` in `a1`),
//! so the node is drawn as `S_b * R * N` with `S_b` the base matrix
//! `0x8007BF10` and `R = Rx(pitch) * Ry(yaw) * Rz(roll)`. Set, it calls
//! [`camera_view_rotation`]'s original `FUN_8001CF50` (`jal` at `0x8001B3A0`)
//! instead, which leaves in the same slot:
//!
//! - bit `0x400` clear: `P * (6 * N)` - `P` the camera rotation with the
//!   flagged axes left out, `6` the literal `0x6000` stored at `0x8001D000`
//!   and applied by `FUN_8005B4E8`. The node's view-space position `+0x2C` is
//!   untouched, so it stays where the full camera put it.
//! - bit `0x400` set: `S_b * N`, after `FUN_8003D1A4` loads the base matrix
//!   block (rotation and its zero translation) and `FUN_8003D344` writes
//!   `+0x2C = S_b * (+0x14)`: the node sits at a fixed eye-space offset, locked
//!   to the camera.
//!
//! Either way the dispatcher then loads the result as the GTE rotation and
//! `+0x2C` as `TR` (`0x8001B3A8..0x8001B414`).
//!
//! The hosts cannot take that branch themselves: every part matrix goes
//! through one view-projection that holds the full `R`. So the kernel
//! expresses the retail result as a **model prefix** the host puts in place of
//! the part's translation - a placement and a basis `K` with `R * S_b * K * N`
//! equal to what retail draws. For the skip arm `K = (6 / S_b) * R^T * P`; for
//! the saved-matrix arm `K = R^T` and the placement is the world point the
//! full camera maps onto the locked eye offset. With no `0x780` bit there is
//! no prefix, and the host draws exactly what it drew before.

use super::{GteMat3, camera_view_rotation, view_rot_flags};
use legaia_engine_vm::battle_cam_script::BattleCamPose;
use legaia_engine_vm::psx_camera::{FieldCameraView, camera_rotation};

/// The `+0x52` bits that route a node through `FUN_8001CF50` (`andi
/// v0,v0,0x780` at `0x8001B374` in the render dispatcher).
pub const CAMERA_RELATIVE_MASK: u16 = 0x0780;

/// The uniform scale `FUN_8001CF50` applies to the node matrix on its skip
/// arm: the literal `0x6000` (`li v0,0x6000` at `0x8001CFFC`, stored into all
/// three lanes of the scale vector), `6.0` in the GTE's `4096 = 1.0`. It is a
/// constant, not the base matrix - in battle, where the base is `4x`, a
/// camera-relative part draws at one and a half times a plain part's size.
pub const CAMERA_RELATIVE_SCALE: f32 = 6.0;

/// Base-matrix scale of the field / cutscene / overworld view
/// (`_DAT_8007BF10 = 24576 * I`).
pub const FIELD_BASE_SCALE: f32 = 6.0;

/// Base-matrix scale of the battle view (`_DAT_8007BF10 = 16384 * I`).
pub const BATTLE_BASE_SCALE: f32 = 4.0;

/// The retail camera a part is drawn against, in the raw retail Y-down world
/// frame, with the eye translation divided by the base scale so that
/// `R * (p - focus) + tr_unit` is the eye-space point over `base_scale`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartCameraPose {
    /// `_DAT_8007B790`, radians.
    pub pitch: f32,
    /// `_DAT_8007B792`, radians.
    pub yaw: f32,
    /// `_DAT_8007B794`, radians.
    pub roll: f32,
    /// The world point the camera orbits.
    pub focus: [f32; 3],
    /// The eye translation trio over [`Self::base_scale`].
    pub tr_unit: [f32; 3],
    /// The base matrix's uniform scale ([`FIELD_BASE_SCALE`] /
    /// [`BATTLE_BASE_SCALE`]).
    pub base_scale: f32,
}

impl PartCameraPose {
    /// A field-frame pose (the follow camera, a cutscene shot, or the
    /// overworld walk through `FieldCameraFrame::field_view`), whose
    /// `tr_eye` is already reduced by the 6x base scale.
    pub fn from_field_view(v: &FieldCameraView) -> Self {
        Self {
            pitch: v.pitch,
            yaw: v.yaw,
            roll: v.roll,
            focus: v.focus,
            tr_unit: v.tr_eye,
            base_scale: FIELD_BASE_SCALE,
        }
    }

    /// The stage-dome battle camera (`battle_cam_script::battle_vp`'s pose):
    /// 12-bit angles, no roll, the eye translation in retail units under the
    /// 4x base.
    pub fn from_battle(pose: &BattleCamPose) -> Self {
        let to_rad = |u: f32| u / 4096.0 * std::f32::consts::TAU;
        Self {
            pitch: to_rad(pose.pitch),
            yaw: to_rad(pose.yaw),
            roll: 0.0,
            focus: pose.focus,
            tr_unit: pose.tr.map(|c| c / BATTLE_BASE_SCALE),
            base_scale: BATTLE_BASE_SCALE,
        }
    }
}

/// A camera-relative node resolved against a host camera: the world point to
/// translate to and the basis to put in front of the node's own rotation,
/// both in the raw retail Y-down frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraRelativePart {
    /// World-space placement.
    pub pos: [f32; 3],
    /// Row-major basis `K`, left-multiplied onto the node's rotation.
    pub basis: [[f32; 3]; 3],
}

fn gte_to_f32(m: &GteMat3) -> [[f32; 3]; 3] {
    m.m.map(|row| row.map(|e| f32::from(e) / 4096.0))
}

fn mul3(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0f32; 3]; 3];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, slot) in row.iter_mut().enumerate() {
            *slot = (0..3).map(|k| a[r][k] * b[k][c]).sum();
        }
    }
    out
}

/// `R^T`, the inverse of the full camera rotation the host's view applies -
/// taken from the same f32 factor the hosts' view-projections are built with
/// (`psx_camera::camera_rotation`), so the undo cancels it to rounding.
fn full_rotation_transpose(cam: &PartCameraPose) -> [[f32; 3]; 3] {
    let r = camera_rotation(cam.pitch, cam.yaw, cam.roll);
    // Column-major `r[c*4 + row]`; the transpose's `[row][c]` is `R[c][row]`.
    let mut out = [[0.0f32; 3]; 3];
    for (row, o) in out.iter_mut().enumerate() {
        for (c, slot) in o.iter_mut().enumerate() {
            *slot = r[row * 4 + c];
        }
    }
    out
}

/// Resolve a node's `+0x52` word against `cam`.
///
/// `pos` is the node's position in the raw retail frame (its `+0x14` trio).
/// `None` when the word carries no `0x780` bit: the node takes the full
/// camera, and the host's existing composition is already retail's.
pub fn camera_relative_part(
    flags: u16,
    pos: [f32; 3],
    cam: &PartCameraPose,
) -> Option<CameraRelativePart> {
    if flags & CAMERA_RELATIVE_MASK == 0 {
        return None;
    }
    let rt = full_rotation_transpose(cam);
    match camera_view_rotation(flags, cam.pitch, cam.yaw, cam.roll) {
        // Saved-matrix arm: eye = S_b * (pos + N v). Under the host's
        // `S_b * (R (p - focus) + tr_unit)` that is the world point
        // `focus + R^T (pos - tr_unit)` and the basis `R^T`.
        None => {
            let d = [
                pos[0] - cam.tr_unit[0],
                pos[1] - cam.tr_unit[1],
                pos[2] - cam.tr_unit[2],
            ];
            let mut p = cam.focus;
            for (i, pi) in p.iter_mut().enumerate() {
                *pi += (0..3).map(|j| rt[i][j] * d[j]).sum::<f32>();
            }
            Some(CameraRelativePart { pos: p, basis: rt })
        }
        // Skip arm: eye orientation `P * 6 N` against the host's `S_b R K N`.
        Some(partial) => {
            let k = CAMERA_RELATIVE_SCALE / cam.base_scale;
            let basis = mul3(&rt, &gte_to_f32(&partial)).map(|row| row.map(|e| e * k));
            Some(CameraRelativePart { pos, basis })
        }
    }
}

/// The host-side model prefix for a part: `T(pos) * K`, column-major, to put
/// in place of the `T(world_pos)` a host composes a part with.
///
/// `world_pos` is the draw record's position, which is also the node's raw
/// `+0x14` trio - the eye-space offset the `0x400` arm locks the node at.
/// `frame_flip` says the host's model frame is the retail frame with Y
/// negated (the battle passes, whose models and camera each carry one
/// `scale(1,-1,1)`); the field passes compose on the raw retail frame and
/// pass `false`. `None` - no `0x780` bit, or no retail camera pose - means
/// "compose as before": the prefix is only ever a replacement, never an extra
/// factor on a flag-free part, so the retail default draws bit-identically.
///
/// REF: FUN_8001CF50 (the host placement of its result; the rotation itself
/// is the port [`camera_view_rotation`])
pub fn camera_relative_model_prefix(
    flags: u16,
    world_pos: [f32; 3],
    cam: Option<&PartCameraPose>,
    frame_flip: bool,
) -> Option<[f32; 16]> {
    let cam = cam?;
    let part = camera_relative_part(flags, world_pos, cam)?;
    let sy = if frame_flip { -1.0 } else { 1.0 };
    // Back into the host frame, `F K F` for the basis. The skip arm keeps the
    // full camera's translation (retail leaves `+0x2C` as the full camera put
    // it) - the node's own world point, which in the host frame is `F p` like
    // any other translation ([`part_model_place`]); the locked arm's
    // placement is a retail-frame point, `F p` here too.
    let p = if flags & view_rot_flags::USE_SAVED_MATRIX != 0 {
        [part.pos[0], part.pos[1] * sy, part.pos[2]]
    } else {
        [world_pos[0], world_pos[1] * sy, world_pos[2]]
    };
    let fl = [1.0, sy, 1.0];
    let b = &part.basis;
    let e = |r: usize, c: usize| fl[r] * b[r][c] * fl[c];
    Some([
        e(0, 0),
        e(1, 0),
        e(2, 0),
        0.0, //
        e(0, 1),
        e(1, 1),
        e(2, 1),
        0.0, //
        e(0, 2),
        e(1, 2),
        e(2, 2),
        0.0, //
        p[0],
        p[1],
        p[2],
        1.0,
    ])
}

/// The model prefix both play hosts place a move-VM part with: the
/// camera-relative prefix ([`camera_relative_model_prefix`]) for a
/// `+0x52 & 0x780` node, else the plain translation `T(F p)`.
///
/// `frame_flip` is the battle frame, whose view-projection carries the
/// trailing `scale(1,-1,1)` (`psx_camera_vp`) that cancels the per-model
/// Y-flip: a model `T(q) * R * F` reaches the retail camera as the point
/// `F q`, so a node at retail `+0x14 = p` is placed at `q = F p`. Placing it
/// at `p` drew every off-floor battle effect mirrored through the floor -
/// `vera_summon_mid_cast`'s glows, seated at `y = -0x280` over the target's
/// raised hand (PROT 0905 arm 0), rendered under the stage.
pub fn part_model_place(
    flags: u16,
    world_pos: [f32; 3],
    cam: Option<&PartCameraPose>,
    frame_flip: bool,
) -> [f32; 16] {
    camera_relative_model_prefix(flags, world_pos, cam, frame_flip).unwrap_or_else(|| {
        let y = if frame_flip {
            -world_pos[1]
        } else {
            world_pos[1]
        };
        [
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            world_pos[0],
            y,
            world_pos[2],
            1.0,
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 2e-3;

    /// A battle part's placement is the retail point under the frame flip,
    /// whether it is a plain node or a skip-bit billboard; a field part keeps
    /// the raw retail point.
    #[test]
    fn a_battle_part_is_placed_at_the_flipped_retail_point() {
        let cam = battle_cam([0.0, 0.0, 0.0]);
        let p = [-365.0, -430.0, -810.0];
        for flags in [0u16, 0x0380] {
            let m = part_model_place(flags, p, Some(&cam), true);
            assert_eq!([m[12], m[13], m[14]], [-365.0, 430.0, -810.0], "{flags:#x}");
        }
        let m = part_model_place(0, p, None, false);
        assert_eq!([m[12], m[13], m[14]], p);
    }

    fn field_cam() -> PartCameraPose {
        PartCameraPose {
            pitch: 0.61,
            yaw: -0.93,
            roll: 0.17,
            focus: [1200.0, -40.0, -400.0],
            tr_unit: [10.0, 85.0, 2730.0],
            base_scale: FIELD_BASE_SCALE,
        }
    }

    fn battle_cam(focus: [f32; 3]) -> PartCameraPose {
        PartCameraPose::from_battle(&BattleCamPose {
            pitch: 32.0,
            yaw: 224.0,
            tr: [0.0, 1280.0, 7680.0],
            focus,
        })
    }

    /// Row-major full camera rotation, the host's `R`.
    fn full(cam: &PartCameraPose) -> [[f32; 3]; 3] {
        let rt = full_rotation_transpose(cam);
        [0, 1, 2].map(|r| [0, 1, 2].map(|c| rt[c][r]))
    }

    fn rx(a: f32) -> [[f32; 3]; 3] {
        let (s, c) = a.sin_cos();
        [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]]
    }

    fn rz(a: f32) -> [[f32; 3]; 3] {
        let (s, c) = a.sin_cos();
        [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
    }

    const ID: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    fn assert_close(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) {
        for r in 0..3 {
            for c in 0..3 {
                assert!(
                    (a[r][c] - b[r][c]).abs() < EPS,
                    "[{r}][{c}]: {a:?} vs {b:?}"
                );
            }
        }
    }

    #[test]
    fn a_flag_free_part_gets_no_prefix() {
        let cam = field_cam();
        // Bits outside 0x780 (0x20 is the dispatcher's own draw gate) leave
        // the full-camera composition alone.
        for flags in [0u16, 0x0020, 0x0040, 0x0800, 0x0001] {
            assert!(camera_relative_part(flags, [1.0, 2.0, 3.0], &cam).is_none());
            assert!(
                camera_relative_model_prefix(flags, [1.0, 2.0, 3.0], Some(&cam), false).is_none()
            );
        }
        // No retail pose (a host's own debug vantage): no prefix either.
        assert!(
            camera_relative_model_prefix(view_rot_flags::SKIP_YAW, [0.0; 3], None, false).is_none()
        );
    }

    #[test]
    fn skip_yaw_removes_exactly_the_yaw_factor() {
        let cam = field_cam();
        let pos = [300.0, -20.0, 50.0];
        let part = camera_relative_part(view_rot_flags::SKIP_YAW, pos, &cam).unwrap();
        // The host draws `R * K * N`; retail draws `Rx * Rz * N` (6x under a
        // 6x base, so the scale cancels on the field).
        let drawn = mul3(&full(&cam), &part.basis);
        assert_close(&drawn, &mul3(&rx(cam.pitch), &rz(cam.roll)));
        // The skip arm never moves the part.
        assert_eq!(part.pos, pos);
        // And the host prefix carries the same basis, column-major.
        let m =
            camera_relative_model_prefix(view_rot_flags::SKIP_YAW, pos, Some(&cam), false).unwrap();
        for r in 0..3 {
            for c in 0..3 {
                assert_eq!(m[c * 4 + r], part.basis[r][c]);
            }
        }
        assert_eq!([m[12], m[13], m[14], m[15]], [pos[0], pos[1], pos[2], 1.0]);
    }

    #[test]
    fn all_three_skips_make_a_screen_aligned_billboard() {
        let cam = field_cam();
        let flags =
            view_rot_flags::SKIP_PITCH | view_rot_flags::SKIP_YAW | view_rot_flags::SKIP_ROLL;
        let part = camera_relative_part(flags, [0.0; 3], &cam).unwrap();
        assert_close(&mul3(&full(&cam), &part.basis), &ID);
    }

    #[test]
    fn the_skip_arm_carries_the_literal_six_fold_scale_in_battle() {
        // In battle the base is 4x but `FUN_8001CF50` scales by `0x6000`, so a
        // camera-relative part is 1.5x a plain part.
        let cam = battle_cam([100.0, 0.0, -50.0]);
        let flags =
            view_rot_flags::SKIP_PITCH | view_rot_flags::SKIP_YAW | view_rot_flags::SKIP_ROLL;
        let part = camera_relative_part(flags, [0.0; 3], &cam).unwrap();
        let drawn = mul3(&full(&cam), &part.basis);
        let k = CAMERA_RELATIVE_SCALE / BATTLE_BASE_SCALE;
        assert_close(&drawn, &[[k, 0.0, 0.0], [0.0, k, 0.0], [0.0, 0.0, k]]);
    }

    #[test]
    fn the_saved_matrix_arm_locks_the_part_to_its_eye_offset() {
        let cam = field_cam();
        let offset = [40.0, -12.0, 900.0];
        let part = camera_relative_part(view_rot_flags::USE_SAVED_MATRIX, offset, &cam).unwrap();
        // The host's eye point `R (p - focus) + tr_unit` is the retail
        // `+0x2C = S_b * (+0x14)` over `S_b`: the offset itself.
        let r = full(&cam);
        let d = [0, 1, 2].map(|i| part.pos[i] - cam.focus[i]);
        let eye = [0, 1, 2].map(|i| (0..3).map(|j| r[i][j] * d[j]).sum::<f32>() + cam.tr_unit[i]);
        for i in 0..3 {
            assert!((eye[i] - offset[i]).abs() < 0.05, "{eye:?} vs {offset:?}");
        }
        // And its orientation is the node's own, with no camera rotation.
        assert_close(&mul3(&r, &part.basis), &ID);
        // The saved-matrix bit wins over any skip bit, as the `0x400` test at
        // `0x8001CF7C` runs first.
        let both = view_rot_flags::USE_SAVED_MATRIX | view_rot_flags::SKIP_YAW;
        assert_eq!(camera_relative_part(both, offset, &cam), Some(part));
    }

    #[test]
    fn the_battle_frame_prefix_conjugates_by_the_y_flip() {
        let cam = battle_cam([0.0; 3]);
        let pos = [10.0, 20.0, 30.0];
        let flags = view_rot_flags::SKIP_YAW;
        let raw = camera_relative_part(flags, pos, &cam).unwrap();
        let m = camera_relative_model_prefix(flags, pos, Some(&cam), true).unwrap();
        // Skip arm: the full camera's translation, which in the flipped host
        // frame is `F p` - the frame's VP undoes the flip on the way to the
        // retail camera (`vera_summon_mid_cast`'s glows drew under the floor
        // while this read `p`).
        assert_eq!([m[12], m[13], m[14]], [pos[0], -pos[1], pos[2]]);
        // Basis `F K F`: the Y row and column change sign, the rest do not.
        let sign = [1.0, -1.0, 1.0];
        for r in 0..3 {
            for c in 0..3 {
                assert_eq!(m[c * 4 + r], sign[r] * raw.basis[r][c] * sign[c]);
            }
        }
        // Locked arm: the retail-frame placement comes back Y-negated.
        let locked = view_rot_flags::USE_SAVED_MATRIX;
        let raw = camera_relative_part(locked, pos, &cam).unwrap();
        let m = camera_relative_model_prefix(locked, pos, Some(&cam), true).unwrap();
        assert_eq!([m[12], m[13], m[14]], [raw.pos[0], -raw.pos[1], raw.pos[2]]);
    }
}
