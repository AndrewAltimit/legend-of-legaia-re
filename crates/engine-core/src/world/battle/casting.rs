//! Spell / summon / monster-special damage resolution and element affinity.
//! Split out of `battle.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

impl World {
    /// Deduct `def`'s MP cost from `caster` and fold its effect onto each
    /// absolute actor slot in `targets`. Shared by the player cast path
    /// ([`Self::apply_battle_spell`], which resolves the cursor rows to slots
    /// from the player's perspective) and the monster-AI cast path
    /// ([`Self::apply_monster_spell`], which resolves party slots). MP is spent
    /// once up front; each target folds through [`Self::fold_spell_outcome`].
    /// Returns `false` (no MP spent, nothing folded) when the caster can't
    /// afford the cost.
    pub(in crate::world) fn cast_spell_on_slots(
        &mut self,
        caster: u8,
        def: &crate::spells::SpellDef,
        targets: &[u8],
    ) -> bool {
        use crate::spells::{SpellSnapshot, cast_spell};

        // Apply the caster's MP-cost ability-bit modifier (Half 0x20 /
        // Quarter 0x10) so an MP-saver accessory reduces the live cast cost,
        // matching the state-machine cast path (battle_action.rs MagicCastBegin).
        // Only party members carry these accessory bits; monster casters
        // (caster >= party_count) pay full cost. Half takes priority over
        // Quarter when both bits are set, dump-confirmed against the retail
        // state-0x28 block (FUN_801E295C 0x801E3D0C) - routed through the shared
        // battle_formulas helper so all three cast paths agree.
        let ability_bits = if (caster as usize) < self.party_count as usize {
            self.character_ability_bits
                .get(caster as usize)
                .copied()
                .unwrap_or(0)
        } else {
            0
        };
        let base_cost = def.mp_cost as u16;
        let modifier = vm::battle_formulas::MpCostModifier::from_ability_flags(ability_bits);
        let cost = vm::battle_formulas::mp_cost_after_ability_bits(base_cost, modifier);
        let (caster_hp, caster_max_hp, caster_mp_before) = match self.actors.get(caster as usize) {
            Some(a) => (a.battle.hp, a.battle.max_hp, a.battle.mp),
            None => return false,
        };
        if caster_mp_before < cost {
            return false;
        }
        if let Some(a) = self.actors.get_mut(caster as usize) {
            a.battle.mp = a.battle.mp.saturating_sub(cost);
        }
        let caster_mag = self.battle_magic.get(caster as usize).copied().unwrap_or(0);

        // For a monster caster whose move id carries a real per-move power
        // record, the damage rolls through the faithful arts/physical kernel
        // seeded with that power instead of the MP-scaled placeholder
        // ([`Self::enemy_move_predamage`]). `None` keeps the placeholder path
        // (disc-free / synthetic battles never install the table).
        let move_power = self.enemy_move_power(caster, def.id);

        // A party member's Seru-magic cast trains the spell: each damaged
        // target contributes spell XP (the `FUN_801ddb30` accrual tail runs
        // once per finisher call, i.e. per hit - kernel
        // `battle_formulas::summon_spell_xp_gain`), summed here and banked
        // after the cast with the level-up check (`FUN_801e70bc`,
        // [`Self::accrue_summon_spell_xp`]). Retail keys on "attacker slot 7"
        // (the summon body); the engine's summon coverage is the
        // [`crate::summon::SERU_SUMMON_IDS`] block, so those are the ids that
        // accrue. `group_target` mirrors the summon's target byte (`+0x1DD`
        // = 8/9 for a group cast): the per-hit unit drops from 12 to 4.
        let is_party_summon_cast = (caster as usize) < self.party_count as usize
            && crate::summon::SERU_SUMMON_IDS.contains(&def.id);
        // Shiny bonus: when the casting character learned this Seru's spell
        // from a shiny capture, every cast deals +35% damage (on top of the
        // normal roll). Mirrors the retail `--shiny-seru` damage hook
        // (`FUN_801dd864` `+0x161` high-bit read). Computed once per cast.
        let shiny_cast = is_party_summon_cast
            && self
                .seru_log
                .is_shiny(self.party_roster_slot(caster as usize) as u8, def.id);
        let group_target = matches!(def.target, crate::spells::SpellTarget::AllEnemies);
        let mut summon_xp_gain: u32 = 0;

        for &t in targets {
            let Some(actor) = self.actors.get(t as usize) else {
                continue;
            };
            // Skip empty slots (no configured HP).
            if actor.battle.max_hp == 0 {
                continue;
            }
            let snap = SpellSnapshot {
                caster_mag,
                caster_hp,
                caster_max_hp,
                caster_mp: caster_mp_before,
                target_mdef: self.battle_defense.get(t as usize).copied().unwrap_or(0),
                target_hp: actor.battle.hp,
                target_hp_max: actor.battle.max_hp,
                target_mp: actor.battle.mp,
                target_alive: actor.battle.liveness != 0,
                target_weakness: crate::spells::ElementMask::default(),
            };
            let mut outcome = cast_spell(def, t, &snap);
            // Override only the magnitude of a damaging monster special-attack;
            // heals / buffs / non-table moves fall through unchanged. The
            // arts/physical kernel already folds the enemy→party affinity in.
            if let Some(power) = move_power
                && let crate::spells::SpellOutcome::Damage { amount, .. } = &mut outcome
                && let Some(faithful) = self.enemy_move_predamage(caster, t, power)
            {
                *amount = faithful;
            } else if let crate::spells::SpellOutcome::Damage { amount, .. } = &mut outcome {
                // Player Seru-magic path. When the monster catalog resolves
                // the summon creature (disc-real battles), the magnitude
                // rolls through the faithful summon branch of FUN_801dd0ac -
                // summon-body HP/AGL + caster AGL + affinity + the caster's
                // per-spell magic-power byte + the FUN_801ddb30 finisher
                // (per-caster summon power-percent, 9999 cap) - replacing
                // the MP-scaled placeholder entirely.
                if let Some(faithful) = self.player_summon_predamage(caster, t, def.id) {
                    *amount = faithful;
                } else {
                    // Fallback (creature unresolved): scale the placeholder
                    // by the element affinity of the summon creature vs. the
                    // target (FUN_801dd864). Post-roll, gated on the affinity
                    // tables - neutral (100) otherwise - so disc-free battles
                    // keep an unchanged magnitude and RNG stream.
                    let pct = self.cast_affinity_pct(def.id, t);
                    if pct != 100 {
                        *amount = ((*amount as u32 * pct as u32) / 100).min(9999) as u16;
                    }
                }
            }
            // Shiny Seru: +35% on the final magnitude (9999-capped), after the
            // affinity / summon roll so it stacks on the spell's normal output.
            if shiny_cast && let crate::spells::SpellOutcome::Damage { amount, .. } = &mut outcome {
                let boosted =
                    (*amount as u32 * (100 + crate::seru_learning::SHINY_DAMAGE_BONUS_PCT)) / 100;
                *amount = boosted.min(9999) as u16;
            }
            // Per-hit spell-XP gain (FUN_801ddb30 tail): the final damage
            // delta against the target's live/max HP at the moment of the hit
            // (the snapshot taken above, before the fold applies the damage).
            if is_party_summon_cast
                && let crate::spells::SpellOutcome::Damage { amount, .. } = &outcome
            {
                summon_xp_gain =
                    summon_xp_gain.saturating_add(vm::battle_formulas::summon_spell_xp_gain(
                        *amount as u32,
                        snap.target_hp,
                        snap.target_hp_max,
                        group_target,
                    ));
            }
            self.fold_spell_outcome(outcome);
        }
        // Bank the accrued XP and run the once-per-cast level-up check
        // (FUN_801e70bc fires at summon return, state 0x36).
        if is_party_summon_cast {
            self.accrue_summon_spell_xp(caster, def.id, summon_xp_gain);
        }
        // Cast band (live-loop path): seat the move's battle-FX at the first
        // real target's battle position so the host can render it.
        let fx_origin = targets
            .iter()
            .find_map(|&t| self.actors.get(t as usize))
            .map(|a| {
                [
                    a.move_state.world_x,
                    a.move_state.world_y,
                    a.move_state.world_z,
                ]
            })
            .unwrap_or([0, -300, -645]);
        if crate::summon::SERU_SUMMON_IDS.contains(&def.id) {
            // A player Seru-magic id resolves to a per-summon overlay: request
            // the summon-creature spawn (the retail cast band's `FUN_8003EC70`
            // overlay load - see `crate::summon`).
            self.request_summon_spawn(def.id, fx_origin);
        } else {
            // Every other move (enemy specials + non-summon spells) that carries
            // a move-power effect list requests its move-FX scene-graph spawn -
            // the `+0x12`/`+0x16` `0x801f6324` prototype records the retail
            // strike setup (`FUN_801e09f8`) spawns on the launch / on-contact
            // transitions. No-op when the move has no FX entries.
            self.request_move_fx_spawn(def.id, fx_origin);
        }
        true
    }

