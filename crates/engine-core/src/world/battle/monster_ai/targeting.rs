//! Monster targeting: the cast target class, class-to-slot resolution and the
//! living-party picks.

use super::*;

impl World {
    /// Target **class** the generic core picks for a monster casting `def`, by
    /// the spell's [`legaia_engine_battle::spells::SpellTarget`] shape (monster's perspective:
    /// enemies = party band, allies = monster band). Single-enemy → a random
    /// living party slot; `AllEnemies` → class `8`; `AllAllies` → class `9`;
    /// `OneAlly` → the most-weakened living ally (or self); `SelfOnly` → self.
    pub(super) fn monster_cast_target_class(
        &mut self,
        slot: u8,
        def: &legaia_engine_battle::spells::SpellDef,
    ) -> u8 {
        use legaia_engine_battle::spells::SpellTarget;
        let pc = self.party.party_count.max(1);
        let n = self.actors.len() as u8;
        match def.target {
            SpellTarget::OneEnemy => self.random_living_party_member(pc).unwrap_or(slot),
            SpellTarget::AllEnemies => 8,
            SpellTarget::AllAllies => 9,
            SpellTarget::SelfOnly => slot,
            SpellTarget::OneAlly => {
                let mut best: Option<(u8, u16)> = None;
                for i in pc..n {
                    if let Some(a) = self.actors.get(i as usize)
                        && a.battle.liveness != 0
                        && a.battle.hp < a.battle.max_hp / 2
                        && best.is_none_or(|(_, hp)| a.battle.hp < hp)
                    {
                        best = Some((i, a.battle.hp));
                    }
                }
                best.map(|(i, _)| i).unwrap_or(slot)
            }
        }
    }

    /// Resolve an absolute target list from a `+0x1DD` target class: `8` = all
    /// living party, `9` = all living monsters, `< party_count` = that single
    /// party slot, otherwise that single monster/self slot.
    pub(super) fn resolve_class_to_slots(&self, slot: u8, class: u8) -> Vec<u8> {
        let pc = self.party.party_count.max(1);
        let n = self.actors.len() as u8;
        let alive = |i: u8| {
            self.actors
                .get(i as usize)
                .is_some_and(|a| a.battle.liveness != 0)
        };
        let _ = slot;
        match class {
            8 => (0..pc).filter(|&i| alive(i)).collect(),
            9 => (pc..n).filter(|&i| alive(i)).collect(),
            t if t < n => vec![t],
            // Out-of-range class: no targets (the caller falls back to physical).
            _ => Vec::new(),
        }
    }

    /// Pick a random living party member (`rand % party_count`, re-rolled until
    /// it lands on a living slot), mirroring the party-target roll shared by
    /// `FUN_801E9FD4` and `FUN_801E7320`. `None` only when the whole party is
    /// down. The deterministic LCG cycles every value, so the re-roll loop
    /// always terminates once one member is alive.
    pub(in crate::world) fn random_living_party_member(&mut self, party_count: u8) -> Option<u8> {
        let pc = party_count.max(1);
        let any_alive = (0..pc).any(|i| {
            self.actors
                .get(i as usize)
                .is_some_and(|a| a.battle.liveness != 0)
        });
        if !any_alive {
            return None;
        }
        loop {
            let t = (self.next_rand() % pc as u32) as u8;
            if self
                .actors
                .get(t as usize)
                .is_some_and(|a| a.battle.liveness != 0)
            {
                return Some(t);
            }
        }
    }

    /// Lowest-HP living party member (slot `0..party_count`), ties broken by
    /// the lower slot index. `None` only when the whole party is down.
    /// Consumes no RNG - used solely by the opt-in
    /// [`crate::world::WorldToggles::smarter_monster_targeting`] override, which runs after the
    /// faithful random pick has already advanced the RNG stream.
    pub(super) fn lowest_hp_living_party_member(&self, party_count: u8) -> Option<u8> {
        let pc = party_count.max(1);
        let mut best: Option<(u8, u16)> = None;
        for i in 0..pc {
            if let Some(a) = self.actors.get(i as usize)
                && a.battle.liveness != 0
                && best.is_none_or(|(_, hp)| a.battle.hp < hp)
            {
                best = Some((i, a.battle.hp));
            }
        }
        best.map(|(i, _)| i)
    }

