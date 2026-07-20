/// Fixed-point bit count for rotation-matrix coefficients (q3.12).
pub const ROT_FRAC_BITS: i32 = 12;

/// Fixed-point scale factor for rotation coefficients (`1 << 12 = 4096`).
/// A retail GTE rotation matrix is encoded with element values in
/// `[-32768, 32767]`; an identity rotation has diagonal = `4096`.
pub const ROT_ONE: i32 = 1 << ROT_FRAC_BITS;

/// Default GTE focal length in pixels (the value the retail TMD renderer
/// loads into `H` for the standard 320×240 PSX frame: `H = 320`).
pub const DEFAULT_H: i32 = 320;

/// Saturated 16-bit signed clamp for the perspective-divide *numerator* -
/// the hardware IR1/IR2 the projection multiplies by the reciprocal. IR
/// registers are 16-bit signed, so the numerator is clamped to `[-32768,
/// 32767]`. NB this is the IR clamp, NOT the final screen-coordinate clamp -
/// the GTE saturates the *stored* SXY FIFO entry much more tightly (see
/// [`SX_MIN`]/[`SX_MAX`]).
pub const SXY_MIN: i32 = i16::MIN as i32;
pub const SXY_MAX: i32 = i16::MAX as i32;

/// Final SXY-FIFO screen-coordinate saturation bound. The GTE clamps the
/// projected screen X/Y to signed 11 bits, `[-0x400, 0x3FF]`, and raises
/// `SX2_SATURATED` / `SY2_SATURATED` when it does - matching the PSX GPU's
/// 11-bit-signed drawing coordinate range. Cross-validated bit-exact against
/// a real-COP2 RTPT ring capture (see the `rtpt_matches_recomp_cop2_capture`
/// oracle in `tests.rs`); the earlier i16 bound here was a latent divergence
/// that a self-consistent in-repo sweep could not surface.
pub const SX_MIN: i32 = -0x400;
pub const SX_MAX: i32 = 0x3FF;

/// 3-vector of i32. Matches the GTE's MAC0/MAC1/MAC2/MAC3 accumulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GteVec3 {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl GteVec3 {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// Convert from f32 world coordinates assuming q19.12 fixed-point.
    /// Saturates at i32 bounds to avoid panics on rare overflow.
    pub fn from_f32_q12(x: f32, y: f32, z: f32) -> Self {
        Self {
            x: clamp_i32_from_f32(x * ROT_ONE as f32),
            y: clamp_i32_from_f32(y * ROT_ONE as f32),
            z: clamp_i32_from_f32(z * ROT_ONE as f32),
        }
    }

    /// Convert back to f32 world coordinates.
    pub fn to_f32_q12(&self) -> (f32, f32, f32) {
        let inv = 1.0 / ROT_ONE as f32;
        (
            self.x as f32 * inv,
            self.y as f32 * inv,
            self.z as f32 * inv,
        )
    }
}

/// Row-major 3×3 rotation matrix in q3.12 fixed-point.
///
/// Identity:
/// ```text
/// [4096, 0, 0]
/// [0, 4096, 0]
/// [0, 0, 4096]
/// ```
/// Each element is in `[-32768, 32767]`. The retail GTE stores these as
/// `i16`; we widen to `i32` to keep the multiply-add path overflow-free
/// without saturation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GteMat3 {
    pub m: [[i16; 3]; 3],
}

impl GteMat3 {
    pub const IDENTITY: Self = Self {
        m: [
            [ROT_ONE as i16, 0, 0],
            [0, ROT_ONE as i16, 0],
            [0, 0, ROT_ONE as i16],
        ],
    };

    /// Build a rotation matrix about the +Y axis by `angle` radians, with
    /// elements quantized to q3.12 (matches the GTE precision).
    ///
    // PORT: FUN_8004629C - retail RotMatrixY: indexes the cos/sin LUT pair
    // (&DAT_80070A2C / &DAT_8007122C) by a 12-bit angle (4096 = 2*PI) and
    // composes via the GTE; this builds the same +Y rotation in q3.12 with a
    // radian input (ROT_ONE = 0x1000 matches the LUT's 1.0).
    // NOT WIRED: the render path composes camera and object transforms as glam
    // f32 matrices, so no frame-path code wants a q3.12 rotation. These
    // builders serve the GTE register oracle, which needs matrices in the
    // hardware's own fixed-point to compare against captured register state,
    // and their callers are that oracle's tests. Note the radian argument is
    // itself a deviation: retail takes a 12-bit angle and reads the LUT, which
    // `billboard::rot_z_psx` models for Z. Wiring these means either the
    // renderer moving to fixed-point transforms or an angle-driven camera
    // path that wants the retail quantisation - neither exists.
    pub fn rot_y(angle: f32) -> Self {
        let c = (angle.cos() * ROT_ONE as f32).round() as i16;
        let s = (angle.sin() * ROT_ONE as f32).round() as i16;
        Self {
            m: [[c, 0, s], [0, ROT_ONE as i16, 0], [-s, 0, c]],
        }
    }

