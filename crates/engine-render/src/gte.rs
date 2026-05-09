//! GTE-style fixed-point matrix transform helpers.
//!
//! The retail TMD renderer at `FUN_8002735c` (60 GTE coprocessor ops) uses
//! signed 16-bit fixed-point matrix coefficients (`q12.4` in the rotation
//! matrix, `q14.16` for translation) to transform a vertex from object →
//! world → view → screen. This module mirrors the multiply-add accumulator
//! shape with tested arithmetic so engines can validate clip-space output
//! against reference values pulled from a Mednafen/PCSX-Redux trace.
//!
//! The scope here is **not** a bit-exact GTE emulator — wgpu's f32 raster
//! pipeline owns the actual transform during rendering. This module exists
//! so downstream code (effect spawners, hit-detection, animation
//! re-targeting, mesh import tooling) has a single place to convert PSX
//! fixed-point coordinates to f32 and run the same translate-rotate-scale
//! kernel the GTE does, with the same field width and saturation. The
//! tests below pin the arithmetic; production rendering uses [`f32`] math.
//!
//! Units (matching `docs/subsystems/renderer.md`):
//! - World-position vertices: q19.12 (3D coordinate, signed)
//! - Rotation matrix: q3.12 (3×3 unit-vector basis, scaled by 4096)
//! - Translation: q19.12
//! - Output screen-space: q19.12 → snapped to integer pixels by the GPU

/// Fixed-point bit count for rotation-matrix coefficients (q3.12).
pub const ROT_FRAC_BITS: i32 = 12;

/// Fixed-point scale factor for rotation coefficients (`1 << 12 = 4096`).
/// A retail GTE rotation matrix is encoded with element values in
/// `[-32768, 32767]`; an identity rotation has diagonal = `4096`.
pub const ROT_ONE: i32 = 1 << ROT_FRAC_BITS;

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
    pub fn rot_y(angle: f32) -> Self {
        let c = (angle.cos() * ROT_ONE as f32).round() as i16;
        let s = (angle.sin() * ROT_ONE as f32).round() as i16;
        Self {
            m: [[c, 0, s], [0, ROT_ONE as i16, 0], [-s, 0, c]],
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_passes_vector_through() {
        let v = GteVec3::new(123, -456, 789);
        let r = GteMat3::IDENTITY.mul_vec(v);
        assert_eq!(r, v);
    }

    #[test]
    fn rot_y_180_negates_x_and_z() {
        let rot = GteMat3::rot_y(std::f32::consts::PI);
        let v = GteVec3::new(1000, 0, 0);
        let r = rot.mul_vec(v);
        // 180° about Y flips X (and Z when non-zero). Allow rounding error
        // up to a few units (q12 quantization → ~0.024% error per element).
        assert!((r.x - (-1000)).abs() <= 2, "x={}", r.x);
        assert_eq!(r.y, 0);
        assert!(r.z.abs() <= 2, "z={}", r.z);
    }

    #[test]
    fn rot_trans_applies_rotation_then_translation() {
        let rot = GteMat3::IDENTITY;
        let trans = GteVec3::new(10, 20, 30);
        let v = GteVec3::new(1, 2, 3);
        assert_eq!(rot_trans(&rot, v, trans), GteVec3::new(11, 22, 33));
    }

    #[test]
    fn fixed_point_round_trip() {
        let original = (1.5f32, -3.25, 0.125);
        let v = GteVec3::from_f32_q12(original.0, original.1, original.2);
        let back = v.to_f32_q12();
        // q12 fixed-point gives ~1/4096 resolution. Each example here is
        // exactly representable.
        assert!((back.0 - original.0).abs() < 1.0 / ROT_ONE as f32 + 1e-6);
        assert!((back.1 - original.1).abs() < 1.0 / ROT_ONE as f32 + 1e-6);
        assert!((back.2 - original.2).abs() < 1.0 / ROT_ONE as f32 + 1e-6);
    }

    #[test]
    fn mul_vec_does_not_overflow_on_max_inputs() {
        // Worst case: rotation with max elements (32767) applied to a
        // vector with max coordinates (i32::MAX / 4 to keep headroom).
        // i64 accumulator must absorb 3 × i32×i16 without panicking.
        let m = GteMat3 {
            m: [[32767, 32767, 32767], [0, 0, 0], [0, 0, 0]],
        };
        let v = GteVec3::new(i32::MAX / 4, i32::MAX / 4, i32::MAX / 4);
        // Should not panic, even on overflow we get saturation.
        let r = m.mul_vec(v);
        // r.x = 3 * (32767 * (i32::MAX/4)) >> 12 ~ 3 * 32767 * (5.4e8) / 4096
        //     ~ 3 * 4.32e9 — saturates to i32::MAX.
        assert_eq!(r.x, i32::MAX);
    }
}
