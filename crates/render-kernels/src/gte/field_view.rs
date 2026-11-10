//! The field camera as the GTE holds it, in integers: the matrix
//! `FUN_800172C0` builds each frame and one `RTPS` through it.
//!
//! The hosts draw through an `f32` view-projection
//! ([`legaia_engine_camera::psx_camera`]); a kernel that stands for one of
//! retail's own `RTPS` calls - a single point the CPU then uses as data, such
//! as the framebuffer grab's source rectangle - projects here instead, so its
//! result is the console's to the pixel.
//!
//! `FUN_800172C0` (`0x800172C0..0x800173B8`):
//!
//! 1. `RotMatrix` (`FUN_80026988`, [`crate::battle_intro::euler_rot_psx`]) of the camera
//!    angles `0x8007B790..94` into the scratch matrix `0x1F8003C8`;
//! 2. `MulMatrix(0x8007BF10, 0x1F8003C8)` ([`GteMat3::mul`]): the base matrix
//!    (`S * I`, `S = 0x6000` on the field) times the rotation;
//! 3. the translation: the eye words `0x800840B8..C0` loaded as `TR`, then
//!    `MVMVA` (`sf = 1`) of the focus words `0x80089118..20` **as halfwords**
//!    (`lhu`, so each wraps to 16 bits) - `TR = eye + (R * focus) >> 12`.
//!
//! The focus globals hold the negated focus point, so a world point `p` lands
//! at `R * (p - focus) + eye` in eye space, the form the `f32` view writes as
//! `S * (R1 * (p - focus) + eye / S)`.
//!
//! REF: FUN_800172C0, FUN_80026988, FUN_8005B3A8

use super::GteMat3;
use super::divide::gte_divide;

/// The field's base matrix scale (`_DAT_8007BF10 = 0x6000 * I`).
pub const FIELD_BASE_Q12: i16 = 0x6000;

/// The field's GTE screen centre `OFX`, in pixels.
pub const FIELD_OFX: i32 = 160;

/// One `RTPS` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rtps {
    /// `SXY2`, saturated to `-0x400..=0x3FF`.
    pub sxy: (i16, i16),
    /// `SZ3`, saturated to `0..=0xFFFF`.
    pub sz: u16,
    /// `IR1..IR3` (`lm = 0`: saturated to `i16`).
    pub ir: [i32; 3],
    /// `MAC1..MAC3`, the eye-space point.
    pub mac: [i32; 3],
}

/// The GTE's rotation, translation and projection constants for one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldGteView {
    /// `RT`.
    pub rot: GteMat3,
    /// `TR`.
    pub tr: [i32; 3],
    /// `H`.
    pub h: u16,
    /// `OFX` / `OFY`, whole pixels.
    pub ofx: i32,
    pub ofy: i32,
}

/// `MVMVA` with `sf = 1` over a translation: `(tr << 12 + R * v) >> 12`, the
/// 32-bit `MAC` registers.
fn mvmva(rot: &GteMat3, tr: [i32; 3], v: [i16; 3]) -> [i32; 3] {
    std::array::from_fn(|r| {
        let sum: i64 = (0..3)
            .map(|c| i64::from(rot.m[r][c]) * i64::from(v[c]))
            .sum();
        (((i64::from(tr[r]) << 12) + sum) >> 12) as i32
    })
}

impl FieldGteView {
    /// The matrix `FUN_800172C0` leaves loaded: `angles` the three camera
    /// angle halfwords, `eye` the eye words, `neg_focus` the focus words (the
    /// negated focus point), `base` the base matrix.
    pub fn from_globals(
        angles: [u16; 3],
        eye: [i32; 3],
        neg_focus: [i32; 3],
        base: &GteMat3,
        h: u16,
        ofx: i32,
        ofy: i32,
    ) -> Self {
        let euler = crate::battle_intro::euler_rot_psx((
            angles[0] as i16,
            angles[1] as i16,
            angles[2] as i16,
        ));
        let rot = base.mul(&euler);
        let tr = mvmva(&rot, eye, neg_focus.map(|v| v as i16));
        Self {
            rot,
            tr,
            h,
            ofx,
            ofy,
        }
    }

    /// The field base matrix `0x6000 * I`.
    pub fn field_base() -> GteMat3 {
        let s = FIELD_BASE_Q12;
        GteMat3 {
            m: [[s, 0, 0], [0, s, 0], [0, 0, s]],
        }
    }

    /// The same frame rebuilt from a host's resolved view: the angles come
    /// back to their 12-bit globals exactly ([`super::psx_angle`]), the eye
    /// trio from the view's `1x` translation times the field's `6x`, the
    /// focus and `H` as whole units. Exact for a view resolved from the
    /// retail globals (`engine-field`'s `scripted_view` and the follow camera
    /// write them this way); a user knob that moves the view moves this with
    /// it.
    pub fn from_field_view(
        view: &legaia_engine_camera::psx_camera::FieldCameraView,
        ofy: i32,
    ) -> Self {
        let angles = [view.pitch, view.yaw, view.roll].map(super::psx_angle);
        let scale = f32::from(FIELD_BASE_Q12) / 4096.0;
        let eye = view.tr_eye.map(|c| (c * scale).round() as i32);
        let neg_focus = view.focus.map(|c| -(c.round() as i32));
        Self::from_globals(
            angles,
            eye,
            neg_focus,
            &Self::field_base(),
            view.h.round().clamp(0.0, 65535.0) as u16,
            FIELD_OFX,
            ofy,
        )
    }

    /// `RTPS` of `p` (`sf = 1`, `lm = 0`).
    pub fn rtps(&self, p: [i16; 3]) -> Rtps {
        let mac = mvmva(&self.rot, self.tr, p);
        let ir = mac.map(|v| v.clamp(-0x8000, 0x7FFF));
        let sz = mac[2].clamp(0, 0xFFFF) as u16;
        let (div, _) = gte_divide(self.h, sz);
        let screen = |of: i32, ir: i32| {
            let v = ((i64::from(of) << 16) + i64::from(ir) * div) >> 16;
            v.clamp(-0x400, 0x3FF) as i16
        };
        Rtps {
            sxy: (screen(self.ofx, ir[0]), screen(self.ofy, ir[1])),
            sz,
            ir,
            mac,
        }
    }
}
