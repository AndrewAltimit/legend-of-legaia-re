//! The clean-room PSX trig LUT reproduction.
//!
//! Retail reads sine values from an in-image LUT (Sony bytes, never
//! committed). [`psx_sin`] / [`psx_cos`] compute the same q3.12 values
//! trigonometrically; the disc-gated oracle
//! `crates/engine-shell/tests/gte_sin_lut_real.rs` compares all 4096
//! entries of both tables against the user's own `SCUS_942.54` to pin the
//! reproduction.
//!
//! Lives in the GTE module - the wgpu-free leaf both hosts link - because
//! its consumers span both: the billboard projector and the battle-intro
//! transition's rotation chain each build matrices from these tables.

use super::ROT_ONE;

/// One full turn in the PSX 12-bit angle space (`0x1000` = 360°).
pub const PSX_ANGLE_TURN: u16 = 0x1000;

/// q3.12 sine of a PSX 12-bit angle (`4096` units per turn).
///
/// Mirrors the retail sin LUT at `0x80070A2C` (indexed `base + 2*angle`
/// by `FUN_8004638c` and the other `RotMatrix*` builders). The retail table
/// is `4096 * sin(2*pi*angle/4096)` **truncated toward zero** (a C `(int)`
/// cast - so `cos(tiny) = 4095` and both lobes bias one step toward zero,
/// not round-to-nearest); the disc-gated LUT oracle asserts the
/// reproduction entry-for-entry against the real table.
pub fn psx_sin(angle: u16) -> i32 {
    let a = (angle & (PSX_ANGLE_TURN - 1)) as f64;
    let radians = a * std::f64::consts::TAU / PSX_ANGLE_TURN as f64;
    (radians.sin() * ROT_ONE as f64).trunc() as i32
}

/// q3.12 cosine of a PSX 12-bit angle.
///
/// Retail has no separate cosine table: the "cos" pointer (`0x8007122C`)
/// is the sine table read `0x400` entries (90°) ahead, which is why the
/// combined LUT spans 5120 entries (1.25 turns).
pub fn psx_cos(angle: u16) -> i32 {
    psx_sin(angle.wrapping_add(PSX_ANGLE_TURN / 4))
}