    /// Port of `FUN_801E7320` - the monster-AI **target resolver**,
    /// invoked by the battle SM (`FUN_801E295C`) at `ActionSeed` as the
    /// `monster_setup` hook for monster actors whose `field_flags & 0x380` is
    /// set. It reads the targeting-class byte the action picker left in
    /// `actor.active_target` (`+0x1DD`) and expands it into a concrete target,
    /// re-rolling the deterministic RNG until it lands on a living actor on the
    /// matching side:
    ///
    /// - **class `0..2`** → a living **monster** slot (`rand % ctx[+1] + 3`,
    ///   `0x801E73B8`); if it lands on self, clears `action_category` and
    ///   keeps self as the target. The `3` is a constant, not the party count:
    ///   retail's monster band always opens at seat 3. The engine seats its
    ///   monsters from `party_count` instead, so the same band origin is
    ///   `party_count` here, and `ctx[+1]` is the seated monster count - not
    ///   the actor table's length, whose tail slots would only burn re-rolls.
    /// - **class `3..6`** → a living **party** slot (`rand % party_count`).
    /// - **class `8`** → 1-in-3 keeps the all-target code `9`, else self.
    /// - **class `7` / other** → 1-in-3 sets the all-target code `8`, else self.
    ///
    /// Retail ctx fields: `ctx[+0]` = party count, `ctx[+1]` = monster count,
    /// `ctx[+0x13]` = active slot - here read from `party_count` / the actor
    /// table / `slot`. See `ghidra/scripts/funcs/overlay_battle_action_801e7320.txt`.
    ///
    /// Retail invokes this from the SM when the actor's `+0x16E` status word has
    /// `field_flags & 0x380` set (the confuse-class statuses). The engine doesn't
    /// model that bitfield, so [`Self::maybe_confuse_retarget`] bridges directly
    /// from [`StatusKind::Confuse`] and calls this on the monster physical-strike
    /// path. (Side detection assumes the retail 3-slot party layout - correct for
    /// a full party; a reduced party + confused monster is a pre-existing edge.)
    ///
    /// PORT: FUN_801E7320
    /// REF: FUN_801E295C
    pub(in crate::world) fn resolve_monster_target(&mut self, slot: u8) {
        let pc = self.party.party_count.max(1);
        // `ctx[+1]`: the seats battle load filled on the monster side.
        let mc = self
            .actors
            .iter()
            .skip(usize::from(pc))
            .take_while(|a| a.battle.max_hp > 0 || a.battle_monster_id.is_some())
            .count()
            .max(1) as u8;
        let class = match self.actors.get(slot as usize) {
            Some(a) => a.battle.active_target,
            None => return,
        };
        let set_target = |w: &mut Self, t: u8| {
            if let Some(a) = w.actors.get_mut(slot as usize) {
                a.battle.active_target = t;
            }
        };
        let clear_category_self = |w: &mut Self| {
            if let Some(a) = w.actors.get_mut(slot as usize) {
                a.battle.action_category = 0;
                a.battle.active_target = slot;
            }
        };
        if class < 3 {
            // Target a living monster (the caster's own band).
            loop {
                let t = (self.next_rand() % mc as u32) as u8 + pc;
                set_target(self, t);
                if self
                    .actors
                    .get(t as usize)
                    .is_some_and(|a| a.battle.liveness != 0)
                {
                    if t == slot {
                        clear_category_self(self);
                    }
                    return;
                }
            }
        } else if class < 7 {
            // Target a living party member.
            loop {
                let t = (self.next_rand() % pc as u32) as u8;
                set_target(self, t);
                if self
                    .actors
                    .get(t as usize)
                    .is_some_and(|a| a.battle.liveness != 0)
                {
                    return;
                }
            }
        } else if class == 8 {
            if self.next_rand().is_multiple_of(3) {
                set_target(self, 9);
            } else {
                clear_category_self(self);
            }
        } else if self.next_rand().is_multiple_of(3) {
            set_target(self, 8);
        } else {
            clear_category_self(self);
        }
    }
}
