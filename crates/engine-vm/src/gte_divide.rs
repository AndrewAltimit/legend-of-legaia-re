//! The GTE perspective divide: the Unsigned Newton-Raphson (UNR) reciprocal
//! every `RTPS` / `RTPT` runs.
//!
//! The PSX GTE does NOT compute an exact `H * 0x10000 / SZ3` for the
//! perspective divide. It approximates the reciprocal `1 / SZ3` with an
//! Unsigned Newton-Raphson step seeded from a 257-entry table, then saturates
//! the 17-bit result. Exact division diverges from hardware by up to a few
//! units, and near/behind the camera (`2 * SZ3 <= H`) hardware sets the divide
//! overflow flag and saturates the quotient to `0x1FFFF` instead of dividing.
//!
//! The 257-entry seed table is generated below from the published PSX hardware
//! algorithm (no$psx "GTE Division Inaccuracy"; the same values Beetle/mednafen
//! derive). It is *computed*, not copied Sony data - the same provenance class
//! as the SPU Gaussian / reverb tables. See `docs/subsystems/renderer.md`.
//!
//! It lives in `engine-vm` - the leaf both the simulation and the draw layer
//! link - so a screen point the simulation itself needs (the fishing line's
//! rod-tip endpoint, `legaia_engine_core::fishing_actors`) and a point the
//! renderer draws divide identically. `legaia_engine_ui::gte` re-exports
//! [`gte_divide`].

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

/// The generated UNR seed table.
pub static GTE_DIV_TABLE: [u8; 0x101] = build_gte_div_table();

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
