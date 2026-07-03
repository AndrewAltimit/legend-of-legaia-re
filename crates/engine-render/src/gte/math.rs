/// Fixed-point bit count for rotation-matrix coefficients (q3.12).
pub const ROT_FRAC_BITS: i32 = 12;

/// Fixed-point scale factor for rotation coefficients (`1 << 12 = 4096`).
/// A retail GTE rotation matrix is encoded with element values in
/// `[-32768, 32767]`; an identity rotation has diagonal = `4096`.
pub const ROT_ONE: i32 = 1 << ROT_FRAC_BITS;

/// Default GTE focal length in pixels (the value the retail TMD renderer
/// loads into `H` for the standard 320×240 PSX frame: `H = 320`).
pub const DEFAULT_H: i32 = 320;

/// Saturated 16-bit signed clamp. The GTE saturates to `[-32768, 32767]`
/// when storing screen-space coordinates back into the SXY FIFO.
pub const SXY_MIN: i32 = i16::MIN as i32;
pub const SXY_MAX: i32 = i16::MAX as i32;

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

    /// Saturate to the GTE's i16 SXY-FIFO range. The retail GTE pushes
    /// off-screen coordinates through this clamp before the OT writer
    /// reads them; reproduce that saturation here so out-of-bounds verts
    /// behave the same as on hardware.
    pub fn saturate_sxy(self) -> Self {
        Self {
            x: self.x.clamp(SXY_MIN, SXY_MAX),
            y: self.y.clamp(SXY_MIN, SXY_MAX),
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

pub(crate) fn saturate_behind(numerator: i32) -> i32 {
    if numerator > 0 {
        SXY_MAX
    } else if numerator < 0 {
        SXY_MIN
    } else {
        0
    }
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
