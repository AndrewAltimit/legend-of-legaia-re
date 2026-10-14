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
