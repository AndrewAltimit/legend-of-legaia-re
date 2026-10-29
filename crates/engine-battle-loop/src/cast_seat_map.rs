//! The cast seat map: how the slot-B modules' retail actor-table rows map onto
//! the engine's battle slots. `engine-core`'s `world::battle::cast_band::seat_map`
//! builds it off the live seats.

/// The engine-slot <-> retail-pool-slot map for one battle.
///
/// Engine slot `e < pc` is retail slot `e`; engine slot `e >= pc` is retail
/// slot `e + (3 - pc)`. Retail slots `pc..3` are a small party's empty seats
/// and map to no engine slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastSeatMap {
    /// The engine's party count, clamped to `1..=3`.
    pc: u8,
    /// The engine's actor-table length.
    engine_len: usize,
}

/// Retail's first monster seat, the fixed base the kernels assume.
pub const FIRST_MONSTER: u8 = legaia_engine_cast::cast_module_ticks::FIRST_MONSTER_SEAT;

/// Retail's seats `3..8` are the five monster seats.
pub const MONSTER_SEATS: u8 = 5;

impl CastSeatMap {
    pub fn new(party_count: u8, engine_len: usize) -> Self {
        Self {
            pc: party_count.clamp(1, FIRST_MONSTER),
            engine_len,
        }
    }

    /// How many empty party seats the retail row inserts.
    fn gap(self) -> u8 {
        FIRST_MONSTER - self.pc
    }

    /// The retail-shaped row's length: every engine slot has a retail seat.
    pub fn retail_len(self) -> usize {
        self.engine_len + usize::from(self.gap())
    }

    /// The retail seat engine slot `engine` occupies.
    pub fn retail(self, engine: u8) -> u8 {
        if engine < self.pc {
            engine
        } else {
            engine.saturating_add(self.gap())
        }
    }

    /// The engine slot behind retail seat `retail`, or `None` for an empty
    /// party seat.
    pub fn engine(self, retail: u8) -> Option<u8> {
        if retail < self.pc {
            Some(retail)
        } else if retail < FIRST_MONSTER {
            None
        } else {
            Some(retail - self.gap())
        }
    }

    /// A `+0x1DD` target code in the engine's space, as retail would hold it:
    /// a seat index for a party or monster seat, and the group codes (`8`
    /// party-wide, `9` enemy-wide) untouched. A code past the engine's
    /// monster row names no combatant and passes through.
    pub fn target_to_retail(self, code: u8) -> u8 {
        if code >= self.pc && code < self.pc + MONSTER_SEATS {
            code + self.gap()
        } else {
            code
        }
    }

    /// The inverse of [`Self::target_to_retail`] over the codes it produces.
    /// An empty party seat has no engine slot; it is kept as retail named it
    /// (it is below `3`, so it still reads as "the party row").
    pub fn target_to_engine(self, code: u8) -> u8 {
        if (FIRST_MONSTER..FIRST_MONSTER + MONSTER_SEATS).contains(&code) {
            code - self.gap()
        } else {
            code
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CastSeatMap;

    #[test]
    fn a_full_party_maps_every_seat_and_code_to_itself() {
        let m = CastSeatMap::new(3, 12);
        assert_eq!(m.retail_len(), 12);
        for s in 0..12u8 {
            assert_eq!(m.retail(s), s);
            assert_eq!(m.engine(s), Some(s));
        }
        for c in 0..=255u8 {
            assert_eq!(m.target_to_retail(c), c);
            assert_eq!(m.target_to_engine(c), c);
        }
    }

    #[test]
    fn a_lone_member_leaves_seats_one_and_two_empty() {
        let m = CastSeatMap::new(1, 10);
        assert_eq!(m.retail_len(), 12);
        assert_eq!(m.engine(0), Some(0));
        assert_eq!(m.engine(1), None);
        assert_eq!(m.engine(2), None);
        // Monster k sits at retail 3 + k, engine 1 + k.
        for k in 0..5u8 {
            assert_eq!(m.engine(3 + k), Some(1 + k));
            assert_eq!(m.retail(1 + k), 3 + k);
            assert_eq!(m.target_to_retail(1 + k), 3 + k);
            assert_eq!(m.target_to_engine(3 + k), 1 + k);
        }
        // The summon seat the hosts use (`8 + party_count`) has a seat too.
        assert_eq!(m.engine(m.retail(9)), Some(9));
        // The group codes are shared.
        assert_eq!(m.target_to_retail(8), 8);
        assert_eq!(m.target_to_retail(9), 9);
        assert_eq!(m.target_to_engine(8), 8);
        assert_eq!(m.target_to_engine(9), 9);
    }

    #[test]
    fn a_pair_leaves_seat_two_empty() {
        let m = CastSeatMap::new(2, 10);
        assert_eq!(m.engine(1), Some(1));
        assert_eq!(m.engine(2), None);
        assert_eq!(m.engine(3), Some(2));
        assert_eq!(m.target_to_retail(2), 3);
        assert_eq!(m.target_to_engine(3), 2);
    }
}
