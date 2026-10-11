//! The reel cylinders: placement, strip length, the face angle ladder and the
//! depth shade `FUN_801d0fa8` gives each face.

// ---------------------------------------------------------------------------
// Reel cylinder (FUN_801d0fa8)
// ---------------------------------------------------------------------------

/// Model-space `x` of reel 0's left edge (`FUN_801cf0d8`: `iVar13 = -0x200`).
pub const REEL_X0: i32 = -0x200;
/// Model-space `x` step between reels (`iVar13 += 0x180`).
pub const REEL_X_STEP: i32 = 0x180;
/// Model-space width of one reel (each face spans `x .. x + 0x100`).
pub const REEL_WIDTH: i32 = 0x100;
/// Reels on the machine.
pub const REEL_COUNT: usize = 3;
/// Symbols on one reel strip.
pub const STRIP_LEN: i32 = 0x14;

/// Base angle of the first emitted reel face (`param_2 + 0x380 + frac`).
pub const REEL_ANGLE_BASE: i32 = 0x380;
/// Angle subtended by one symbol (`0x1000` = a full turn).
pub const REEL_ANGLE_STEP: i32 = 0x100;
/// A full turn, in the 12-bit angle the SCUS trig tables are indexed by.
pub const ANGLE_FULL: i32 = 0x1000;
/// Reel faces emitted per reel, per frame.
pub const REEL_FACES: usize = 8;

/// The reel cylinder's `y` radius: `y(a) = (sin(a) * -0x249) >> 12`.
pub const REEL_Y_RADIUS: i32 = 0x249;
/// The reel cylinder's `z` radius shift: `z(a) = cos(a) >> 3`, i.e. radius 512.
pub const REEL_Z_SHIFT: u32 = 3;

/// Peak gouraud shade of a reel face (`0xB4`). The retail blend is
/// `texel * shade / 128`, so `0xB4` is a 1.41x brighten, not a clamp at 1.0.
pub const REEL_SHADE_MAX: i32 = 0xB4;
/// Depth-cue bias: the shade peaks at `z = -REEL_SHADE_Z_BIAS`.
pub const REEL_SHADE_Z_BIAS: i32 = 0x200;
/// Depth-cue gain (`(z + bias) * 0x21C >> 9`).
pub const REEL_SHADE_Z_GAIN: i32 = 0x21C;
/// The blend divisor of a `POLY_GT4`'s vertex colour.
pub const SHADE_NEUTRAL: i32 = 128;

/// SCUS virtual address of the 4096-entry **sine** table (4.12 fixed, amplitude
/// `0x1000`) the reel renderer reaches through the pointer `_DAT_8007b81c`.
pub const SIN_TABLE_VA: u32 = 0x8007_0A2C;
/// SCUS virtual address of the matching **cosine** table (`_DAT_8007b7f8`).
pub const COS_TABLE_VA: u32 = 0x8007_122C;

/// `sin(angle)` in 4.12 fixed point, amplitude `0x1000` - the SCUS table at
/// [`SIN_TABLE_VA`], reproduced.
///
/// The retail entries are `trunc(0x1000 * sin)`, **truncating toward zero**,
/// not rounding: a stand-in that rounds disagrees with the disc table on about
/// half of the 4096 entries by one LSB, and every consumer multiplies that
/// error by a radius before shifting it back down. Nothing here reproduces a
/// table *value* - the arithmetic is analytic and the disc is the oracle
/// (`engine-scene/tests/minigame_polar_trig_tables_disc.rs`).
// REF: FUN_801d0fa8 (the reel renderer's trig-table reads)
pub fn sin_4096(angle: i32) -> i32 {
    let a = angle.rem_euclid(ANGLE_FULL) as f64 * (core::f64::consts::TAU / ANGLE_FULL as f64);
    // `as i32` on an f64 truncates toward zero - the retail table's rounding.
    (4096.0 * a.sin()) as i32
}

/// `cos(angle)` in 4.12 fixed point - the SCUS table at [`COS_TABLE_VA`].
///
/// That table *is* the sine one read a quarter turn ahead (the two VAs are
/// `0x800` bytes apart), so this is the same read at a phase offset rather than
/// an independent series - which is what keeps the two in exact agreement at
/// the truncation boundaries.
pub fn cos_4096(angle: i32) -> i32 {
    sin_4096(angle + ANGLE_FULL / 4)
}

/// Model-space `y` of the reel cylinder at `angle` (`FUN_801d0fa8`).
// PORT: FUN_801d0fa8 (reel cylinder y: sin(a) * -0x249 >> 12)
//
// The native window draws the cylinders through this kernel
// (`engine-ui::slot_cabinet::slot_cabinet_prims`, from the play-window's
// slot-machine frame). The browser pages still do the cylinder maths in
// JavaScript: `slot_marquee_json` exports this module's *constants*
// (REEL_Y_RADIUS, REEL_Z_SHIFT, REEL_SHADE_*, ANGLE_FULL, PROJ_*) and
// `slotDrawReels` in site/_content/minigames.html recomputes y / z / shade in
// floats, without retail's fixed-point round-toward-zero biases
// (`(v + 0xFFF) >> 12`, `(v + 7) >> 3`). `asset slot-scene` prints the
// fixed-point table so the two can be compared instead of assumed equal.
pub fn reel_y(angle: i32) -> i32 {
    let v = sin_4096(angle) * -REEL_Y_RADIUS;
    // Retail biases negatives before the arithmetic shift (round toward zero).
    if v < 0 { (v + 0xFFF) >> 12 } else { v >> 12 }
}

/// Model-space `z` of the reel cylinder at `angle` (`FUN_801d0fa8`).
// PORT: FUN_801d0fa8 (reel cylinder z: cos(a) >> 3)
// Drawn natively through `slot_cabinet`; the browser pages recompute it
// in floating-point JavaScript (see `reel_y`).
pub fn reel_z(angle: i32) -> i32 {
    let v = cos_4096(angle);
    if v < 0 {
        (v + 7) >> REEL_Z_SHIFT
    } else {
        v >> REEL_Z_SHIFT
    }
}

/// The depth-cued gouraud shade of a reel vertex at model-space `z`, clamped to
/// `0 ..= 0xB4` (`FUN_801d0fa8`). Feed it to a `texel * shade / 128` blend.
// PORT: FUN_801d0fa8 (reel depth-cue shade)
// Drawn natively through `slot_cabinet`; the browser pages recompute it
// in floating-point JavaScript (see `reel_y`).
pub fn reel_shade(z: i32) -> i32 {
    let v = (z + REEL_SHADE_Z_BIAS) * REEL_SHADE_Z_GAIN;
    let v = if v < 0 { (v + 0x1FF) >> 9 } else { v >> 9 };
    (REEL_SHADE_MAX - v).clamp(0, REEL_SHADE_MAX)
}

/// Model-space left edge of reel `r`.
pub fn reel_x(r: usize) -> i32 {
    REEL_X0 + (r as i32) * REEL_X_STEP
}