    /// Build a rotation about the +X axis (pitch).
    ///
    // PORT: FUN_800461A4 - retail RotMatrixX (same cos/sin LUT + 12-bit angle
    // as FUN_8004629C, about the +X axis).
    // NOT WIRED: GTE-oracle-only, same reason as `rot_y` above.
    pub fn rot_x(angle: f32) -> Self {
        let c = (angle.cos() * ROT_ONE as f32).round() as i16;
        let s = (angle.sin() * ROT_ONE as f32).round() as i16;
        Self {
            m: [[ROT_ONE as i16, 0, 0], [0, c, -s], [0, s, c]],
        }
    }

    /// Build a rotation about the +Z axis (roll).
    ///
    // PORT: FUN_8004638C - retail RotMatrixZ (same cos/sin LUT + 12-bit angle
    // as FUN_8004629C, about the +Z axis).
    // NOT WIRED: GTE-oracle-only, same reason as `rot_y` above. This address
    // has a second, more faithful port in `billboard::rot_z_psx`, which takes
    // the retail 12-bit angle and reads the LUT rather than f32 trig; the two
    // are pinned to agree at the cardinals by a unit test there. That one is
    // inert as well, for the separate reason recorded on that module.
    pub fn rot_z(angle: f32) -> Self {
        let c = (angle.cos() * ROT_ONE as f32).round() as i16;
        let s = (angle.sin() * ROT_ONE as f32).round() as i16;
        Self {
            m: [[c, -s, 0], [s, c, 0], [0, 0, ROT_ONE as i16]],
        }
    }

    /// Compose two q3.12 rotation matrices: `out = a * b`. Same i64-widened
    /// accumulator shape as [`Self::mul_vec`]; result is renormalised back
    /// to q3.12 with i16 saturation on each element.
    pub fn mul(&self, other: &Self) -> Self {
        let mut out = [[0i16; 3]; 3];
        for (r, row_out) in out.iter_mut().enumerate() {
            for (c, slot) in row_out.iter_mut().enumerate() {
                let acc = (self.m[r][0] as i64) * (other.m[0][c] as i64)
                    + (self.m[r][1] as i64) * (other.m[1][c] as i64)
                    + (self.m[r][2] as i64) * (other.m[2][c] as i64);
                let scaled = acc >> ROT_FRAC_BITS;
                *slot = scaled.clamp(i16::MIN as i64, i16::MAX as i64) as i16;
            }
        }
        Self { m: out }
    }

    /// Apply this rotation matrix to `v` and return the result in q19.12.
    /// The accumulator is widened to i64 internally to prevent overflow on
    /// cumulative multiply-adds.
    pub fn mul_vec(&self, v: GteVec3) -> GteVec3 {
        let row = |r: usize| -> i32 {
            let a = (self.m[r][0] as i64) * (v.x as i64);
            let b = (self.m[r][1] as i64) * (v.y as i64);
            let c = (self.m[r][2] as i64) * (v.z as i64);
            // Sum of three i32×i16 products fits in i64; shift by ROT_FRAC_BITS
            // to drop the q12 fractional. Clamp on conversion to i32 so an
            // overflow surfaces as saturation, not a panic.
            ((a + b + c) >> ROT_FRAC_BITS).clamp(i32::MIN as i64, i32::MAX as i64) as i32
        };
        GteVec3 {
            x: row(0),
            y: row(1),
            z: row(2),
        }
    }
}

impl Default for GteMat3 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Equivalent of `RTPT` (rotate, translate, perspective transform): apply
/// `rot * v + trans`. Result is in q19.12.
pub fn rot_trans(rot: &GteMat3, v: GteVec3, trans: GteVec3) -> GteVec3 {
    let r = rot.mul_vec(v);
    GteVec3::new(
        r.x.saturating_add(trans.x),
        r.y.saturating_add(trans.y),
        r.z.saturating_add(trans.z),
    )
}

fn clamp_i32_from_f32(f: f32) -> i32 {
    if !f.is_finite() {
        return 0;
    }
    f.round().clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

/// 2D screen-space coordinate, GTE-style q.0 in pixels (signed i16-clamped).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScreenXY {
    pub x: i32,
    pub y: i32,
}

