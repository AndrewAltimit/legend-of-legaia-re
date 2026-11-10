//! The Lost Grail's *Final Heal*: the revive sweep the battle action
//! machine's cleanup state `0x50` opens with (`jal 0x801E6968` at
//! `0x801E5C6C`).
//!
//! PORT: FUN_801E6968 (the revive arms `0x801E6968..0x801E6CE4`; the tail is
//! [`crate::battle_stage_module::boss_transition_stage_id`])
//!
//! The sweep reads the acting actor's target byte `+0x1DD`: a party seat
//! (`< 3`) sweeps that seat alone, `8` sweeps every seat below the party
//! count `ctx[+0]`, anything else none. A seat is revived when its
//! character's ability bit `0x27` (record `+0xF8 & 0x80`) is set and its live
//! HP `+0x14C` is `0`: `FUN_800402F4(4, 1, seat)` stands it up at full HP.
//! Then the first accessory slot (`+0x19B..=+0x19D`) holding the Lost Grail
//! `0xE7` is emptied and the bit cleared, and a second scan sets the bit
//! again when another Lost Grail is still equipped. A seat whose bit is set
//! with no Lost Grail equipped is revived and keeps the bit.

/// The Lost Grail's item id.
pub const LOST_GRAIL: u8 = 0xE7;
/// Ability bit `0x27` in the record's second ability word (`+0xF8`).
pub const FINAL_HEAL_WORD1_BIT: u32 = 0x80;
/// The target byte that sweeps the whole party.
pub const TARGET_ALL_PARTY: u8 = 8;

/// What the sweep reads and writes of one party seat.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FinalHealMember {
    /// The seat's live HP `+0x14C`.
    pub hp: u16,
    /// The character record's ability word `+0xF8`.
    pub ability_word1: u32,
    /// The record's three accessory slots `+0x19B..=+0x19D`.
    pub accessories: [u8; 3],
}

/// One seat's arm. `true` when it revives: the caller stands the seat up at
/// full HP (`FUN_800402F4(4, 1, seat)`); the bit and the accessories are
/// updated here.
pub fn final_heal_member(m: &mut FinalHealMember) -> bool {
    if m.ability_word1 & FINAL_HEAL_WORD1_BIT == 0 || m.hp != 0 {
        return false;
    }
    if let Some(i) = m.accessories.iter().position(|&a| a == LOST_GRAIL) {
        m.accessories[i] = 0;
        m.ability_word1 &= !FINAL_HEAL_WORD1_BIT;
    }
    if m.accessories.contains(&LOST_GRAIL) {
        m.ability_word1 |= FINAL_HEAL_WORD1_BIT;
    }
    true
}

/// The whole sweep over the party seats for the acting actor's `target`
/// byte. Returns the seats revived, in order.
pub fn final_heal_sweep(target: u8, party_count: u8, seats: &mut [FinalHealMember]) -> Vec<u8> {
    let mut out = Vec::new();
    if target < 3 {
        if let Some(m) = seats.get_mut(usize::from(target))
            && final_heal_member(m)
        {
            out.push(target);
        }
    } else if target == TARGET_ALL_PARTY {
        let n = usize::from(party_count).min(seats.len());
        for (s, m) in seats.iter_mut().enumerate().take(n) {
            if final_heal_member(m) {
                out.push(s as u8);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn down(word1: u32, accessories: [u8; 3]) -> FinalHealMember {
        FinalHealMember {
            hp: 0,
            ability_word1: word1,
            accessories,
        }
    }

    #[test]
    fn the_first_grail_is_spent_and_a_second_keeps_the_bit() {
        let mut m = down(0x80, [0, LOST_GRAIL, LOST_GRAIL]);
        assert!(final_heal_member(&mut m));
        assert_eq!((m.accessories, m.ability_word1), ([0, 0, LOST_GRAIL], 0x80));
        assert!(final_heal_member(&mut m));
        assert_eq!((m.accessories, m.ability_word1), ([0, 0, 0], 0));
        assert!(!final_heal_member(&mut m), "the bit is gone");
    }

    #[test]
    fn a_bit_with_no_grail_revives_and_stays() {
        let mut m = down(0x80, [1, 2, 3]);
        assert!(final_heal_member(&mut m));
        assert_eq!(m.ability_word1, 0x80);
    }

    #[test]
    fn the_target_byte_picks_the_seats() {
        let seats = [down(0x80, [LOST_GRAIL, 0, 0]); 3];
        let mut s = seats;
        assert_eq!(final_heal_sweep(1, 3, &mut s), vec![1]);
        let mut s = seats;
        assert_eq!(final_heal_sweep(8, 2, &mut s), vec![0, 1]);
        let mut s = seats;
        assert!(
            final_heal_sweep(4, 3, &mut s).is_empty(),
            "a monster target"
        );
    }
}
