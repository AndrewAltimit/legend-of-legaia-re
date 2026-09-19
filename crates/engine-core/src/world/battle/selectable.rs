//! The **raw `+0x16E` selectable predicate** and the two pool scans that read
//! it.
//!
//! The port models a battle actor's ailments twice over, and until this module
//! the two never met. `BattleActor::field_flags` is the raw `+0x16E` word, and
//! the cast band writes it directly (a Seru debuff ORs its bit in); the status
//! tracker holds the same conditions as a typed
//! [`vm::status_effects::StatusInstance`] list, and the turn loop reads that.
//! Retail has one representation, so a scan that asks the raw word and a loop
//! that asks the typed list can disagree about the same actor.
//!
//! [`World::raw_status_word`] is the bridge, and it is a *composition* rather
//! than a choice: the typed list packs to its retail bits through
//! [`vm::status_effects::pack_display_flags`], whose bit map is pinned
//! bit-by-bit in `vm::status_effects::display_flags`, and the raw word is
//! OR-ed in unchanged. Nothing is dropped in either direction, so the result
//! is the word retail would be holding - including the `0x380` delegation
//! group, which is an equipment passive rather than an ailment and therefore
//! only ever appears on the raw side.
//!
//! The bit map itself is tabulated in
//! [`docs/subsystems/battle-action.md`](../../../../../docs/subsystems/battle-action.md).

use super::*;
use vm::battle_action::{PoolActor, first_selectable_target, next_selectable_actor};

impl World {
    /// Actor `slot`'s retail `+0x16E` status word.
    ///
    /// The raw field the cast band writes, OR the typed status list packed
    /// through [`vm::status_effects::pack_display_flags`]. Faint contributes
    /// no bit (retail has no KO flag - a dead actor is one whose `+0x14C` is
    /// zero), which is why every consumer pairs this with liveness.
    ///
    /// REF: FUN_8002C2E4 (the bit map, via `pack_display_flags`)
    pub(in crate::world) fn raw_status_word(&self, slot: u8) -> u16 {
        let raw = self
            .actors
            .get(usize::from(slot))
            .map(|a| a.battle.field_flags)
            .unwrap_or(0);
        raw | vm::status_effects::pack_display_flags(self.battle.status_effects.statuses(slot))
    }

    /// The party rows as the two pool scans read them: liveness halfword plus
    /// [`Self::raw_status_word`], one entry per commandable seat.
    fn command_pool(&self, party_count: u8) -> (Vec<PoolActor>, Vec<u8>) {
        let mut pool = Vec::with_capacity(usize::from(party_count));
        let mut ids = Vec::with_capacity(usize::from(party_count));
        for slot in 0..party_count {
            let (liveness, hp) = self
                .actors
                .get(usize::from(slot))
                .map(|a| (a.battle.liveness, a.battle.hp))
                .unwrap_or((0, 0));
            pool.push(PoolActor {
                // Retail's scan reads `+0x14C` alone; the port also refuses a
                // seat at zero HP, which retail reaches by the same halfword
                // (the two are one field there and two here).
                alive: liveness != 0 && hp != 0,
                status: self.raw_status_word(slot),
                ..PoolActor::default()
            });
            // `DAT_8007BD10` holds a **1-based** char id per *battle seat*,
            // and the scan skips id `4` - the AI-companion seat. The port's
            // player party is three seats, so its ids are `1..=3` and that
            // term never fires: an engine seat that acts on its own carries
            // the `0x380` delegation bits instead, which the status term
            // above already refuses. Deriving the id from the roster slot
            // would be the wrong thing - the port lets a story guest occupy
            // a commandable seat, and roster slot 3 would then read as the
            // companion.
            ids.push(slot.wrapping_add(1));
        }
        (pool, ids)
    }

    /// The next party member who still owes this round a command, scanning
    /// forward from `after` (or from slot `0`).
    ///
    /// The selectability half is retail's, verbatim: `FUN_801DBA04` from zero
    /// and `FUN_801DB81C` from `cursor + 1`, both refusing the AI-companion
    /// seat, a seat with no liveness and a seat whose status word carries
    /// `+0x16E & 0xF84` (Stone, the `0x380` delegation group, Numb, Sleep).
    /// The port supplies that word through [`Self::raw_status_word`], so the
    /// scan sees the same bits whether the condition arrived as a cast-band
    /// write or as a typed tracker entry.
    ///
    /// The **round-flow** half is the port's own and sits outside the kernel,
    /// because retail keeps it elsewhere: a member who has already committed
    /// this round is skipped by re-entering the scan from the slot it landed
    /// on. Retail's cursor walks on after each commit instead, so the two
    /// agree on every ordinary ring walk and differ only where the port
    /// reopens a ring mid-round.
    ///
    /// PORT: FUN_801DB81C
    /// PORT: FUN_801DBA04
    pub(in crate::world) fn next_member_owing_command(&self, after: Option<u8>) -> Option<u8> {
        let party_count = self.party.party_count.clamp(1, 3);
        let (pool, ids) = self.command_pool(party_count);
        let mut cursor = match after {
            None => first_selectable_target(&pool, &ids, party_count),
            Some(a) => next_selectable_actor(&pool, &ids, party_count, a),
        };
        // Both scans return `actor_count` when nothing qualifies.
        while cursor < party_count {
            if !self.battle.round_flow.committed(cursor) {
                return Some(cursor);
            }
            cursor = next_selectable_actor(&pool, &ids, party_count, cursor);
        }
        None
    }
}