impl ScreenXY {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Saturate to the GTE's signed-11-bit SXY-FIFO range `[-0x400, 0x3FF]`.
    /// The retail GTE pushes projected coordinates through this clamp before
    /// the OT writer reads them, raising SX2/SY2 on saturation; reproduce that
    /// so out-of-bounds verts behave the same as on hardware. Bound confirmed
    /// against a real-COP2 capture (see [`SX_MIN`]/[`SX_MAX`]).
    pub fn saturate_sxy(self) -> Self {
        Self {
            x: self.x.clamp(SX_MIN, SX_MAX),
            y: self.y.clamp(SX_MIN, SX_MAX),
        }
    }
}

/// Per-vertex GTE clip status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clip {
    /// Vertex is in front of the near plane and projection is well-defined.
    SafeFront,
    /// Vertex is behind or on the camera plane (`view.z <= 0`). The retail
    /// GTE saturates SXY in this case; the OT entry is usually skipped.
    Behind,
}

/// Output of a single GTE projection (one `RTPT` slot).
#[derive(Debug, Clone, Copy)]
pub struct ProjectedVertex {
    pub screen_xy: ScreenXY,
    pub view_z: i32,
    pub clip: Clip,
}

// ---- Perspective divide: Unsigned Newton-Raphson (UNR) reciprocal. --------
//
// The PSX GTE does NOT compute an exact `H * 0x10000 / SZ3` for the
// perspective divide. It approximates the reciprocal `1 / SZ3` with an
// Unsigned Newton-Raphson step seeded from a 257-entry table, then saturates
// the 17-bit result. Exact division diverges from hardware by up to a few
// units, and near/behind the camera (`2 * SZ3 <= H`) hardware sets the divide
// overflow flag and saturates the quotient to `0x1FFFF` instead of dividing.
//
// The 257-entry seed table is generated below from the published PSX hardware
// algorithm (no$psx "GTE Division Inaccuracy"; the same values Beetle/mednafen
// derive). It is *computed*, not copied Sony data - the same clean-room
// provenance class as the SPU Gaussian / reverb tables (`spu/gauss.rs`,
// `spu/reverb.rs`). See docs/subsystems/renderer.md.

/// Build the 257-entry UNR reciprocal seed table from the documented formula.
/// Entry `n` seeds the reciprocal of a normalized divisor whose top bits are
/// `n`; four Newton-Raphson iterations refine `xa` before it is packed to the
/// stored 8-bit correction. Const-evaluated, so no runtime initialisation.
const fn build_gte_div_table() -> [u8; 0x101] {
    let mut table = [0u8; 0x101];
    let mut divisor = 0x8000u32;
    while divisor < 0x10000 {
        let mut xa = 512u32;
        let mut i = 1;
        while i < 5 {
            // Wrapping u32 arithmetic exactly as the hardware-derivation spec
            // specifies; the Newton-Raphson recurrence keeps the value in range.
            xa = (xa.wrapping_mul((1024 * 512) - ((divisor >> 7) * xa))) >> 18;
            i += 1;
        }
        table[((divisor >> 7) & 0xFF) as usize] = (((xa + 1) >> 1).wrapping_sub(0x101)) as u8;
        divisor += 0x80;
    }
    table[0x100] = table[0xFF];
    table
}

static GTE_DIV_TABLE: [u8; 0x101] = build_gte_div_table();

/// The single Newton-Raphson refinement the GTE applies to the seed value.
/// `divisor` is the 16-bit normalized denominator (bit 15 set).
fn gte_calc_recip(divisor: u16) -> i64 {
    let idx = (((divisor as u32 & 0x7FFF) + 0x40) >> 7) as usize;
    let x = 0x101i64 + GTE_DIV_TABLE[idx] as i64;
    let tmp = (((divisor as i64) * -x) + 0x80) >> 8;
    ((x * (0x20000 + tmp)) + 0x80) >> 8
}

