//! The ten live retail camera globals as one value.
//!
//! `engine-core`'s field camera holds one of these, and so does the Baka
//! Fighter duel camera in the minigame crate; keeping the type here, next to
//! the camera mover that steps the same ten axes, lets both use it without
//! the minigame crate depending on `engine-core`. `engine-core` re-exports it
//! as `camera::RetailCamGlobals`.

use crate::camera_mover::AXIS_COUNT;

/// The ten live retail camera globals, in the order the op-`0x45` param mask
/// and the camera mover both use them.
///
/// This is the state the retail engine actually renders from, and the state a
/// state trace samples - not a world-space `(eye, look_at)` pair. Keeping it
/// verbatim is what makes the engine comparable to a recomp capture channel
/// for channel:
///
/// | axis | global | role |
/// |---|---|---|
/// | 0 / 1 / 2 | `_DAT_8007B790/92/94` | pitch / yaw / roll (12-bit, `4096` = full turn) |
/// | 3 / 4 / 5 | `_DAT_800840B8/BC/C0` | eye-space translation trio `tr_eye`; axis 5 is the eye-back depth |
/// | 6 / 7 / 8 | `_DAT_80089118/1C/20` | camera focus, stored **negated** in X and Z |
/// | 9 | `_DAT_8007B6F4` | GTE `H` projection register |
///
/// The focus storage convention is the one that catches people out: the
/// globals hold `(-X, +Y, -Z)` of the world focus point (`FUN_801DAB90`), so
/// a retail capture of a shot focused on world `(8640, 0, 10304)` reads
/// `(-8640, 0, -10304)`. See
/// [`cutscene.md`](../../../docs/subsystems/cutscene.md).
///
/// REF: FUN_801DE084
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailCamGlobals(pub [i32; AXIS_COUNT]);

impl RetailCamGlobals {
    /// The field-entry reset values written by `FUN_80025C24` (caller
    /// `FUN_801D6704`, field init): angles `(0x1B8, 0x64, 0)` and
    /// `tr_eye = (0, -256, 16420)`. Focus and `H` are left as the scene
    /// establishes them - the routine is six stores and none of them is the
    /// focus trio or `_DAT_8007B6F4`, so `engine-core`'s `Camera::reset_globals_for_scene_entry`
    /// rewrites only those six axes and this constant's `H` is the field value
    /// the register otherwise holds (`512`), not `0`. A glide beat that names
    /// slot `9` starts from that value; seeding `0` made the first frames of
    /// town01's entry glide project through an `H` no retail frame ever had.
    ///
    /// PORT: FUN_80025C24
    pub const FIELD_RESET: Self = Self([0x1B8, 0x64, 0, 0, -256, 16420, 0, 0, 0, 512]);

    /// The axes `FUN_80025C24` writes: pitch, yaw, roll and the eye trio.
    pub const FIELD_RESET_AXES: [usize; 6] = [0, 1, 2, 3, 4, 5];

    /// Pitch / yaw / roll, 12-bit units.
    pub fn angles(&self) -> [i32; 3] {
        [self.0[0], self.0[1], self.0[2]]
    }

    /// The eye-space translation trio (`_DAT_800840B8`).
    pub fn tr_eye(&self) -> [i32; 3] {
        [self.0[3], self.0[4], self.0[5]]
    }

    /// The focus trio exactly as retail stores it - X and Z **negated**.
    pub fn focus_stored(&self) -> [i32; 3] {
        [self.0[6], self.0[7], self.0[8]]
    }

    /// The focus as a world-space point: `(-axis6, axis7, -axis8)`.
    pub fn focus_world(&self) -> [i32; 3] {
        [-self.0[6], self.0[7], -self.0[8]]
    }

    /// GTE `H`.
    pub fn h(&self) -> i32 {
        self.0[9]
    }

    /// The same ten axes in the shape the camera-relative effect-actor
    /// normalizer wants (`crate::camera_rel_actor`). The
    /// normalizer compares each of a spawn record's ten reference
    /// halfwords against exactly these globals, so the conversion is a
    /// re-labelling, not a transform - note in particular that the focus
    /// goes across **stored** (X and Z negated), because that is the form
    /// `FUN_80021248` compares against.
    pub fn camera_snapshot(&self) -> crate::camera_rel_actor::CameraSnapshot {
        crate::camera_rel_actor::CameraSnapshot {
            angles: [self.0[0] as u16, self.0[1] as u16, self.0[2] as u16],
            offsets: self.tr_eye(),
            focus: self.focus_stored(),
            gte_h: self.0[9] as i16,
        }
    }
}

impl Default for RetailCamGlobals {
    fn default() -> Self {
        Self::FIELD_RESET
    }
}
