//! The monster's action pick: its cast definition lookup, the weighted pick and
//! the battle-mode word.

use super::*;

impl World {
    /// The cast record a monster's turn arms for `spell_id`: the catalog's,
    /// or - for a capture-class special, which the catalog does not carry -
    /// one built off the disc spell table
    /// ([`legaia_engine_battle::retail_magic::capture_special_def`]). The action SM routes
    /// the latter to its capture band on the table's class byte, as retail's
    /// does; without it the pick degraded to a strike.
    pub(in crate::world) fn monster_cast_def(
        &self,
        spell_id: u8,
    ) -> Option<legaia_engine_battle::spells::SpellDef> {
        self.tables
            .spell_catalog
            .get(spell_id)
            .cloned()
            .or_else(|| {
                let table = self.menu.text.as_ref()?.spell_names.as_ref()?;
                legaia_engine_battle::retail_magic::capture_special_def(table, spell_id)
            })
    }

    /// Monster-AI action picker - port of the **generic decision
    /// core** of `FUN_801E9FD4` (`overlay_battle_action_801e9fd4.txt`), the
    /// routine retail runs (from `recompute_battle_order` / `FUN_801DABA4`) to
    /// choose each monster's action.
    ///
    /// Faithful to the core: it rolls `rand % (1 + live_magic_count)` over the
    /// monster's own global magic-attack ids (record `+0x21..=+0x23`, carried on
    /// [`legaia_engine_battle::monster_catalog::MonsterDef::magic_attacks`]); a roll of `0`
    /// picks a **physical** strike (target `rand % party_count`), otherwise it
    /// picks magic id `magic[roll-1]` and resolves the target by the spell's
    /// shape byte (`spell_table[id*0xC + 2] & 0x60`), modelled here through the
    /// catalog's [`legaia_engine_battle::spells::SpellTarget`]: `OneEnemy` → a random living
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
    /// switch is ported in [`legaia_engine_battle::monster_ai`] ([`legaia_engine_battle::monster_ai::decide`])
    /// and consulted here as an override, followed by the post-switch
    /// recent-target ring ([`legaia_engine_battle::monster_ai::apply_recent_target_ring`]). The
    /// companion target resolver `FUN_801E7320` is ported as
    /// [`Self::resolve_monster_target`] (the `monster_setup` hook).
    ///
    /// PORT: FUN_801E9FD4
    /// REF: FUN_801DABA4, FUN_801DA51C
    pub(in crate::world) fn pick_monster_action(&mut self, slot: u8) -> MonsterAction {
        let pc = self.party.party_count.max(1);

        // Retail-compare debug seed: a capture taken mid monster cast replays
        // that cast on the seat's next turn instead of the AI's pick.
        if let Some((seat, spell_id)) = self.battle.forced_monster_cast
            && seat == slot
            && let Some(def) = self.monster_cast_def(spell_id)
        {
            let class = self.monster_cast_target_class(slot, &def);
            let mut targets = self.resolve_class_to_slots(slot, class);
            if targets.len() == 1
                && let Some(t) = self.battle.forced_monster_target.take()
                && (t < pc || t == slot)
                && self.actors.get(t as usize).is_some_and(|a| a.battle.hp > 0)
            {
                targets = vec![t];
            }
            if !targets.is_empty() {
                if let Some(a) = self.actors.get_mut(slot as usize) {
                    a.battle.action_category = 2;
                    a.battle.params[0] = spell_id;
                    a.battle.mp = a.battle.mp.saturating_add(u16::from(def.mp_cost));
                }
                return MonsterAction::Cast { spell_id, targets };
            }
        }

        // --- generic decision core ---
        // The monster's own castable global magic ids (parser already drops the
        // empty `<= 1` slots, so every entry is "live").
        let magic: Vec<u8> = self
            .actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| self.tables.monster_catalog.get(id))
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
        let roll = self.next_rand() % denom;
        // Provisional choice (category 3 = physical strike, 2 = magic).
        let (mut category, mut spell_id) = (3u8, 0u8);
        let mut target_class;
        if roll != 0 {
            let id = magic[(roll - 1) as usize];
            if let Some(def) = self.monster_cast_def(id)
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
            // `ctx[+1]`, the seated monster count - not the actor table's
            // length, which would read every `monster_count != 4` / `!= 3`
            // scripted-cast gate as open.
            let seated = self.seated_monster_slots().count() as u8;
            let ctx = legaia_engine_battle::monster_ai::MonsterAiCtx {
                monster_id: (monster_id & 0xFF) as u8,
                monster_index: slot.saturating_sub(pc),
                caster_slot: slot,
                hp,
                max_hp,
                mp,
                party_count: pc,
                monster_count: seated.max(1),
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
                core_category: category,
            };
            let mut ai = std::mem::take(&mut self.battle.monster_ai_state);
            let mut spirit_writeback = None;
            if let Some(cast) =
                legaia_engine_battle::monster_ai::decide(&ctx, &mut ai, &mut || self.next_rand())
            {
                category = cast.category;
                spell_id = cast.spell_id;
                if !cast.keep_target {
                    target_class = cast.target_class;
                }
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
            target_class = legaia_engine_battle::monster_ai::apply_recent_target_ring(
                target_class,
                spell_id,
                pc,
                &mut ai,
                &mut || self.next_rand(),
            );
            self.battle.monster_ai_state = ai;
        }

        // --- the once-per-pass flee checkpoint (`FUN_801E9FD4` loop bottom,
        // `jal 0x801ec0dc` at 801ea980) ---
        // Retail attempts the enemy escape roll exactly once per picker pass,
        // after the current monster's pick (including the scripted switch) has
        // consumed its draws, and a success OVERRIDES the picked category with
        // 5 (`sb 5, 0x1de(s4)`). The roll runs first and draws its RNG either
        // way; the special-battle word is tested after it (`0x801EA994`), so
        // a granted flee is dropped in an arena leg or a Ra-Seru-forbidden
        // fight and the monster keeps its pick.
        if !self.battle.monster_flee_attempted {
            self.battle.monster_flee_attempted = true;
            if self.monster_flee_roll(slot) && self.special_battle_word() == 0 {
                if let Some(a) = self.actors.get_mut(slot as usize) {
                    a.battle.action_category = 5;
                }
                return MonsterAction::Flee;
            }
        }

        // Optional, gated, NON-FAITHFUL: smarter single-target selection. By
        // now every RNG draw of the faithful random pick (magic roll, target
        // roll + re-roll loop, scripted override, anti-repeat ring) is already
        // consumed, so overriding the chosen slot here does not move the RNG
        // stream. We only redirect a single living-party target (`class < pc`)
        // to the lowest-HP living member; all-party (8) / monster-band (9) /
        // self targets are left exactly as the faithful path resolved them.
        if self.toggles.smarter_monster_targeting
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
        self.battle.monster_ai_state.mode_flags
    }

    /// Advance the battle-mode counter by one - the faithful port of the
    /// battle-action SM's `case 0xFF` (`_DAT_8007BD24[0x28A] += 1`), the
    /// boss-phase-transition pseudo-action. A boss script issues action `0xFF`
    /// when the fight crosses a scripted phase boundary; the next monster turn's
    /// `Self::pick_monster_action` then reads the bumped mode through
    /// [`legaia_engine_battle::monster_ai::decide`], activating that phase's scripted casts.
    /// The retail counter is a byte, so it wraps at `0xFF`.
    ///
    /// PORT: FUN_801E295C
    pub fn advance_battle_mode(&mut self) {
        self.battle.monster_ai_state.mode_flags =
            self.battle.monster_ai_state.mode_flags.wrapping_add(1);
    }
}
