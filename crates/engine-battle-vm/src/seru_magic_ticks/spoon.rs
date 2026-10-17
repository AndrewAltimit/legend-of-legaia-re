//! Spoon's heal amount and cure sweep, the Orb heal's doubled sibling.

use super::*;

/// The phase of PROT 0919 (Spoon) whose arm heals and cures
/// (`0x801F78A8`; its sweep is `0x801F7F48..0x801F83D8`).
pub const SPOON_HEAL_ARM: u8 = 7;

/// PROT 0919 (Spoon) arm 7's cure half - the status ladder and the tier-`4`
/// AP doubling - over party seats `0..3`.
///
/// Retail walks a fixed three seats (`sltiu v0,s3,0x3` at `0x801F83D0`),
/// skips a dead one whole (`beq v0,zero,0x801f83c4` at `0x801F7F98`), and
/// once the caster's magic level is `>= 3` (`0x801F815C`) switches on the
/// cure selector `0x801F6960` (`0x801F8168`): the same four keep-masks as
/// Orb ([`orb_cleanse_mask`]), and selector `4` doubles the seat's AP gauge
/// `+0x170` under a cap of 100 (`0x801F8394..0x801F83C0`). `+0x16E & 4`
/// skips only the HP store (`0x801F8100`), so the ladder still runs there.
/// `cleanse` is `None` below the level gate.
///
/// The heal itself is the engine's cast fold's, as for every heal in the
/// band; this is the half with no second owner.
///
/// Returns the seats whose AP was doubled.
///
/// REF: FUN_801F69D8 (PROT 0919 arm 7 `0x801F7F48..0x801F83D8`; the phase chain is `chain_bodies::SPOON_CHAIN`)
pub fn spoon_cure_sweep(seats: &mut [CastActorState], cleanse: Option<u8>) -> Vec<u8> {
    let mut doubled = Vec::new();
    let Some(mask) = cleanse.and_then(orb_cleanse_mask) else {
        return doubled;
    };
    for seat in 0..ORB_PARTY_SEATS {
        let Some(v) = seats.get_mut(seat as usize) else {
            continue;
        };
        if v.hp == 0 {
            continue;
        }
        v.flags &= mask;
        if cleanse == Some(4) {
            v.spirit_gauge = crate::seru_magic_ticks::cure_tier4_ap(v.spirit_gauge);
            doubled.push(seat);
        }
    }
    doubled
}
