//! Nova's creature motion kernels: the lift and landing heights and the spin
//! rate its arms write. `engine-core`'s `world::battle::cast_band::nova_motion`
//! applies them to the seated creature.

use legaia_engine_system::trig::trig12;

/// The arm whose pass carries the cloud banks over to the victim
/// (`0x801F7A54..0x801F7B0C`).
pub const NOVA_CLOUD_CARRY_ARM: u8 = 0x0D;

/// The arms that jitter every whirlwind ring round the victim on each of
/// their passes: `0x0E` (`0x801F7C80..0x801F7D04`), `0x0F`
/// (`0x801F7D8C..0x801F7E10`) and `0x10` (`0x801F7F48..0x801F7FCC`), each the
/// same loop over the sixteen handles at `0x801F9134`.
pub const NOVA_RING_JITTER_ARMS: [u8; 3] = [0x0E, 0x0F, 0x10];

/// One ring coordinate's jitter off a `rand()` draw: `r % 63 - 0x20`
/// (`mult 0x82082083`, `sra 5`, `q * 63` at `0x801F7C88..0x801F7CB8`).
pub fn nova_ring_jitter(r: u32) -> i16 {
    (r % 63) as i16 - 0x20
}

/// A whirlwind ring's yaw off its `rand()` draw: `r % 0xFFF` (`mult
/// 0x80080081`, `sra 0xB`, `q * 0xFFF` at `0x801F7BC4..0x801F7BF4`).
pub fn nova_ring_yaw(r: u32) -> i16 {
    (r % 0xFFF) as i16
}

/// What PROT 0913 arm `0x13` turns the victim by a pass while it holds
/// (`addiu v0,v0,-0x600` at `0x801F82B8`): a fixed step a **pass**, whatever
/// the frame step, so the port takes it on the tick that opens the pass.
pub const NOVA_LANDING_SPIN_PER_PASS: u16 = 0x600;

/// The victim's height under PROT 0913 arm `0x10`'s climb for the countdown
/// `w`: `-sin(w) * ctx[+0x6D0] >> 14` (`0x801F803C..0x801F8064`, the `+0x3FFF`
/// rounding of a negative product included).
pub fn nova_lift_height(w: i32, depth: i16) -> i16 {
    let (sin, _) = trig12((w & 0xFFF) as u16);
    let p = -i32::from(sin) * i32::from(depth);
    let p = if p < 0 { p + 0x3FFF } else { p };
    (p >> 14) as i16
}

/// The victim's height while arm `0x13` brings it down, countdown `w`:
/// `-sin(w) * ctx[+0x6D0] >> 12` (`0x801F82C0..0x801F82F4`).
pub fn nova_landing_height(w: i32, depth: i16) -> i16 {
    let (sin, _) = trig12((w & 0xFFF) as u16);
    let p = -i32::from(sin) * i32::from(depth);
    let p = if p < 0 { p + 0xFFF } else { p };
    (p >> 12) as i16
}

/// What arm `0x10` takes off the victim's heading a pass for countdown `w`:
/// `sin(w) / 2` (`0x801F8000..0x801F8038`, `sra 1` of the rounded sine). The
/// step is per pass, not scaled by the frame step: a pass of step `3` turns
/// the victim as far as a pass of step `1`, so over the arm's game time the
/// spin follows the number of passes retail ran.
pub fn nova_lift_spin_per_pass(w: i32) -> i16 {
    let (sin, _) = trig12((w & 0xFFF) as u16);
    let s = i32::from(sin);
    ((s + i32::from(s < 0)) >> 1) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_draws_fold_as_the_module_divides() {
        assert_eq!(nova_ring_jitter(0), -0x20);
        assert_eq!(nova_ring_jitter(62), 30);
        assert_eq!(nova_ring_jitter(63), -0x20);
        assert_eq!(nova_ring_jitter(0x7FFF), (0x7FFF % 63) as i16 - 0x20);
        assert_eq!(nova_ring_yaw(0xFFF), 0);
        assert_eq!(nova_ring_yaw(0x7FFF), (0x7FFF % 0xFFF) as i16);
    }

    #[test]
    fn the_lift_peaks_a_quarter_depth_up_at_a_quarter_turn() {
        assert_eq!(nova_lift_height(0, 0xC00), 0);
        assert_eq!(nova_lift_height(0x400, 0xC00), -0x300);
        assert_eq!(nova_landing_height(0x400, 0xC00), -0xC00);
        assert_eq!(nova_lift_spin_per_pass(0x400), 0x800);
        assert_eq!(nova_lift_spin_per_pass(0xC00), -0x800);
    }
}