/// GTE perspective divide: approximate `H * 0x10000 / SZ3` via the UNR
/// reciprocal, matching PSX hardware including the near/behind-camera overflow
/// clamp. Returns `(quotient, overflow)` where `overflow` is set only when
/// `2 * SZ3 <= H` (the divide-overflow FLAG case); the plain 17-bit saturation
/// of a large quotient does not raise the flag, mirroring hardware.
///
/// `H` is the focal length register and `SZ3` the projected depth bucket, both
/// unsigned; the quotient is later multiplied by the (i16-saturated) IR1/IR2
/// numerator and shifted right by 16 to yield the screen coordinate offset.
pub fn gte_divide(h: u16, sz3: u16) -> (i64, bool) {
    // Overflow / behind-camera: hardware saturates the quotient and flags it.
    if (sz3 as u32) * 2 <= h as u32 {
        return (0x1FFFF, true);
    }
    let shift = sz3.leading_zeros(); // clz16, 0..=15 (sz3 != 0 here)
    let dividend = (h as u32) << shift;
    let divisor = (sz3 as u32) << shift; // normalized to [0x8000, 0xFFFF]
    let recip = gte_calc_recip((divisor | 0x8000) as u16);
    let result = (((dividend as i64) * recip) + 0x8000) >> 16;
    (result.min(0x1FFFF), false)
}

/// Apply a `gte_divide` quotient to one screen axis: multiply by the
/// i16-saturated numerator (IR1 for X, IR2 for Y) and drop the 16 fractional
/// bits the reciprocal carries. Caller adds `OFX`/`OFY` and clamps.
pub(crate) fn gte_persp_term(numerator_i16: i32, recip: i64) -> i64 {
    (numerator_i16 as i64 * recip) >> 16
}

/// `NCLIP`: signed 2× area of the screen-space triangle (a, b, c), used by
/// the GTE for back-face rejection. The retail TMD renderer reads `MAC0`
/// after `NCLIP` and drops primitives where the result is non-negative
/// (i.e. zero area or back-facing under PSX winding).
///
/// Returns the cross product `(b - a) × (c - a)` widened to i64 so a
/// quadrant-spanning triangle doesn't overflow.
pub fn nclip(a: ScreenXY, b: ScreenXY, c: ScreenXY) -> i64 {
    let ab_x = (b.x - a.x) as i64;
    let ab_y = (b.y - a.y) as i64;
    let ac_x = (c.x - a.x) as i64;
    let ac_y = (c.y - a.y) as i64;
    ab_x * ac_y - ab_y * ac_x
}

/// `AVSZ3`: average of three SZ values (post-RTPT view-space Z). Returns
/// the OT-bucket index `(z0 + z1 + z2) * ZSF3 / 4096`, but with `ZSF3 = 1`
/// the formula simplifies to `(z0 + z1 + z2) >> ROT_FRAC_BITS`. Caller can
/// supply a custom scale via [`avsz3_with_scale`] when reproducing a
/// retail-captured GTE state.
pub fn avsz3(z0: i32, z1: i32, z2: i32) -> i32 {
    avsz3_with_scale(z0, z1, z2, ROT_ONE)
}

