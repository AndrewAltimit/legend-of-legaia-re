//! Battle slot numbering: retail's fixed actor pool against the engine's
//! compact seating.
//!
//! Retail seats party member `i` at pool slot `i` and monster `k` at pool
//! slot `3 + k` **whatever the party size** (`FUN_800513F0`, `addiu s0,s2,0x3`
//! at `0x8005185C`), so a party of one leaves pool slots `1` and `2` empty and
//! every `slot < 3` test in the overlay means "a party seat". The engine
//! seats monster `k` straight after the party, at `party_count + k`, and every
//! actor index it stores (`active_actor`, a target byte `+0x1DD`, a loop
//! index over `World::actors`) is an **engine slot**.
//!
//! The two agree only for a full party. A raw engine slot handed to a test
//! written in retail's numbering reads a small party's first monsters as
//! party seats, which is how a two-member party's Nighto kill parked the
//! state-`0x51` settle gate for good. So every crossing between the two
//! numberings goes through [`SeatMap`], and a kernel whose test is on the
//! pool slot takes a [`RetailSlot`] - an engine slot has to be converted
//! before it type-checks.
//!
//! Target codes `>= 8` (`8` = whole party, `9` = whole enemy row) are not
//! slots and pass through both directions unchanged.

/// Party seats in retail's pool (`0..3`).
pub const RETAIL_PARTY_SEATS: u8 = 3;

/// Retail's first monster pool slot (`addiu s0,s2,0x3` at `0x8005185C`).
pub const RETAIL_FIRST_MONSTER: u8 = 3;

/// The first target code that is not a slot (`8` = party-wide).
pub const FIRST_TARGET_CODE: u8 = 8;

/// A slot in retail's fixed actor-pool numbering: party `0..3`, monster `k`
/// at `3 + k`, codes `>= 8` passed through.
///
/// Built from an engine slot through [`SeatMap::retail`], or from a value
/// that is already in retail's numbering (a disc literal, a retail capture)
/// through [`RetailSlot::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct RetailSlot(u8);

impl RetailSlot {
    /// Wrap a value that is **already** a retail pool slot. Never pass an
    /// engine slot here; use [`SeatMap::retail`].
    pub const fn new(raw: u8) -> Self {
        Self(raw)
    }

    /// The raw pool-slot byte.
    pub const fn get(self) -> u8 {
        self.0
    }

    /// Retail's `slot < 3`: a party seat.
    pub const fn is_party(self) -> bool {
        self.0 < RETAIL_PARTY_SEATS
    }

    /// A monster seat (`3..8`), excluding the target codes.
    pub const fn is_monster(self) -> bool {
        self.0 >= RETAIL_FIRST_MONSTER && self.0 < FIRST_TARGET_CODE
    }
}

/// The map between engine slots and retail pool slots for one battle's party
/// size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeatMap {
    /// The party count, clamped to `1..=3`.
    pc: u8,
}

impl SeatMap {
    /// The map for a battle seating `party_count` party members (clamped to
    /// `1..=3`, retail's party row).
    pub const fn new(party_count: u8) -> Self {
        let pc = if party_count < 1 {
            1
        } else if party_count > RETAIL_PARTY_SEATS {
            RETAIL_PARTY_SEATS
        } else {
            party_count
        };
        Self { pc }
    }

    /// The clamped party count.
    pub const fn party_count(self) -> u8 {
        self.pc
    }

    /// Engine slot `engine` is a party seat (`engine < party_count`).
    pub const fn is_party(self, engine: u8) -> bool {
        engine < self.pc
    }

    /// The retail pool slot engine slot `engine` occupies. Party seats keep
    /// their index, monster `k` (engine `party_count + k`) lands on `3 + k`;
    /// codes `>= 8` pass through.
    pub const fn retail(self, engine: u8) -> RetailSlot {
        RetailSlot(if engine >= FIRST_TARGET_CODE || engine < self.pc {
            engine
        } else {
            engine - self.pc + RETAIL_FIRST_MONSTER
        })
    }

    /// The engine slot behind retail pool slot `retail`, or `None` for a
    /// party seat the battle leaves empty. Codes `>= 8` pass through.
    pub const fn engine(self, retail: u8) -> Option<u8> {
        if retail >= FIRST_TARGET_CODE || retail < self.pc {
            Some(retail)
        } else if retail < RETAIL_FIRST_MONSTER {
            None
        } else {
            Some(retail - RETAIL_FIRST_MONSTER + self.pc)
        }
    }
}

/// [`SeatMap::retail`] as a raw byte, for a caller holding only the party
/// count.
pub fn retail_pool_slot(party_count: u8, slot: u8) -> u8 {
    SeatMap::new(party_count).retail(slot).get()
}

/// [`SeatMap::engine`] for a caller holding only the party count.
pub fn engine_slot_for_retail(party_count: u8, retail: u8) -> Option<u8> {
    SeatMap::new(party_count).engine(retail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_party_maps_every_slot_to_itself() {
        let m = SeatMap::new(3);
        for s in 0..10 {
            assert_eq!(m.retail(s).get(), s);
            assert_eq!(m.engine(s), Some(s));
        }
    }

    #[test]
    fn small_parties_put_the_first_monster_on_pool_slot_three() {
        for pc in 1..=3u8 {
            let m = SeatMap::new(pc);
            for p in 0..pc {
                assert!(m.retail(p).is_party());
                assert!(m.is_party(p));
            }
            for k in 0..5u8 {
                let e = pc + k;
                let r = m.retail(e);
                assert_eq!(r.get(), 3 + k, "pc {pc} monster {k}");
                assert!(!r.is_party());
                assert!(!m.is_party(e));
                assert_eq!(m.engine(r.get()), Some(e));
            }
            for empty in pc..3 {
                assert_eq!(m.engine(empty), None, "pc {pc} empty seat {empty}");
            }
            assert_eq!(m.retail(8).get(), 8);
            assert_eq!(m.retail(9).get(), 9);
            assert_eq!(m.engine(8), Some(8));
            assert_eq!(m.engine(9), Some(9));
        }
    }

    /// Property: for every party size and every engine slot naming a
    /// combatant, the party/monster verdict of the retail slot equals the
    /// engine's own `slot < party_count`, and the map round-trips.
    #[test]
    fn the_party_verdict_survives_the_map_for_every_seating() {
        for party_count in 0..=5u8 {
            let m = SeatMap::new(party_count);
            for e in 0..(m.party_count() + 5) {
                let r = m.retail(e);
                assert_eq!(r.is_party(), m.is_party(e), "pc {party_count} slot {e}");
                assert_eq!(r.is_monster(), !m.is_party(e));
                assert_eq!(m.engine(r.get()), Some(e));
            }
        }
    }
}
