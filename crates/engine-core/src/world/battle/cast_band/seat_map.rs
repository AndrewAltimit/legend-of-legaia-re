//! The seat array the slot-B cast-module kernels walk, laid out the way
//! retail lays it out.
//!
//! Retail's battle loader seats party member `i` at pool slot `i` and monster
//! `k` at `3 + k` whatever the party size (`FUN_800513F0`, the monster loop's
//! `addiu s0,s2,0x3` at `0x8005185C`); a party of one or two leaves the
//! unused party slots as zeroed actor structs (`+0x14C` HP `0`, `+0x16E`
//! flags `0`, prim word `+0x04` `0` in the Tetsu tutorial's solo capture).
//! Every kernel in `legaia_engine_cast::cast_module_ticks` / `cast_arm_ticks` /
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
use legaia_engine_battle_loop::cast_seat_map::*;

impl World {
    /// This battle's [`CastSeatMap`].
    pub(in crate::world) fn cast_seat_map(&self) -> CastSeatMap {
        CastSeatMap::new(self.party.party_count, self.actors.len())
    }

    /// [`World::cast_actor_state`] with its `+0x1DD` target code in retail's
    /// seat space - the view a kernel reads.
    pub(in crate::world) fn cast_view(
        &self,
        slot: u8,
    ) -> legaia_engine_cast::cast_module_ticks::CastActorState {
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
        st: &legaia_engine_cast::cast_module_ticks::CastActorState,
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
    pub(in crate::world) fn cast_seat_row(
        &self,
    ) -> Vec<legaia_engine_cast::cast_module_ticks::CastActorState> {
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
        seats: &[legaia_engine_cast::cast_module_ticks::CastActorState],
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
    pub(in crate::world) fn cast_arm_ext_row(
        &self,
    ) -> Vec<legaia_engine_cast::cast_arm_ticks::CastArmExtState> {
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
        exts: &[legaia_engine_cast::cast_arm_ticks::CastArmExtState],
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
        hits: impl IntoIterator<Item = legaia_engine_cast::cast_module_ticks::AoeHit>,
    ) -> Vec<legaia_engine_cast::cast_module_ticks::AoeHit> {
        let map = self.cast_seat_map();
        hits.into_iter()
            .filter_map(|h| {
                map.engine(h.seat)
                    .map(|seat| legaia_engine_cast::cast_module_ticks::AoeHit { seat, ..h })
            })
            .collect()
    }
}
