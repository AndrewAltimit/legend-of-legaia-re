//! The engine's 12-bit trig: the `SCUS_942.54` sine table at `0x80070A2C`
//! sampled by formula. [`crate::rotation_lut`] holds the same table as the
//! pair the rotation steppers index.

/// Sine table sample of a 12-bit angle, `trunc(sin(a * 2pi / 4096) * 4096)`.
///
/// This is the formula of the SCUS-resident table at `0x80070A2C`
/// (`_DAT_8007B81C`; disc-verified byte-exact by the engine's
/// [`crate::rotation_lut::RetailRotationLut`], truncation toward zero). The
/// cosine read is the same table at `+0x800` bytes (`_DAT_8007B7F8`), i.e.
/// the angle shifted a quarter turn.
pub fn sin12(angle: u16) -> i16 {
    let a = (angle & 0xFFF) as f64;
    (f64::sin(a * std::f64::consts::TAU / 4096.0) * 4096.0).trunc() as i16
}

/// `(sin, cos)` LUT pair of a 12-bit angle - the `_DAT_8007B81C` /
/// `_DAT_8007B7F8` read pair every battle motion consumer performs.
pub fn trig12(angle: u16) -> (i16, i16) {
    (sin12(angle), sin12(angle.wrapping_add(0x400)))
}

/// Entries in the `SCUS_942.54` arctangent table at `0x8006F4C8`: the ratio
/// index `(min << 11) / max` runs `0..=0x800`.
pub const ATAN_LUT_LEN: usize = 0x801;

/// The arctangent table at `0x8006F4C8` by formula: entry `i` is
/// `trunc(atan(i / 2048) * 4096 / 2pi)`, the arctan of the ratio in 12-bit
/// turn units truncated toward zero. Byte for byte the retail table (the
/// retail-exec differential holds every entry); rounding instead moves 1037 of
/// the 2049 entries up by one.
pub fn atan_lut() -> &'static [i16; ATAN_LUT_LEN] {
    use std::sync::OnceLock;
    static LUT: OnceLock<[i16; ATAN_LUT_LEN]> = OnceLock::new();
    LUT.get_or_init(|| {
        let mut t = [0i16; ATAN_LUT_LEN];
        for (i, slot) in t.iter_mut().enumerate() {
            let ratio = i as f64 / 2048.0;
            *slot = (ratio.atan() * 4096.0 / std::f64::consts::TAU).trunc() as i16;
        }
        t
    })
}

/// The 12-bit bearing of a displacement `(dz, dx)`, `FUN_80019B28` past its
/// two subtractions: `0x000` toward `+Z`, `0x400` toward `+X`, `0x800` toward
/// `-Z`, `0xC00` toward `-X`. The signs pick a quadrant, the shorter leg is
/// divided into the longer (`(min << 11) / max`), [`atan_lut`] is read at the
/// quotient and the quadrant's constant reassembles the circle, masked to
/// `0xFFF`. Retail takes the two points as halfwords and subtracts in 32 bits,
/// so a delta may run past `i16`.
// PORT: FUN_80019B28
pub fn bearing12(dz: i32, dx: i32) -> u16 {
    let lut = atan_lut();
    let (mut dz, mut dx) = (dz, dx);
    let mut quad = 0u32;
    if dz < 0 {
        dz = -dz;
        quad |= 2;
    }
    if dx < 0 {
        dx = -dx;
        quad |= 1;
    }
    let at = |numer: i32, denom: i32| -> i32 { i32::from(lut[((numer << 11) / denom) as usize]) };
    let raw: i32 = match quad {
        0 if dz >= dx => {
            if dx == 0 {
                0
            } else {
                at(dx, dz)
            }
        }
        0 => 0x400 - at(dz, dx),
        1 if dz >= dx => {
            if dx == 0 {
                0x1000
            } else {
                0x1000 - at(dx, dz)
            }
        }
        1 => {
            if dz == 0 {
                0xC00
            } else {
                at(dz, dx) + 0xC00
            }
        }
        2 if dz >= dx => {
            if dx == 0 {
                0x800
            } else {
                0x800 - at(dx, dz)
            }
        }
        2 => {
            if dz == 0 {
                0x400
            } else {
                at(dz, dx) + 0x400
            }
        }
        _ if dz >= dx => {
            if dx == 0 {
                0x800
            } else {
                at(dx, dz) + 0x800
            }
        }
        _ => {
            if dz == 0 {
                0xC00
            } else {
                0xC00 - at(dz, dx)
            }
        }
    };
    (raw & 0xFFF) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin12_matches_the_retail_table_anchors() {
        // Quarter-turn anchors of the 0x80070A2C table.
        assert_eq!(sin12(0), 0);
        assert_eq!(sin12(0x400), 4096);
        assert_eq!(sin12(0x800), 0);
        assert_eq!(sin12(0xC00), -4096);
        // Truncation toward zero (not rounding): sin(1/4096 turn).
        assert_eq!(sin12(1), 6); // 4096*sin(2pi/4096) = 6.283.. -> 6
        // cos = the same table a quarter turn on.
        assert_eq!(trig12(0), (0, 4096));
        assert_eq!(trig12(0x400), (4096, 0));
    }
}
