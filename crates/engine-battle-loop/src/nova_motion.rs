//! Nova's creature motion kernels: the lift and landing heights and the spin
//! rate its arms write. `engine-core`'s `world::battle::cast_band::nova_motion`
//! applies them to the seated creature.

use legaia_engine_system::trig::trig12;

/// The arm whose pass carries the cloud banks over to the victim
/// (`0x801F7A54..0x801F7B0C`).
pub const NOVA_CLOUD_CARRY_ARM: u8 = 0x0D;

/// What PROT 0913 arm `0x13` turns the victim by a pass while it holds
/// (`addiu v0,v0,-0x600` at `0x801F82B8`), per engine tick: a battle frame
/// is two ticks.
pub const NOVA_LANDING_SPIN_PER_TICK: u16 = 0x300;

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

/// What arm `0x10` takes off the victim's heading a tick for countdown `w`:
/// `sin(w) / 2` a battle frame (`0x801F8000..0x801F8038`), half of it on
/// each of the frame's two engine ticks.
pub fn nova_lift_spin_per_tick(w: i32) -> i16 {
    let (sin, _) = trig12((w & 0xFFF) as u16);
    sin / 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lift_peaks_a_quarter_depth_up_at_a_quarter_turn() {
        assert_eq!(nova_lift_height(0, 0xC00), 0);
        assert_eq!(nova_lift_height(0x400, 0xC00), -0x300);
        assert_eq!(nova_landing_height(0x400, 0xC00), -0xC00);
        assert_eq!(nova_lift_spin_per_tick(0x400), 0x400);
    }
}
