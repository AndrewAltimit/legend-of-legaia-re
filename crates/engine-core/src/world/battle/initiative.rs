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

    /// Seed every living battle slot's initiative key; dead slots get `0`.
    ///
    /// The per-actor arithmetic is the shared kernel
    /// [`vm::battle_formulas::seed_initiative`] - the port of `FUN_801DA780`'s
    /// scoring body. This used to inline a bare `speed + rand()%(speed/2+1) + 1`,
    /// which is the *aliased* `overlay_0897_801e23ec` reading of the seeder and
    /// drops three terms the battle-resident routine actually applies: the
    /// wounded-HP bonus, the Slow halving, and the `+0xF4` always-act-first /
    /// always-act-last ability arms. Everything now goes through the kernel so
    /// there is one implementation to be right.
    ///
    /// After the per-slot sweep the `ctx+0x290` side lockout
    /// ([`vm::battle_formulas::apply_side_lockout`]) zeroes the disadvantaged
    /// side's keys, so a back attack costs the party its whole first round and
    /// a pre-emptive strike costs the monsters theirs.
    ///
    /// PORT: FUN_801DA780 (the slot sweep + lockout; per-actor scoring in
    /// `battle_formulas::seed_initiative`)
    pub(in crate::world) fn reseed_initiative(&mut self) {
        use vm::battle_formulas::{InitiativeActor, initiative_roll_modulus, seed_initiative};
        let party_count = self.party_count as usize;
        for i in 0..BATTLE_SLOTS {
            let alive = self.actors.get(i).is_some_and(|a| a.battle.liveness != 0);
            if !alive {
                if let Some(a) = self.actors.get_mut(i) {
                    a.battle.init_key = 0;
                }
                continue;
            }
            let is_party = i < party_count;
            let actor = InitiativeActor {
                speed: self.battle_speed[i],
                hp: self.actors[i].battle.hp,
                max_hp: self.actors[i].battle.max_hp,
                is_party,
                // Retail reads the Slow status from `actor+0x16E == 0x1000`.
                // The engine's status model does not carry that word yet, so
                // no actor is ever flagged Slow here.
                slowed: false,
                // The `+0xF4` ability word only applies to party slots (retail
                // gates the whole arm on `slot < 3`); monsters pass 0.
                ability_bits: if is_party {
                    self.initiative_ability_bits(i)
                } else {
                    0
                },
            };
            let roll = (self.next_rng() % initiative_roll_modulus(actor.speed)) as u16;
            let key = seed_initiative(&actor, roll);
            if let Some(a) = self.actors.get_mut(i) {
                a.battle.init_key = key;
            }
        }
        // `ctx+0x290` side lockout - read the *unlatched* copy, as retail does.
        //
        // `battle_formulas::apply_side_lockout` splits the sides at the fixed
        // retail boundary (party `0..=2`, monsters `3..=6`), because retail
        // always reserves three party slots even for a one- or two-member
        // party. The engine **compacts** instead: `enter_battle` seats the
        // first monster at `party_count`, so slot 1 can be a monster. Applying
        // the fixed split here would lock out the wrong side for any party
        // smaller than three, so the side test is taken from `party_count`.
        // The kernel stays the retail-layout reference; this is the engine's
        // seating adapter, not a different rule.
        let lockout = self.battle_formation;
        let party_count = self.party_count as usize;
        for slot in 0..BATTLE_SLOTS {
            let locked = match lockout {
                vm::battle_formulas::FormationAdvantage::None => false,
                // Back attack: the monsters got the drop, the party sits out.
                vm::battle_formulas::FormationAdvantage::BackAttack => slot < party_count,
                // Pre-emptive strike: the party got the drop.
                vm::battle_formulas::FormationAdvantage::Preemptive => slot >= party_count,
            };
            if locked && let Some(a) = self.actors.get_mut(slot) {
                a.battle.init_key = 0;
            }
        }
    }

    /// The character record's `+0xF4` ability bitfield for a party battle slot -
    /// the word `FUN_801DA780` tests for the always-act-first / always-act-last
    /// passives ([`vm::battle_formulas::InitiativeAbility`]). Unresolvable slots
    /// carry no bits.
    fn initiative_ability_bits(&self, slot: usize) -> u32 {
        let Some(member) = self.roster.members.get(self.party_roster_slot(slot)) else {
            return 0;
        };
        let bits = member.ability_bits();
        u32::from_le_bytes([bits[0], bits[1], bits[2], bits[3]])
    }

    /// Seed the battle's initiative keys at setup: every living actor gets a
    /// key, then slot 0's key is consumed so it leads round 1 and the selector
    /// orders the rest by initiative. No-op (keys left at `0`) when no SPD is
    /// present, leaving the battle on the round-robin fallback.
    ///
    /// Slot 0's key is *not* consumed when the formation locked the party out
    /// (a back attack): its key is already `0` and the monsters open the
    /// battle, which is the whole point of the lockout.
    pub(in crate::world) fn seed_battle_initiative(&mut self) {
        if !self.any_battle_speed() {
            return;
        }
        self.reseed_initiative();
        if let Some(a) = self.actors.get_mut(0) {
            a.battle.init_key = 0;
        }
    }

    /// Roll this battle's formation advantage into
    /// [`Self::battle_formation`] (`ctx+0x290`), the port of `FUN_80051D84`'s
    /// caller side. Both sides' mean SPD is compared with a random spread and
    /// the winner still has to pass a rarity gate; see
    /// [`vm::battle_formulas::roll_formation_advantage`] for the arithmetic.
    ///
    /// The Guardian Ring / Sentinel-class `+0xF8` bits
    /// ([`vm::battle_formulas::FormationAbility`]) fold from the living party
    /// members, matching the escape roll's fold.
    ///
    /// **Partial**: retail's map-gated scripted ambush arm (monster ids
    /// `0x3D..=0x3F` on maps `0x0C` / `0x15`) is passed `map_id: 0` because the
    /// engine has no numeric map-id space at this layer, so that arm never
    /// fires. The unconditional `monster_id == 0xA7` ambush arm does.
    ///
    /// PORT: FUN_80051D84 (the caller side; arithmetic in
    /// `battle_formulas::roll_formation_advantage`)
    pub(in crate::world) fn roll_battle_formation(
        &mut self,
        formation: &crate::monster_catalog::FormationDef,
    ) {
        use vm::battle_formulas::{FormationInputs, roll_formation_advantage};
        let party_n = (self.party_count as usize).min(self.actors.len());
        let party_spd: Vec<u16> = (0..party_n)
            .filter(|&i| self.actors[i].battle.liveness != 0)
            .map(|i| self.battle_speed.get(i).copied().unwrap_or(0))
            .collect();
        let enemy_spd: Vec<u16> = (party_n..self.actors.len())
            .filter(|&i| self.actors[i].battle.liveness != 0)
            .map(|i| self.battle_speed.get(i).copied().unwrap_or(0))
            .collect();
        let mut ability_bits = 0u32;
        for slot in 0..party_n {
            if self.actors[slot].battle.liveness == 0 {
                continue;
            }
            if let Some(member) = self.roster.members.get(self.party_roster_slot(slot)) {
                let b = member.ability_bits();
                ability_bits |= u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
            }
        }
        let inputs = FormationInputs {
            ability_bits,
            monster_id: formation
                .slots
                .first()
                .map(|s| s.monster_id as u8)
                .unwrap_or(0),
            map_id: 0,
        };
        // The score inputs are owned locals, so the RNG closure can hold the
        // only borrow of `self` - draws stay on the shared determinism stream.
        let mut rand = || self.next_rng();
        self.battle_formation =
            roll_formation_advantage(&party_spd, &enemy_spd, &inputs, &mut rand);
    }

    /// Latch the formation advantage, the port of `FUN_801E295C` state `0x00`:
    ///
    /// ```text
    /// 801e2b30  lbu v0,0x290(v1)
    /// 801e2b38  sb  v0,0x291(v1)
    /// 801e2b48  sb  zero,0x290(v0)
    /// ```
    ///
    /// Runs once, after [`Self::seed_battle_initiative`] - the seeder is the
    /// only reader of the unlatched `+0x290`, so latching before it would
    /// silently disable the side lockout, and never latching at all silently
    /// disables pre-emptive-strike escapes ([`Self::roll_battle_escape`]).
    ///
    /// PORT: FUN_801E295C (state 0x00, the `+0x290` -> `+0x291` latch)
    pub(in crate::world) fn latch_battle_formation(&mut self) {
        self.battle_formation_latched = self.battle_formation;
        self.battle_formation = vm::battle_formulas::FormationAdvantage::None;
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
