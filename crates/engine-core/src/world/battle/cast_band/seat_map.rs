//! The seat array the slot-B cast-module kernels walk, laid out the way
//! retail lays it out.
//!
//! Retail's battle loader seats party member `i` at pool slot `i` and monster
//! `k` at `3 + k` whatever the party size (`FUN_800513F0`, the monster loop's
//! `addiu s0,s2,0x3` at `0x8005185C`); a party of one or two leaves the
//! unused party slots as zeroed actor structs (`+0x14C` HP `0`, `+0x16E`
//! flags `0`, prim word `+0x04` `0` in the Tetsu tutorial's solo capture).
//! Every kernel in `legaia_engine_vm::cast_module_ticks` / `cast_arm_ticks` /
//! `cast_seru_ticks_*` is ported against that pool: its row sweeps run over
//! seats `3..7`, its "`+0x1DD < 3` is the party" tests and its `3 + ctx[+1]`
//! arithmetic all assume the fixed base.
//!
//! The engine compacts its monster row to `party_count + k`. So at the
//! [`World::run_cast_module_code`] seam the engine's actor table is lifted
//! into a retail-shaped row - party in `0..party_count`, zeroed empty seats
//! up to `3`, the engine's monster row from `3` on - and the kernels' writes,
//! hit seats and target codes are mapped back. For a party of three the map
//! is the identity, so a full party runs exactly as it did before the seam.

use super::*;

/// The engine-slot <-> retail-pool-slot map for one battle.
///
/// Engine slot `e < pc` is retail slot `e`; engine slot `e >= pc` is retail
/// slot `e + (3 - pc)`. Retail slots `pc..3` are a small party's empty seats
/// and map to no engine slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::world) struct CastSeatMap {
    /// The engine's party count, clamped to `1..=3`.
    pc: u8,
    /// The engine's actor-table length.
    engine_len: usize,
}

/// Retail's first monster seat, the fixed base the kernels assume.
const FIRST_MONSTER: u8 = vm::cast_module_ticks::FIRST_MONSTER_SEAT;

/// Retail's seats `3..8` are the five monster seats.
const MONSTER_SEATS: u8 = 5;

impl CastSeatMap {
    pub(in crate::world) fn new(party_count: u8, engine_len: usize) -> Self {
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
    pub(in crate::world) fn retail_len(self) -> usize {
        self.engine_len + usize::from(self.gap())
    }

    /// The retail seat engine slot `engine` occupies.
    pub(in crate::world) fn retail(self, engine: u8) -> u8 {
        if engine < self.pc {
            engine
        } else {
            engine.saturating_add(self.gap())
        }
    }

    /// The engine slot behind retail seat `retail`, or `None` for an empty
    /// party seat.
    pub(in crate::world) fn engine(self, retail: u8) -> Option<u8> {
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
    pub(in crate::world) fn target_to_retail(self, code: u8) -> u8 {
        if code >= self.pc && code < self.pc + MONSTER_SEATS {
            code + self.gap()
        } else {
            code
        }
    }

    /// The inverse of [`Self::target_to_retail`] over the codes it produces.
    /// An empty party seat has no engine slot; it is kept as retail named it
    /// (it is below `3`, so it still reads as "the party row").
    pub(in crate::world) fn target_to_engine(self, code: u8) -> u8 {
        if (FIRST_MONSTER..FIRST_MONSTER + MONSTER_SEATS).contains(&code) {
            code - self.gap()
        } else {
            code
        }
    }
}

impl World {
    /// This battle's [`CastSeatMap`].
    pub(in crate::world) fn cast_seat_map(&self) -> CastSeatMap {
        CastSeatMap::new(self.party.party_count, self.actors.len())
    }

    /// [`World::cast_actor_state`] with its `+0x1DD` target code in retail's
    /// seat space - the view a kernel reads.
    pub(in crate::world) fn cast_view(&self, slot: u8) -> vm::cast_module_ticks::CastActorState {
        let map = self.cast_seat_map();
        let mut st = self.cast_actor_state(slot);
        st.target_code = map.target_to_retail(st.target_code);
        st
    }

    /// Write a kernel's view back onto engine slot `slot`, mapping its target
    /// code into the engine's space. A code the kernel left as it was keeps
    /// the engine's own value, so a code with no exact inverse round-trips.
    pub(in crate::world) fn write_cast_view(
        &mut self,
        slot: u8,
        st: &vm::cast_module_ticks::CastActorState,
    ) {
        let map = self.cast_seat_map();
        let live = self
            .actors
            .get(slot as usize)
            .map_or(0, |a| a.battle.active_target);
        let mut st = *st;
        st.target_code = if st.target_code == map.target_to_retail(live) {
            live
        } else {
            map.target_to_engine(st.target_code)
        };
        self.write_cast_actor_state(slot, &st);
    }

    /// The whole actor table as a retail-shaped seat row: an empty party seat
    /// is the zeroed struct retail leaves there.
    pub(in crate::world) fn cast_seat_row(&self) -> Vec<vm::cast_module_ticks::CastActorState> {
        let map = self.cast_seat_map();
        (0..map.retail_len())
            .map(|r| {
                map.engine(r as u8)
                    .map_or_else(Default::default, |e| self.cast_view(e))
            })
            .collect()
    }

    /// Write a [`Self::cast_seat_row`] back; the empty seats' writes are
    /// dropped (retail's land in a struct nothing reads).
    pub(in crate::world) fn write_cast_seat_row(
        &mut self,
        seats: &[vm::cast_module_ticks::CastActorState],
    ) {
        let map = self.cast_seat_map();
        for (r, st) in seats.iter().enumerate() {
            if let Some(e) = map.engine(r as u8) {
                self.write_cast_view(e, st);
            }
        }
    }

    /// [`Self::cast_seat_row`] for the extra record fields
    /// ([`World::cast_arm_ext_state`]).
    pub(in crate::world) fn cast_arm_ext_row(&self) -> Vec<vm::cast_arm_ticks::CastArmExtState> {
        let map = self.cast_seat_map();
        (0..map.retail_len())
            .map(|r| {
                map.engine(r as u8)
                    .map_or_else(Default::default, |e| self.cast_arm_ext_state(e))
            })
            .collect()
    }

    /// [`Self::write_cast_seat_row`] for the extra record fields.
    pub(in crate::world) fn write_cast_arm_ext_row(
        &mut self,
        exts: &[vm::cast_arm_ticks::CastArmExtState],
    ) {
        let map = self.cast_seat_map();
        for (r, st) in exts.iter().enumerate() {
            if let Some(e) = map.engine(r as u8) {
                self.write_cast_arm_ext_state(e, st);
            }
        }
    }

    /// A kernel's hit list, its seats mapped back to engine slots (a hit on
    /// an empty party seat is dropped).
    pub(in crate::world) fn cast_hits_to_engine(
        &self,
        hits: impl IntoIterator<Item = vm::cast_module_ticks::AoeHit>,
    ) -> Vec<vm::cast_module_ticks::AoeHit> {
        let map = self.cast_seat_map();
        hits.into_iter()
            .filter_map(|h| {
                map.engine(h.seat)
                    .map(|seat| vm::cast_module_ticks::AoeHit { seat, ..h })
            })
            .collect()
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
