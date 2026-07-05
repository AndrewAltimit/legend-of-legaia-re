//! Monster-turn decisioning: the action picker, confuse retargeting, physical /
//! cast arming, and monster-side target-class resolution. Split out of
//! `battle.rs` as additional `impl World` blocks; no logic change from the
//! original inline definitions.

use super::*;

impl World {
    pub(in crate::world) fn take_monster_turn(&mut self, slot: u8) {
        use vm::battle_action::ActionState;

        self.battle_ctx.active_actor = slot;
        match self.pick_monster_action(slot) {
            // A silenced/petrified caster can't cast - fall back to a physical
            // strike (mirrors the affordability fallback below).
            MonsterAction::Cast { .. } if self.actor_blocked_from_magic(slot) => {
                self.arm_monster_physical(slot);
            }
            MonsterAction::Cast {
                spell_id,
                mut targets,
            } => {
                // A confused caster's spell lands on the opposite side.
                self.confuse_retarget_cast(slot, &mut targets);
                let def = self.spell_catalog.get(spell_id).cloned();
                if let Some(def) = def
                    && self.cast_spell_on_slots(slot, &def, &targets)
                {
                    self.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
                    return;
                }
                // Cast didn't fold (no catalog entry / unaffordable after the
                // pick) - fall through to a physical strike.
                self.arm_monster_physical(slot);
            }
            MonsterAction::Physical { target } => {
                // AGL-driven multi-action budget: how many swings this monster
                // lands this turn (single swing when it has no AGL / swing data).
                self.arm_monster_strike_budget(slot);
                self.battle_ctx.queued_action = 3;
                self.battle_ctx.action_state = ActionState::Begin.as_byte();
                if let Some(a) = self.actors.get_mut(slot as usize) {
                    a.battle.active_target = target;
                    a.battle.action_category = 3;
                }
                self.maybe_confuse_retarget(slot);
            }
        }
    }

    /// Confuse retarget: a confused actor "acts uncontrollably", so once its
    /// single-target action's target is picked, flip it to a random living
    /// member of the *opposite* side via the ported `FUN_801E7320` resolver
    /// ([`Self::resolve_monster_target`]). Consumes battle RNG (reroll-while-
    /// dead), matching retail's structure; no-op when `slot` isn't confused.
    ///
    /// Retail triggers the resolver off the actor's `+0x16E` status word
    /// (`field_flags & 0x380`); the engine bridges directly from
    /// [`StatusKind::Confuse`] instead, since the bit-set site is the still-open
    /// capture thread and the engine tracks status by kind. Wired for both the
    /// monster physical strike ([`Self::take_monster_turn`]) and the party
    /// physical strike ([`Self::arm_party_physical`], which the live loop routes
    /// a confused party member through instead of opening the command menu).
    /// Confused monster *casts* flip via [`Self::confuse_retarget_cast`] (the
    /// cast path resolves a targets `Vec` rather than `active_target`).
    pub(in crate::world) fn maybe_confuse_retarget(&mut self, slot: u8) {
        if self.actor_is_confused(slot) {
            self.resolve_monster_target(slot);
        }
    }

    /// Confuse retarget for a *cast*: a confused caster's spell lands on the
    /// opposite side (uncontrollably), mirroring the physical retarget. The
    /// engine's monster cast resolves targets into a `Vec` (not the single
    /// `active_target` byte `FUN_801E7320` rewrites), so this flips the whole
    /// list: a single-target cast picks one random living member of the opposite
    /// side (one RNG draw); an area cast hits every living member there. A
    /// self-only cast is left as-is, and the flip is skipped when the opposite
    /// side has no living member. No-op when `caster` isn't confused.
    ///
    /// Monster-only in practice: a confused party member never reaches a cast -
    /// it auto-flails physically (see [`Self::arm_party_physical`]).
    pub(in crate::world) fn confuse_retarget_cast(&mut self, caster: u8, targets: &mut Vec<u8>) {
        if !self.actor_is_confused(caster) || targets.is_empty() {
            return;
        }
        // A self-only cast (e.g. a self-buff) isn't a side-flip target.
        if targets.len() == 1 && targets[0] == caster {
            return;
        }
        let pc = self.party_count.max(1);
        let n = self.actors.len() as u8;
        let opposite_is_monster = targets[0] < pc;
        let opp = if opposite_is_monster { pc..n } else { 0..pc };
        let living: Vec<u8> = opp
            .filter(|&s| {
                self.actors
                    .get(s as usize)
                    .is_some_and(|a| a.battle.liveness != 0)
            })
            .collect();
        if living.is_empty() {
            return;
        }
        if targets.len() == 1 {
            let pick = living[(self.next_rng() as usize) % living.len()];
            *targets = vec![pick];
        } else {
            *targets = living;
        }
    }