/// Same as [`avsz3`] but with an explicit `zsf3` (q3.12). The retail GTE
/// uses `ZSF3 = 1024` for an "average" bucket and `ZSF3 = 4096` for a
/// "sum" bucket.
pub fn avsz3_with_scale(z0: i32, z1: i32, z2: i32, zsf3: i32) -> i32 {
    let sum = z0 as i64 + z1 as i64 + z2 as i64;
    let scaled = (sum * zsf3 as i64) >> ROT_FRAC_BITS;
    scaled.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// `AVSZ4`: same as [`avsz3`] for quads.
pub fn avsz4(z0: i32, z1: i32, z2: i32, z3: i32) -> i32 {
    avsz4_with_scale(z0, z1, z2, z3, ROT_ONE)
}

/// Same as [`avsz4`] but with an explicit `zsf4` (q3.12).
pub fn avsz4_with_scale(z0: i32, z1: i32, z2: i32, z3: i32, zsf4: i32) -> i32 {
    let sum = z0 as i64 + z1 as i64 + z2 as i64 + z3 as i64;
    let scaled = (sum * zsf4 as i64) >> ROT_FRAC_BITS;
    scaled.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// Convert a GTE-projected `ScreenXY` (q.0 pixel coord, possibly outside
/// the viewport) into integer pixel coordinates clamped to a render
/// target. Useful for offline rasterizers that don't want to handle
/// off-screen vertices specially.
pub fn screen_to_pixel(screen: ScreenXY, w: i32, h: i32) -> (i32, i32) {
    (screen.x.clamp(0, w - 1), screen.y.clamp(0, h - 1))
}

// ---- Free functions used by the register-transfer layer. -----------------

/// Sign-extend the low 16 bits of `v` to a u32. The cop2 register file
/// returns IR / matrix elements as sign-extended 32-bit values when read
/// through MFC2 / CFC2.
pub(crate) fn sign_extend_i16(v: i16) -> u32 {
    v as i32 as u32
}

/// Pack two i16 values into one u32: low half = `lo`, high half = `hi`.
/// Matches the cop2 register layout for matrix rows + SXY entries.
pub(crate) fn pack_i16_lo_hi(lo: i16, hi: i16) -> u32 {
    ((lo as u16) as u32) | (((hi as u16) as u32) << 16)
}

pub(crate) fn unpack_i16_lo_hi(v: u32) -> (i16, i16) {
    let lo = (v & 0xFFFF) as i16;
    let hi = ((v >> 16) & 0xFFFF) as i16;
    (lo, hi)
}

pub(crate) fn clamp_i32_from_i64(v: i64) -> i32 {
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// Pack IR1/IR2/IR3 into the BGR555 form the cop2cr28 (IRGB) read returns.
/// Hardware clamps each IR to `0..=0x1F` (i.e. `IR >> 7`).
pub(crate) fn packed_irgb(ir1: i32, ir2: i32, ir3: i32) -> u32 {
    let r = ((ir1 >> 7).clamp(0, 0x1F)) as u32;
    let g = ((ir2 >> 7).clamp(0, 0x1F)) as u32;
    let b = ((ir3 >> 7).clamp(0, 0x1F)) as u32;
    r | (g << 5) | (b << 10)
}

/// LZCR semantics: count leading bits that match the sign bit of `lzcs`.
/// For a non-negative `lzcs` the count of leading zeros; for a negative
/// `lzcs` the count of leading ones (0..=32).
pub(crate) fn count_leading_same(v: i32) -> u32 {
    if v < 0 {
        (!v as u32).leading_zeros()
    } else {
        (v as u32).leading_zeros()
    }
}

#[cfg(test)]
mod unr_tests {
    use super::*;

    #[test]
    fn unr_table_matches_published_seed_values() {
        // The generated table must reproduce the published PSX GTE UNR seed
        // table (no$psx / Beetle). Pin the head, the two trailing entries, and
        // the duplicated 0x100 slot - a mismatch means the formula drifted.
        let expected_head = [0xFFu8, 0xFD, 0xFB, 0xF9, 0xF7, 0xF5, 0xF3, 0xF1];
        assert_eq!(&GTE_DIV_TABLE[..8], &expected_head);
        assert_eq!(GTE_DIV_TABLE[0xFE], 0x00);
        assert_eq!(GTE_DIV_TABLE[0xFF], 0x00);
        assert_eq!(GTE_DIV_TABLE[0x100], GTE_DIV_TABLE[0xFF]);
    }

    #[test]
    fn gte_divide_near_camera_overflow_saturates_and_flags() {
        // Audit case: H=256, SZ3=100. 2*100 <= 256 ⇒ overflow, quotient
        // saturates to 0x1FFFF. With IR1=50 the projected offset is 99 (the
        // retail value), where an exact H*IR1/SZ3 would give 128.
        let (recip, overflow) = gte_divide(256, 100);
        assert!(overflow);
        assert_eq!(recip, 0x1FFFF);
        assert_eq!(gte_persp_term(50, recip), 99);
    }

    #[test]
    fn gte_divide_overflow_boundary_is_2sz3_le_h() {
        // Exactly 2*SZ3 == H still overflows (<=), one above does not.
        let (r_ovf, ovf) = gte_divide(320, 160);
        assert!(ovf);
        assert_eq!(r_ovf, 0x1FFFF);
        let (_r, no_ovf) = gte_divide(320, 161);
        assert!(!no_ovf);
        // SZ3 == 0 (behind / on the near plane) always overflows.
        assert_eq!(gte_divide(320, 0), (0x1FFFF, true));
    }

    #[test]
    fn gte_divide_matches_exact_within_documented_error_for_large_sz3() {
        // For SZ3 > H/2 (no overflow) the UNR reciprocal tracks the exact
        // divide to within the documented bound (±1, up to 2 at the boundary /
        // for extreme numerators), across the full i16 numerator range.
        for &h in &[256u16, 320] {
            let mut sz3 = (h / 2) + 1;
            while sz3 <= 8192 {
                let (recip, overflow) = gte_divide(h, sz3);
                assert!(!overflow, "sz3={sz3} above H/2 must not overflow");
                let mut ir = -32768i64;
                while ir <= 32767 {
                    let approx = gte_persp_term(ir as i32, recip);
                    let exact = (ir * h as i64) / sz3 as i64;
                    assert!(
                        (approx - exact).abs() <= 2,
                        "h={h} sz3={sz3} ir={ir}: approx={approx} exact={exact}"
                    );
                    ir += 97;
                }
                sz3 += 1;
            }
        }
    }
}