    /// Roll a monster special-attack's damage through the faithful
    /// arts/physical kernel ([`legaia_engine_vm::battle_formulas::arts_physical_predamage`],
    /// the non-summon branch of `FUN_801dd0ac`) seeded with the move's real
    /// per-move power (the move-power table's `+0`, `>> 2`). Returns the
    /// pre-finisher damage clamped to `1..=9999`, or `None` when the table
    /// isn't loaded or `move_id` has no power record - in which case the caller
    /// keeps the MP-scaled spell placeholder.
    ///
    /// Stat bridge (all read live off the actor arrays, faithful to the retail
    /// fields the kernel reads): attacker/target AGL = `battle_accuracy`
    /// (`+0x168`, the AGL-derived stat); HP = `battle.hp` (`+0x14c`); the two
    /// defender defense terms (`+0x15c`/`+0x160`) = the [`Self::battle_defense_split`]
    /// (UDF, LDF) pair, falling back to the single [`Self::battle_defense`].
    /// Element affinity comes from [`Self::enemy_affinity_pct`] when the
    /// affinity tables are installed (`matrix[enemy_element][party_element]`,
    /// `FUN_801dd864`), else 100 (neutral); status-weaken (`+0x16e`) and the
    /// guard byte (`+0x1de`) default to none. The affinity scale is applied
    /// *during* the roll (inside [`arts_physical_predamage_lazy`], before the
    /// conditional bonus-arm threshold, matching retail's scale→bonus order),
    /// so a non-neutral affinity can change whether the lazy bonus pair is
    /// drawn. What is invariant is the *gating*: an uninstalled table resolves
    /// to 100% (no scaling), reproducing the no-affinity baseline - magnitude
    /// and RNG stream bit-identical - so disc-free / synthetic battles are
    /// unperturbed.
    ///
    /// The `rand()` draws are taken in retail call order: attacker ×2, defender
    /// ×1, then the bonus pair ×2 **lazily** - drawn only when the conditional
    /// bonus arm fires ([`arts_physical_predamage_lazy`]). So the global RNG
    /// cursor advances by exactly three draws on the no-bonus path and five on
    /// the bonus path, matching `FUN_801dd0ac`'s call order.
    ///
    /// The roll then runs the closed-form finisher stages (`FUN_801ddb30`):
    /// the party defender's equipment elemental-resistance ladder (the
    /// elemental-guard / All-Guard accessory bits via [`Self::defender_resist`],
    /// tested against the attacking monster's record element `+0x1D`), the
    /// Spirit guard halve, the `rand()%9 + 8` no-damage floor (drawn lazily,
    /// exactly when mitigation zeroes the hit - retail's only finisher draw),
    /// and the 9999 cap.
    ///
    /// REF: FUN_801dd0ac (arts/physical branch)
    /// REF: FUN_801ddb30 (finisher stages on the enemy-special path)
    fn enemy_move_predamage(&mut self, attacker: u8, target: u8, power: i32) -> Option<u16> {
        use vm::battle_formulas::{
            DamageFinish, SummonRollActor, arts_physical_predamage_lazy, damage_finish_lazy,
        };

        let element_affinity_pct = self.enemy_affinity_pct(attacker, target);
        let a = self.actors.get(attacker as usize)?;
        let attacker_roll = SummonRollActor {
            hp: a.battle.hp,
            agl: self
                .battle_accuracy
                .get(attacker as usize)
                .copied()
                .unwrap_or(0),
            ..Default::default()
        };
        let target_roll = self.summon_roll_defender(target)?;
        // Retail rand() is 15-bit; mask to match its range. The attacker (×2) and
        // defender (×1) draws are always taken; the bonus pair is drawn lazily by
        // the closure below, only when the bonus arm fires, so the shared RNG
        // cursor advances exactly as retail's does (three or five draws).
        let rng3 = [
            (self.next_rng() & 0x7fff) as u16,
            (self.next_rng() & 0x7fff) as u16,
            (self.next_rng() & 0x7fff) as u16,
        ];
        let (atk, def) = arts_physical_predamage_lazy(
            power,
            &attacker_roll,
            &target_roll,
            element_affinity_pct,
            rng3,
            || {
                [
                    (self.next_rng() & 0x7fff) as u16,
                    (self.next_rng() & 0x7fff) as u16,
                ]
            },
        );
        // Closed-form finisher stages (FUN_801ddb30). The attacker element is
        // the monster record's `+0x1D` byte, read record-direct the way the
        // finisher's resist ladder does; unresolved (synthetic catalogs) it
        // falls back to 7 = non-elemental, which no resist bit matches, so
        // disc-free battles keep magnitude and RNG stream unchanged (a >= 1
        // roll never trips the floor draw without mitigation).
        let attacker_element = self
            .actors
            .get(attacker as usize)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| self.monster_catalog.get(id))
            .map(|d| d.element)
            .unwrap_or(7);
        let target_is_party = target < self.party_count;
        let finish = DamageFinish {
            predamage: atk.saturating_sub(def).clamp(1, 9999),
            attacker_slot: 3,
            defender_slot: if target_is_party { 0 } else { 3 },
            attacker_element,
            defender_resist: self.defender_resist(target),
            defender_guarding: self
                .battle_guarding
                .get(target as usize)
                .copied()
                .unwrap_or(false),
            enemy_defender_halve: false,
            bypass_party_resist: false,
            summon_power_pct: 100,
            floor_rand: 0,
        };
        let over = damage_finish_lazy(&finish, || (self.next_rng() & 0x7fff) as u16);
        Some(over.min(9999) as u16)
    }

    /// Build the defender-side [`SummonRollActor`] for an actor slot - the
    /// stat bridge shared by the monster special-attack roll
    /// ([`Self::enemy_move_predamage`]) and the player summon roll
    /// ([`Self::player_summon_predamage`]). AGL = `battle_accuracy` (the
    /// `+0x168` AGL-derived stat); HP = `battle.hp` (`+0x14c`); the two
    /// defense terms (`+0x15c`/`+0x160`) = the [`Self::battle_defense_split`]
    /// (UDF, LDF) pair, falling back to the single [`Self::battle_defense`];
    /// status-weaken (`+0x16e`) and the guard byte (`+0x1de`) default to none.
    fn summon_roll_defender(&self, slot: u8) -> Option<vm::battle_formulas::SummonRollActor> {
        let t = self.actors.get(slot as usize)?;
        let (stat_a, stat_b) = self
            .battle_defense_split
            .get(slot as usize)
            .copied()
            .flatten()
            .unwrap_or_else(|| {
                (
                    self.battle_defense.get(slot as usize).copied().unwrap_or(0),
                    0,
                )
            });
        Some(vm::battle_formulas::SummonRollActor {
            hp: t.battle.hp,
            agl: self
                .battle_accuracy
                .get(slot as usize)
                .copied()
                .unwrap_or(0),
            stat_a,
            stat_b,
            status: 0,
            guard: 0,
        })
    }

    /// The caster's per-spell magic-power byte, read the way the scale stage
    /// `FUN_801dd864` does: search the character record's 32-entry spell-id
    /// list (record `+0x13D`, live `0x80084845 + char*0x414`) for the cast
    /// spell id and take the parallel level byte (`+0x161`, live
    /// `0x80084869`). The retail loop bounds the search at `0x20` entries even
    /// though the record field holds 36; mirrored here. Returns `1` (the
    /// [`vm::battle_formulas::apply_magic_power`] identity) when the roster
    /// doesn't carry the spell - a fresh cast with no recorded level.
    pub(in crate::world) fn caster_magic_power_byte(&self, caster: u8, spell_id: u8) -> u8 {
        const RETAIL_SEARCH_BOUND: usize = 0x20;
        // Battle ordinal -> the occupying character's record.
        let Some(record) = self
            .roster
            .members
            .get(self.party_roster_slot(caster as usize))
        else {
            return 1;
        };
        let list = record.spell_list();
        list.ids
            .iter()
            .take(RETAIL_SEARCH_BOUND)
            .position(|&id| id == spell_id)
            .map(|i| list.levels[i])
            .unwrap_or(1)
    }

    /// Roll a player Seru-magic summon's damage through the faithful summon
    /// branch of the shared battle kernel (`FUN_801dd0ac` `attacker_slot ==
    /// 7`) plus the closed-form finisher stages (`FUN_801ddb30`), replacing
    /// the MP-scaled spell placeholder. Returns `None` when `spell_id` isn't
    /// a summon or the monster catalog doesn't resolve the summon creature -
    /// disc-free / synthetic battles keep the placeholder path with an
    /// unchanged RNG stream.
    ///
    /// Faithful seeds:
    /// - **Summon body** (the retail slot-7 actor): the namesake `battle_data`
    ///   creature's record HP (`+0x0C`) and AGL - the battle loader installs
    ///   the record stats on the freshly-spawned summon actor
    ///   ([`Self::summon_creature_def`]).
    /// - **Caster AGL** (`DAT_801C9370[ctx+0x13]` `+0x168`): the casting
    ///   party member's `battle_accuracy`, doubled into the roll.
    /// - **Scale stage** (`FUN_801dd864`): element affinity
    ///   `matrix[summon element][target element]`, then the caster's
    ///   per-spell magic-power byte ([`Self::caster_magic_power_byte`]).
    /// - **Finisher** (`FUN_801ddb30`): no-damage floor (`rand()%9 + 8`,
    ///   drawn lazily), the per-caster summon power-percent
    ///   (`0x801F5468[(char_id-1)*8 + summon_element]`,
    ///   [`legaia_asset::element_affinity::ElementAffinity::summon_power_pct`]),
    ///   and the 9999 cap. The party-resist / guard stages don't apply to an
    ///   enemy defender; the state-mutating tail (popup accumulator, MP
    ///   drain, stat debuffs) stays in the live fold.
    ///
    /// RNG: draws attacker + defender eagerly, the bonus-arm and floor draws
    /// lazily - the shared cursor advances exactly as retail's does (two,
    /// three, or four draws).
    ///
    /// PORT: FUN_801dd0ac (summon branch, live wiring; pure kernel in
    /// battle_formulas::summon_predamage_lazy)
    pub(in crate::world) fn player_summon_predamage(
        &mut self,
        caster: u8,
        target: u8,
        spell_id: u8,
    ) -> Option<u16> {
        use vm::battle_formulas::{
            DamageFinish, DefenderResist, SummonRollActor, damage_finish_lazy,
            summon_predamage_lazy,
        };

        if (caster as usize) >= self.party_count as usize {
            return None;
        }
        let creature = self.summon_creature_def(spell_id)?;
        let summon = SummonRollActor {
            hp: creature.hp,
            // The summon-roll's "agl" slot is the actor's `+0x168` stat, which
            // for a monster is record `+0x18` = INT (`MonsterDef::intel`).
            agl: creature.intel,
            ..Default::default()
        };
        let summon_element = creature.element;
        let caster_agl = self
            .battle_accuracy
            .get(caster as usize)
            .copied()
            .unwrap_or(0);
        let target_roll = self.summon_roll_defender(target)?;
        let element_affinity_pct = self.cast_affinity_pct(spell_id, target);
        let magic_power_byte = self.caster_magic_power_byte(caster, spell_id);

        let rng2 = [
            (self.next_rng() & 0x7fff) as u16,
            (self.next_rng() & 0x7fff) as u16,
        ];
        let (atk, def) = summon_predamage_lazy(
            &summon,
            caster_agl,
            &target_roll,
            element_affinity_pct,
            magic_power_byte,
            rng2,
            || (self.next_rng() & 0x7fff) as u16,
        );

        // Closed-form finisher stages (FUN_801ddb30). The defender is an
        // enemy (no party-resist stage); kernel slot convention is retail's
        // (party 0..2, enemies 3+). The summon power-percent comes from the
        // per-caster 0x801F5468 table when the affinity tables are installed
        // (char id = slot + 1, same model as the element lookups), else 100.
        let summon_power_pct = self
            .element_affinity
            .as_ref()
            .and_then(|aff| aff.summon_power_pct(caster + 1, summon_element))
            .unwrap_or(100);
        let predamage = atk.saturating_sub(def);
        let finish = DamageFinish {
            predamage,
            attacker_slot: 7,
            defender_slot: 3 + target.saturating_sub(self.party_count),
            attacker_element: summon_element,
            defender_resist: DefenderResist::default(),
            defender_guarding: false,
            enemy_defender_halve: false,
            bypass_party_resist: false,
            summon_power_pct,
            floor_rand: 0,
        };
        let over = damage_finish_lazy(&finish, || (self.next_rng() & 0x7fff) as u16);
        Some(over.min(9999) as u16)
    }

    /// Element-affinity multiplier (percent) for a monster (`attacker`) special
    /// attack landing on a party member (`target`): `matrix[enemy_element]
    /// [party_member_element]` from the parsed affinity tables
    /// ([`legaia_asset::element_affinity`], `FUN_801dd864`). Returns `100`
    /// (neutral, no change) when the affinity tables aren't installed or either
    /// element doesn't resolve, so disc-free / synthetic battles are unaffected.
    ///
    /// The attacker (enemy) element is its monster record's `+0x1D` byte
    /// (carried on [`crate::monster_catalog::MonsterDef::element`]); the defender
    /// (party member) element is the per-character table entry for the active
    /// party. The engine models the active party as `char_id == party slot`
    /// (0-based), so a party actor at slot `target` is the 1-based char id
    /// `target + 1` the affinity table indexes. Reading the enemy element off
    /// `MonsterDef::element` is faithful to the retail read: `FUN_801dd864`
    /// fetches it record-direct (`0x801C9348[slot-3]` → `+0x1d`), not from a
    /// copied live-actor field.
    ///
    /// PORT: FUN_801dd864 (element resolution + affinity-matrix lookup, the
    /// enemy→party direction; the status-weaken / guard-double / slot-7 summon
    /// stages of the full retail function are not part of this scalar)
    fn enemy_affinity_pct(&self, attacker: u8, target: u8) -> u8 {
        let Some(aff) = self.element_affinity.as_ref() else {
            return 100;
        };
        let Some(enemy_elem) = self
            .actors
            .get(attacker as usize)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| self.monster_catalog.get(id))
            .map(|d| d.element)
        else {
            return 100;
        };
        let Some(party_elem) = aff.character_element(target + 1) else {
            return 100;
        };
        aff.affinity_pct(enemy_elem, party_elem).unwrap_or(100)
    }

    /// Element id (`0..=7`) of a battle slot, resolved by slot the way the retail
    /// affinity stage [`FUN_801dd864`] does: a party member (slot `< party_count`)
    /// takes its per-character table element; any other slot - an enemy, or the
    /// slot-7 summon body - takes its monster record's `+0x1D` element
    /// ([`crate::monster_catalog::MonsterDef::element`]). Returns `None` when the
    /// affinity tables aren't installed or no element resolves, so callers fall
    /// back to neutral.
    fn battle_slot_element(&self, slot: u8) -> Option<u8> {
        let aff = self.element_affinity.as_ref()?;
        if (slot as usize) < self.party_count as usize {
            aff.character_element(slot + 1)
        } else {
            let id = self.actors.get(slot as usize)?.battle_monster_id?;
            Some(self.monster_catalog.get(id)?.element)
        }
    }

    /// Element id of the *summon creature* a player Seru-magic `spell_id` attacks
    /// as. Retail resolves a player magic cast's attacker element by slot, and a
    /// summon cast runs through the slot-7 summon body - its namesake
    /// `battle_data` creature ([`crate::summon::summon_creature_id`]) - so the
    /// attacker element is that creature's record `+0x1D`, *not* the casting
    /// character's. Resolved off the loaded monster catalog by matching the
    /// spell's display name ([`crate::retail_magic`]) to the lowest creature id
    /// of that name (the `"$2"`/`"$3"` higher-level variants carry distinct names
    /// and are excluded). `None` for a non-summon id or when the catalog / spell
    /// name doesn't resolve.
    /// The `battle_data` creature def a player Seru-magic `spell_id` summons -
    /// the namesake creature (Gimard spell → Gimard creature; see
    /// [`crate::summon::summon_creature_id`]). Resolved by matching the
    /// spell's display name against the loaded monster catalog, so the
    /// `"$2"`/`"$3"` higher-level enemy variants are excluded. `None` when the
    /// id isn't a summon or the catalog doesn't carry the creature (disc-free
    /// / synthetic battles).
    fn summon_creature_def(&self, spell_id: u8) -> Option<&crate::monster_catalog::MonsterDef> {
        if !crate::summon::SERU_SUMMON_IDS.contains(&spell_id) {
            return None;
        }
        let name = crate::retail_magic::get(spell_id)?.name;
        self.monster_catalog
            .by_id
            .values()
            .filter(|d| d.name == name)
            .min_by_key(|d| d.id)
    }

    fn summon_attacker_element(&self, spell_id: u8) -> Option<u8> {
        self.summon_creature_def(spell_id).map(|d| d.element)
    }

    /// Element-affinity multiplier (percent) for a player Seru-magic cast
    /// (`spell_id`) landing on `target`: `matrix[summon-creature element][target
    /// element]` ([`legaia_asset::element_affinity`], `FUN_801dd864`). The
    /// attacker element is the summon creature's ([`Self::summon_attacker_element`]),
    /// the defender element is resolved by slot ([`Self::battle_slot_element`]).
    /// Returns `100` (neutral, no change) when the tables aren't installed, the id
    /// isn't a summon, or either element fails to resolve - so disc-free /
    /// synthetic battles and non-summon casts are unaffected. Applied post-roll,
    /// so it never touches the RNG stream.
    fn cast_affinity_pct(&self, spell_id: u8, target: u8) -> u8 {
        let Some(aff) = self.element_affinity.as_ref() else {
            return 100;
        };
        let Some(atk_elem) = self.summon_attacker_element(spell_id) else {
            return 100;
        };
        let Some(def_elem) = self.battle_slot_element(target) else {
            return 100;
        };
        aff.affinity_pct(atk_elem, def_elem).unwrap_or(100)
    }

    /// Whether a monster caster's chosen move id resolves to a real per-move
    /// power record (so its damage should roll through [`Self::enemy_move_predamage`]
    /// rather than the MP-scaled spell placeholder). Only fires when the
    /// move-power table is installed (disc-real battles), keeping disc-free /
    /// synthetic battles on the placeholder path with an unchanged RNG stream.
    fn enemy_move_power(&self, caster: u8, move_id: u8) -> Option<i32> {
        if (caster as usize) < self.party_count as usize {
            return None;
        }
        self.move_power.as_ref()?.power_for_move_id(move_id)
    }

    /// Fold a single-target [`crate::spells::SpellOutcome`] into live actor
    /// state and surface a HUD popup. Damage subtracts HP (and downs the
    /// target at zero); heals / revives add HP (capped); cures clear the
    /// target's status; buffs adjust a per-slot scalar with a turn timer
    /// ([`Self::apply_battle_buff`]); capture rolls vs the monster's weakened
    /// state ([`Self::resolve_capture`]); escape flags a return to the field
    /// ([`Self::battle_escaped`]). `Failed` is a no-op (MP already spent).
    pub(in crate::world) fn fold_spell_outcome(&mut self, outcome: crate::spells::SpellOutcome) {
        use crate::spells::SpellOutcome as O;
        match outcome {
            O::Damage { target, amount, .. } => {
                // The shared damage finisher fills the defender's spirit-art
                // gauge from any hit (magic included), and it does so on the
                // pre-nullify amount - retail charges the gauge before the
                // absorb stage, so a Stone target's absorbed cast still charges.
                self.accrue_spirit_gauge(target, amount);
                // A petrified target (Stone) absorbs the hit - no HP loss, like
                // the basic-attack path; Stone is invulnerable at every damage
                // entry point.
                let applied = if self.actor_is_petrified(target) {
                    0
                } else {
                    amount
                };
                if let Some(a) = self.actors.get_mut(target as usize) {
                    a.battle.hp = a.battle.hp.saturating_sub(applied);
                    if a.battle.hp == 0 {
                        a.battle.liveness = 0;
                    }
                }
                self.battle_hit_fx.push(BattleHitFx {
                    target_slot: target,
                    amount: applied,
                    is_heal: false,
                    is_crit: false,
                });
                if applied > 0 {
                    let survives = self
                        .actors
                        .get(target as usize)
                        .map(|a| a.battle.hp > 0)
                        .unwrap_or(false);
                    self.queue_battle_reaction(target as usize, survives);
                }
            }
            O::Heal { target, amount } => {
                if let Some(a) = self.actors.get_mut(target as usize) {
                    a.battle.hp = a.battle.hp.saturating_add(amount).min(a.battle.max_hp);
                }
                if amount > 0 {
                    self.battle_hit_fx.push(BattleHitFx {
                        target_slot: target,
                        amount,
                        is_heal: true,
                        is_crit: false,
                    });
                }
            }
            O::Cure { target, .. } => {
                self.status_effects.cure_all(target);
            }
            O::Revive { target, hp } => {
                if let Some(a) = self.actors.get_mut(target as usize) {
                    a.battle.hp = hp.min(a.battle.max_hp);
                    if a.battle.hp > 0 {
                        a.battle.liveness = 1;
                    }
                }
                self.battle_hit_fx.push(BattleHitFx {
                    target_slot: target,
                    amount: hp,
                    is_heal: true,
                    is_crit: false,
                });
            }
            O::Buff {
                target,
                stat,
                magnitude,
                turns,
            } => {
                self.apply_battle_buff(target, stat, magnitude, turns);
            }
            O::CaptureRoll { target, hit_pct } => {
                self.resolve_capture(target, hit_pct);
            }
            O::Escape => {
                self.battle_escaped = true;
            }
            // Multi-target variants aren't produced by per-slot casts; Failed
            // is a no-op (MP was already spent up front).
            _ => {}
        }
    }
}
