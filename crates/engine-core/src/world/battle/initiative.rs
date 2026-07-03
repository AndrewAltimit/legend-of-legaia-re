//! Turn order: opponent scan, round-robin fallback, and SPD-seeded initiative
//! keys. Split out of `battle.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

impl World {
    /// First living actor on the side opposing `attacker`. Party slots
    /// (`< party_count`) oppose the monster band (`party_count..`); monster
    /// slots oppose the party. `None` if that side is wiped.
    pub(in crate::world) fn first_living_opponent_of(&self, attacker: u8) -> Option<u8> {
        let pc = self.party_count.max(1);
        let n = self.actors.len() as u8;
        let (lo, hi) = if attacker < pc { (pc, n) } else { (0, pc) };
        (lo..hi).find(|&i| {
            self.actors
                .get(i as usize)
                .is_some_and(|a| a.battle.liveness != 0)
        })
    }

    /// Next living combatant after `after` in round-robin slot order across
    /// the whole actor table (party then monsters, wrapping). Drives the live
    /// loop's turn cycling so monsters take turns interleaved with the party.
    /// `None` only when no actor is alive.
    pub(in crate::world) fn next_living_combatant(&self, after: u8) -> Option<u8> {
        let n = self.actors.len();
        if n == 0 {
            return None;
        }
        (1..=n).find_map(|step| {
            let idx = (after as usize + step) % n;
            (self.actors[idx].battle.liveness != 0).then_some(idx as u8)
        })
    }

    /// True when at least one living battle slot carries a non-zero SPD. Gates
    /// the SPD-seeded initiative turn order on real speed data; otherwise the
    /// battle stays on the round-robin [`Self::next_living_combatant`].
    pub(in crate::world) fn any_battle_speed(&self) -> bool {
        (0..BATTLE_SLOTS).any(|i| {
            self.battle_speed[i] != 0 && self.actors.get(i).is_some_and(|a| a.battle.liveness != 0)
        })
    }

    /// Seed every living battle slot's initiative key from its SPD; dead slots
    /// get `0`. Per-actor formula `init_key = speed + rand()%(speed/2 + 1) + 1`
    /// (`overlay_0897_801e23ec`), so every living actor's key is `>= 1`.
    /// `true` while any *living* actor still holds an unspent initiative key.
    /// When this goes false the round is over and the next
    /// [`Self::next_combatant_by_initiative`] reseeds - the live loop uses this
    /// as its once-per-round boundary for status ticking. Dead actors' stale
    /// keys are ignored (only living actors count), so it agrees with the
    /// reseed condition inside the selector.
    pub(super) fn any_living_initiative_key(&self) -> bool {
        (0..BATTLE_SLOTS).any(|i| {
            self.actors
                .get(i)
                .is_some_and(|a| a.battle.liveness != 0 && a.battle.init_key != 0)
        })
    }

    fn reseed_initiative(&mut self) {
        for i in 0..BATTLE_SLOTS {
            let alive = self.actors.get(i).is_some_and(|a| a.battle.liveness != 0);
            if !alive {
                if let Some(a) = self.actors.get_mut(i) {
                    a.battle.init_key = 0;
                }
                continue;
            }
            let speed = self.battle_speed[i];
            let span = (speed / 2 + 1) as u32; // never 0
            let key = speed as u32 + (self.next_rng() % span) + 1;
            if let Some(a) = self.actors.get_mut(i) {
                a.battle.init_key = key.min(u16::MAX as u32) as u16;
            }
        }
    }

    /// Seed the battle's initiative keys at setup: every living actor gets a
    /// key, then slot 0's key is consumed so it leads round 1 and the selector
    /// orders the rest by initiative. No-op (keys left at `0`) when no SPD is
    /// present, leaving the battle on the round-robin fallback.
    pub(in crate::world) fn seed_battle_initiative(&mut self) {
        if !self.any_battle_speed() {
            return;
        }
        self.reseed_initiative();
        if let Some(a) = self.actors.get_mut(0) {
            a.battle.init_key = 0;
        }
    }

    /// Next combatant by SPD-seeded initiative - the port of
    /// `recompute_battle_order` (`FUN_801daba4`). Returns the living actor with
    /// the highest current initiative key (random tiebreak via `rand %
    /// tie_count`), consuming that actor's key so the next turn picks another.
    /// When every living actor's key is spent a new round is seeded. Dead
    /// actors' keys are zeroed (the function's first loop) so they can't be
    /// picked. Falls back to round-robin when no actor carries SPD.
    ///
    /// PORT: FUN_801DABA4
    pub(in crate::world) fn next_combatant_by_initiative(&mut self) -> Option<u8> {
        if !self.any_battle_speed() {
            return self.next_living_combatant(self.battle_ctx.active_actor);
        }
        // First loop: zero dead actors' keys so the max-pick skips them.
        for i in 0..BATTLE_SLOTS {
            if self.actors.get(i).is_some_and(|a| a.battle.liveness == 0)
                && let Some(a) = self.actors.get_mut(i)
            {
                a.battle.init_key = 0;
            }
        }
        // Round boundary: when no living actor still holds a key, reseed.
        let any_key = (0..BATTLE_SLOTS).any(|i| {
            self.actors
                .get(i)
                .is_some_and(|a| a.battle.liveness != 0 && a.battle.init_key != 0)
        });
        if !any_key {
            self.reseed_initiative();
        }
        // Highest key among living actors; ties collected in slot order.
        let mut best: u16 = 0;
        let mut ties: Vec<u8> = Vec::new();
        for i in 0..BATTLE_SLOTS {
            let Some(a) = self.actors.get(i) else {
                continue;
            };
            if a.battle.liveness == 0 {
                continue;
            }
            let key = a.battle.init_key;
            if key == 0 {
                continue;
            }
            if key > best {
                best = key;
                ties.clear();
                ties.push(i as u8);
            } else if key == best {
                ties.push(i as u8);
            }
        }
        if ties.is_empty() {
            return self.next_living_combatant(self.battle_ctx.active_actor);
        }
        let pick = ties[(self.next_rng() as usize) % ties.len()];
        if let Some(a) = self.actors.get_mut(pick as usize) {
            a.battle.init_key = 0; // consume this turn
        }
        Some(pick)
    }
}
