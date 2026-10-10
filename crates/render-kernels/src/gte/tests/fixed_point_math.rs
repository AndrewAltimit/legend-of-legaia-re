//! Fixed-point vector and matrix math.

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
    let r = m.mul_vec(v);
    assert_eq!(r.x, i32::MAX);
}

#[test]
fn rot_x_90_y_to_z() {
    // 90° pitch around +X axis sends +Y to +Z.
    let rot = GteMat3::rot_x(std::f32::consts::FRAC_PI_2);
    let v = GteVec3::new(0, 1000, 0);
    let r = rot.mul_vec(v);
    assert!(r.y.abs() <= 2, "y={}", r.y);
    assert!((r.z - 1000).abs() <= 2, "z={}", r.z);
}

#[test]
fn rot_z_90_x_to_y() {
    // 90° roll around +Z axis sends +X to +Y.
    let rot = GteMat3::rot_z(std::f32::consts::FRAC_PI_2);
    let v = GteVec3::new(1000, 0, 0);
    let r = rot.mul_vec(v);
    assert!(r.x.abs() <= 2, "x={}", r.x);
    assert!((r.y - 1000).abs() <= 2, "y={}", r.y);
}

#[test]
fn mat3_mul_identity_is_noop() {
    let r = GteMat3::rot_y(0.7);
    let combined = r.mul(&GteMat3::IDENTITY);
    // Identity composition should be lossless within q3.12 rounding.
    for i in 0..3 {
        for j in 0..3 {
            assert!(
                (combined.m[i][j] as i32 - r.m[i][j] as i32).abs() <= 1,
                "[{i}][{j}] mismatch: combined={} vs r={}",
                combined.m[i][j],
                r.m[i][j],
            );
        }
    }
}

#[test]
fn mat3_mul_compose_two_y_rotations() {
    // rot_y(a) * rot_y(b) ≈ rot_y(a + b) - verify within q3.12 rounding.
    let a = std::f32::consts::FRAC_PI_4;
    let b = std::f32::consts::FRAC_PI_3;
    let composed = GteMat3::rot_y(a).mul(&GteMat3::rot_y(b));
    let direct = GteMat3::rot_y(a + b);
    for i in 0..3 {
        for j in 0..3 {
            assert!(
                (composed.m[i][j] as i32 - direct.m[i][j] as i32).abs() <= 4,
                "[{i}][{j}] composed={} direct={}",
                composed.m[i][j],
                direct.m[i][j],
            );
        }
    }
}