    /// True if `slot` carries the Confuse status.
    pub(in crate::world) fn actor_is_confused(&self, slot: u8) -> bool {
        self.status_effects
            .statuses(slot)
            .iter()
            .any(|s| s.kind == vm::status_effects::StatusKind::Confuse)
    }

    /// Arm a generic physical strike for party member `slot` against the first
    /// living opponent, then apply any [`Self::maybe_confuse_retarget`]. Shared
    /// by the non-player-driven party turn and the confused-party turn (a
    /// confused member can't be controlled, so it auto-acts and the retarget
    /// flips its strike to a random living ally). No-op retarget when the member
    /// isn't confused, so the auto-resolve path is RNG-unchanged.
    pub(in crate::world) fn arm_party_physical(&mut self, slot: u8) {
        use vm::battle_action::ActionState;
        let target = self.first_living_opponent_of(slot).unwrap_or(slot);
        self.battle_ctx.active_actor = slot;
        self.battle_ctx.queued_action = 3;
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
        if let Some(a) = self.actors.get_mut(slot as usize) {
            a.battle.active_target = target;
            a.battle.action_category = 3;
        }
        self.maybe_confuse_retarget(slot);
    }

    /// Arm a generic physical strike for monster `slot` against the first
    /// living party member (fallback when a picked cast can't fold).
    fn arm_monster_physical(&mut self, slot: u8) {
        use vm::battle_action::ActionState;
        self.arm_monster_strike_budget(slot);
        let target = self.first_living_opponent_of(slot).unwrap_or(slot);
        self.battle_ctx.queued_action = 3;
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
        if let Some(a) = self.actors.get_mut(slot as usize) {
            a.battle.active_target = target;
            a.battle.action_category = 3;
        }
        self.maybe_confuse_retarget(slot);
    }

    /// Compute + store the AGL-driven multi-action budget for the physical swing
    /// monster `slot` is about to make - the enemy analogue of the party Arts AP
    /// gauge. Clean-room port of the AGL-gauge spending loop in the picker
    /// `FUN_801E9FD4`: the monster gets one swing per action its per-round AGL
    /// gauge ([`crate::monster_catalog::MonsterDef::agl`]) can afford from its
    /// physical swing costs (`action_costs`), capped at 15, via
    /// [`vm::battle_action::enemy_action_budget`]. Draws battle RNG (one roll per
    /// candidate pick) exactly as retail's picker does.
    ///
    /// Falls back to a single swing - drawing **no** RNG - when the monster has
    /// no AGL gauge or no costed swing actions (the disc-free / synthetic
    /// catalog), so unbudgeted battles keep their RNG stream and behaviour
    /// bit-identical. The result is consumed by [`Self::apply_basic_attack`].
    ///
    /// PORT: FUN_801E9FD4
    pub(in crate::world) fn arm_monster_strike_budget(&mut self, slot: u8) {
        let (agl, costs) = self
            .actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| self.monster_catalog.get(id))
            .map(|d| (d.agl, d.action_costs.clone()))
            .unwrap_or((0, Vec::new()));
        self.monster_strike_budget = if agl > 0 && !costs.is_empty() {
            let stream =
                vm::battle_action::enemy_action_budget(agl, &costs, &mut || self.next_rng());
            (stream.len() as u8).max(1)
        } else {
            1
        };
    }

    /// Monster-AI action picker - clean-room port of the **generic decision
    /// core** of `FUN_801E9FD4` (`overlay_battle_action_801e9fd4.txt`), the
    /// routine retail runs (from `recompute_battle_order` / `FUN_801DABA4`) to
    /// choose each monster's action.
    ///
    /// Faithful to the core: it rolls `rand % (1 + live_magic_count)` over the
    /// monster's own global magic-attack ids (record `+0x21..=+0x23`, carried on
    /// [`crate::monster_catalog::MonsterDef::magic_attacks`]); a roll of `0`
    /// picks a **physical** strike (target `rand % party_count`), otherwise it
    /// picks magic id `magic[roll-1]` and resolves the target by the spell's
    /// shape byte (`spell_table[id*0xC + 2] & 0x60`), modelled here through the
    /// catalog's [`crate::spells::SpellTarget`]: `OneEnemy` → a random living
    /// party member, `AllEnemies` → the whole living party, `AllAllies` → the
    /// whole living monster band, `OneAlly` → the most-weakened living ally (or
    /// self), `SelfOnly` → self. A cast the monster can't afford from its live
    /// MP (`actor+0x150`) falls back to a physical strike, matching retail's
    /// affordability gate (`actor[0x150] < spell.mp_cost`).
    ///
    /// The large per-monster-id scripted-cast `switch` that follows the core in
    /// retail keys on `DAT_8007BD0C[slot]`, which `FUN_801DA51C` fills from the
    /// encounter record's `[+4 + slot]` monster ids - i.e. the **monster id**,
    /// not an abstract AI-type, so each case is bespoke AI for a specific
    /// monster the engine already identifies via `battle_monster_id`. That
    /// switch is ported in [`crate::monster_ai`] ([`crate::monster_ai::decide`])
    /// and consulted here as an override, followed by the post-switch
    /// recent-target ring ([`crate::monster_ai::apply_recent_target_ring`]). The
    /// companion target resolver `FUN_801E7320` is ported as
    /// [`Self::resolve_monster_target`] (the `monster_setup` hook).
    ///
    /// PORT: FUN_801E9FD4
    /// REF: FUN_801DABA4, FUN_801DA51C
    pub(in crate::world) fn pick_monster_action(&mut self, slot: u8) -> MonsterAction {
        let pc = self.party_count.max(1);

        // --- generic decision core ---
        // The monster's own castable global magic ids (parser already drops the
        // empty `<= 1` slots, so every entry is "live").
        let magic: Vec<u8> = self
            .actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| self.monster_catalog.get(id))
            .map(|d| d.magic_attacks.clone())
            .unwrap_or_default();
        let mp = self
            .actors
            .get(slot as usize)
            .map(|a| a.battle.mp)
            .unwrap_or(0);

        // Roll over (1 + live_magic_count); 0 => physical. Always consumes one
        // RNG draw, exactly like retail.
        let denom = 1 + magic.len() as u32;
        let roll = self.next_rng() % denom;
        // Provisional choice (category 3 = physical strike, 2 = magic).
        let (mut category, mut spell_id) = (3u8, 0u8);
        let mut target_class;
        if roll != 0 {
            let id = magic[(roll - 1) as usize];
            if let Some(def) = self.spell_catalog.get(id).cloned()
                && mp >= def.mp_cost as u16
            {
                category = 2;
                spell_id = id;
                target_class = self.monster_cast_target_class(slot, &def);
            } else {
                target_class = self.random_living_party_member(pc).unwrap_or(slot);
            }
        } else {
            target_class = self.random_living_party_member(pc).unwrap_or(slot);
        }

        // --- per-monster-id scripted override (the FUN_801E9FD4 switch) + the
        // post-switch recent-target anti-repeat ring. Run in a borrow window
        // with the AI state owned locally so the RNG closure can take `self`.
        if let Some(monster_id) = self
            .actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
        {
            let (hp, max_hp) = self
                .actors
                .get(slot as usize)
                .map(|a| (a.battle.hp, a.battle.max_hp))
                .unwrap_or((0, 0));
            let allies_with_mp = (0..pc)
                .filter(|&i| {
                    self.actors
                        .get(i as usize)
                        .is_some_and(|a| a.battle.liveness != 0 && a.battle.mp != 0)
                })
                .count() as u8;
            let n = self.actors.len() as u8;
            let ctx = crate::monster_ai::MonsterAiCtx {
                monster_id: (monster_id & 0xFF) as u8,
                monster_index: slot.saturating_sub(pc),
                caster_slot: slot,
                hp,
                max_hp,
                mp,
                party_count: pc,
                monster_count: n.saturating_sub(pc).max(1),
                field_flags: self
                    .actors
                    .get(slot as usize)
                    .map(|a| a.battle.field_flags)
                    .unwrap_or(0),
                allies_with_mp,
                spirit_gauge: self
                    .actors
                    .get(slot as usize)
                    .map(|a| a.battle.spirit_gauge)
                    .unwrap_or(0),
            };
            let mut ai = std::mem::take(&mut self.monster_ai_state);
            let mut spirit_writeback = None;
            if let Some(cast) = crate::monster_ai::decide(&ctx, &mut ai, &mut || self.next_rng()) {
                category = cast.category;
                spell_id = cast.spell_id;
                target_class = cast.target_class;
                spirit_writeback = cast.spirit_gauge_writeback;
            }
            // The 0x8A charge gate clamps the caster's own gauge as it fires
            // (`actor+0x170 = 0x32`). Applied after the RNG borrow window; it
            // draws no RNG, so the determinism stream is untouched.
            if let Some(g) = spirit_writeback
                && let Some(a) = self.actors.get_mut(slot as usize)
            {
                a.battle.spirit_gauge = g;
            }
            // Anti-repeat ring (applies to whichever single party target stands).
            target_class = crate::monster_ai::apply_recent_target_ring(
                target_class,
                spell_id,
                pc,
                &mut ai,
                &mut || self.next_rng(),
            );
            self.monster_ai_state = ai;
        }

        // Optional, gated, NON-FAITHFUL: smarter single-target selection. By
        // now every RNG draw of the faithful random pick (magic roll, target
        // roll + re-roll loop, scripted override, anti-repeat ring) is already
        // consumed, so overriding the chosen slot here does not move the RNG
        // stream. We only redirect a single living-party target (`class < pc`)
        // to the lowest-HP living member; all-party (8) / monster-band (9) /
        // self targets are left exactly as the faithful path resolved them.
        if self.smarter_monster_targeting
            && target_class < pc
            && let Some(low) = self.lowest_hp_living_party_member(pc)
        {
            target_class = low;
        }

        // --- build the action ---
        if category == 2 {
            let targets = self.resolve_class_to_slots(slot, target_class);
            if !targets.is_empty() {
                if let Some(a) = self.actors.get_mut(slot as usize) {
                    a.battle.action_category = 2;
                    a.battle.params[0] = spell_id;
                }
                return MonsterAction::Cast { spell_id, targets };
            }
        }
        // Physical strike (or a cast that resolved no targets).
        let target = if target_class < pc {
            target_class
        } else {
            self.random_living_party_member(pc)
                .or_else(|| self.first_living_opponent_of(slot))
                .unwrap_or(slot)
        };
        if let Some(a) = self.actors.get_mut(slot as usize) {
            a.battle.action_category = 3;
            a.battle.active_target = target;
        }
        MonsterAction::Physical { target }
    }

    /// The live battle-mode counter (`ctx+0x28A`, `_DAT_8007BD24[0x28A]`).
    ///
    /// This is the boss/scripted-mode gate the per-monster AI `switch` reads:
    /// multi-phase bosses (`0xA8`, `0xB4`, `0xB5`, `0xB6`, `0xA2..=0xA4`, …)
    /// change which spell they cast as it advances. `0` in a normal battle.
    pub fn battle_mode(&self) -> u8 {
        self.monster_ai_state.mode_flags
    }

    /// Advance the battle-mode counter by one - the faithful port of the
    /// battle-action SM's `case 0xFF` (`_DAT_8007BD24[0x28A] += 1`), the
    /// boss-phase-transition pseudo-action. A boss script issues action `0xFF`
    /// when the fight crosses a scripted phase boundary; the next monster turn's
    /// `Self::pick_monster_action` then reads the bumped mode through
    /// [`crate::monster_ai::decide`], activating that phase's scripted casts.
    /// The retail counter is a byte, so it wraps at `0xFF`.
    ///
    /// PORT: FUN_801E295C
    pub fn advance_battle_mode(&mut self) {
        self.monster_ai_state.mode_flags = self.monster_ai_state.mode_flags.wrapping_add(1);
    }

    /// Target **class** the generic core picks for a monster casting `def`, by
    /// the spell's [`crate::spells::SpellTarget`] shape (monster's perspective:
    /// enemies = party band, allies = monster band). Single-enemy → a random
    /// living party slot; `AllEnemies` → class `8`; `AllAllies` → class `9`;
    /// `OneAlly` → the most-weakened living ally (or self); `SelfOnly` → self.
    fn monster_cast_target_class(&mut self, slot: u8, def: &crate::spells::SpellDef) -> u8 {
        use crate::spells::SpellTarget;
        let pc = self.party_count.max(1);
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
    fn resolve_class_to_slots(&self, slot: u8, class: u8) -> Vec<u8> {
        let pc = self.party_count.max(1);
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
    fn random_living_party_member(&mut self, party_count: u8) -> Option<u8> {
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
            let t = (self.next_rng() % pc as u32) as u8;
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
    /// [`World::smarter_monster_targeting`] override, which runs after the
    /// faithful random pick has already advanced the RNG stream.
    fn lowest_hp_living_party_member(&self, party_count: u8) -> Option<u8> {
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

    /// Clean-room port of `FUN_801E7320` - the monster-AI **target resolver**,
    /// invoked by the battle SM (`FUN_801E295C`) at `ActionSeed` as the
    /// `monster_setup` hook for monster actors whose `field_flags & 0x380` is
    /// set. It reads the targeting-class byte the action picker left in
    /// `actor.active_target` (`+0x1DD`) and expands it into a concrete target,
    /// re-rolling the deterministic RNG until it lands on a living actor on the
    /// matching side:
    ///
    /// - **class `0..2`** → a living **monster** slot (`rand % monster_count +
    ///   party_count`); if it lands on self, clears `action_category` and keeps
    ///   self as the target.
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
        let pc = self.party_count.max(1);
        let mc = (self.actors.len() as u8).saturating_sub(pc).max(1);
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
                let t = (self.next_rng() % mc as u32) as u8 + pc;
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
                let t = (self.next_rng() % pc as u32) as u8;
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
            if self.next_rng().is_multiple_of(3) {
                set_target(self, 9);
            } else {
                clear_category_self(self);
            }
        } else if self.next_rng().is_multiple_of(3) {
            set_target(self, 8);
        } else {
            clear_category_self(self);
        }
    }
}
