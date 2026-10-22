//! Resolving one spell onto its target slots: the paid and prepaid entry points
//! and the shared resolver.

use super::*;

impl World {
    /// Deduct `def`'s MP cost from `caster` and fold its effect onto each
    /// absolute actor slot in `targets`. The direct-fold entry: MP is spent
    /// once up front; each target folds through [`Self::fold_spell_outcome`].
    /// Returns `false` (no MP spent, nothing folded) when the caster can't
    /// afford the cost.
    ///
    /// The action SM's cast band folds through
    /// [`Self::cast_spell_on_slots_prepaid`] instead - the band's `0x28` has
    /// already charged the same ability-bit-folded cost - so every live path
    /// is the prepaid one; this direct entry is the oracles' (the kernel
    /// tests fold a cast without running the band).
    #[cfg(test)]
    pub(in crate::world) fn cast_spell_on_slots(
        &mut self,
        caster: u8,
        def: &legaia_engine_battle::spells::SpellDef,
        targets: &[u8],
    ) -> bool {
        self.cast_spell_on_slots_impl(caster, def, targets, true)
    }

    /// The direct fold (`cast_spell_on_slots`) for a cast whose MP the action SM's
    /// `MagicCastBegin` already debited: no affordability gate, no second
    /// debit, and the snapshot's caster MP is the pre-debit value so the
    /// MP-keyed magnitudes match the direct fold. The summon-creature spawn
    /// request is the band's stager's, so it is not re-raised here.
    pub(in crate::world) fn cast_spell_on_slots_prepaid(
        &mut self,
        caster: u8,
        def: &legaia_engine_battle::spells::SpellDef,
        targets: &[u8],
    ) -> bool {
        self.cast_spell_on_slots_impl(caster, def, targets, false)
    }

    fn cast_spell_on_slots_impl(
        &mut self,
        caster: u8,
        def: &legaia_engine_battle::spells::SpellDef,
        targets: &[u8],
        charge_mp: bool,
    ) -> bool {
        use legaia_engine_battle::spells::{SpellSnapshot, cast_spell};

        // Apply the caster's MP-cost ability-bit modifier (Half 0x20 /
        // Quarter 0x10) so an MP-saver accessory reduces the live cast cost,
        // matching the state-machine cast path (battle_action.rs MagicCastBegin).
        // Only party members carry these accessory bits; monster casters
        // (caster >= party_count) pay full cost. Half takes priority over
        // Quarter when both bits are set, dump-confirmed against the retail
        // state-0x28 block (FUN_801E295C 0x801E3D0C) - routed through the shared
        // battle_formulas helper so all three cast paths agree.
        let ability_bits = if (caster as usize) < self.party.party_count as usize {
            self.party
                .character_ability_bits
                .get(caster as usize)
                .copied()
                .unwrap_or(0)
        } else {
            0
        };
        let cost = legaia_engine_battle::spells::caster_mp_cost(def, ability_bits);
        let (caster_hp, caster_max_hp, caster_mp_now) = match self.actors.get(caster as usize) {
            Some(a) => (a.battle.hp, a.battle.max_hp, a.battle.mp),
            None => return false,
        };
        let caster_mp_before = if charge_mp {
            caster_mp_now
        } else {
            caster_mp_now.saturating_add(cost)
        };
        if charge_mp {
            if caster_mp_now < cost {
                return false;
            }
            if let Some(a) = self.actors.get_mut(caster as usize) {
                a.battle.mp = a.battle.mp.saturating_sub(cost);
            }
        }
        let caster_mag = self.battle.magic.get(caster as usize).copied().unwrap_or(0);

        // For a monster caster whose move id carries a real per-move power
        // record, the damage rolls through the faithful arts/physical kernel
        // seeded with that power instead of the MP-scaled placeholder
        // ([`Self::enemy_move_predamage`]). `None` keeps the placeholder path
        // (disc-free / synthetic battles never install the table).
        let move_power = self
            .enemy_move_power(caster, def.id)
            .or_else(|| self.monster_capture_power(caster, def.id));

        // A party member's Seru-magic cast trains the spell: each damaged
        // target contributes spell XP (the `FUN_801ddb30` accrual tail runs
        // once per finisher call, i.e. per hit - kernel
        // `battle_formulas::summon_spell_xp_gain`), summed here and banked
        // after the cast with the level-up check (`FUN_801e70bc`,
        // [`Self::accrue_summon_spell_xp`]). Retail keys on "attacker slot 7"
        // (the summon body, `0x801DE440`) with no spell-id test, and both the
        // base and the evolved Seru blocks cast through that body
        // ([`legaia_engine_effects::summon::has_summon_body`]); `FUN_801E70BC`'s own
        // threshold multiplier set names evolved ids (`0x8D`). So both blocks
        // accrue. `group_target` mirrors the summon's target byte (`+0x1DD`
        // = 8/9 for a group cast): the per-hit unit drops from 12 to 4.
        let is_party_summon_cast = (caster as usize) < self.party.party_count as usize
            && legaia_engine_effects::summon::has_summon_body(def.id);
        // The high block (`0x99..=0xA0`) trains the spell too: its modules
        // strike through the same slot-7 body, and the two that strike
        // nothing (Horn, Jedo outside a scripted fight) credit inline
        // ([`legaia_engine_effects::summon::module_trains_spell_xp`]).
        let trains_spell_xp = (caster as usize) < self.party.party_count as usize
            && legaia_engine_effects::summon::module_trains_spell_xp(def.id);
        // Jedo's non-scripted arm strikes nothing, so no finisher tail runs
        // for it; it credits a flat grant per living monster seat instead.
        let jedo_banish = def.id == JEDO_SPELL_ID && !self.battle.scripted_fight;
        // Shiny bonus: when the casting character learned this Seru's spell
        // from a shiny capture, every cast deals +35% damage (on top of the
        // normal roll). Mirrors the retail `--shiny-seru` damage hook
        // (`FUN_801dd864` `+0x161` high-bit read). Computed once per cast.
        let shiny_cast = is_party_summon_cast
            && self
                .seru
                .log
                .is_shiny(self.party_roster_slot(caster as usize) as u8, def.id);
        let group_target = matches!(
            def.target,
            legaia_engine_battle::spells::SpellTarget::AllEnemies
        );
        let mut summon_xp_gain: u32 = 0;
        if trains_spell_xp {
            summon_xp_gain = self.inline_module_spell_xp(def.id, jedo_banish);
        }
        // The Seru-magic **side-effect stager** (`FUN_801F3D3C`). Retail's
        // summon module calls it once as the cast commits, before any hit, and
        // the damage finisher then reads the staged percent on every hit - so
        // it is staged here, once, and applied inside the per-target loop.
        //
        // It stages only on the summon path (retail keys on attacker slot 7),
        // which is what `is_party_summon_cast` already identifies. `None` -
        // and no `rand()` draw - for every other cast, for a caster below
        // magic level 3, and on a host with no side-effect table installed.
        let side_effect = if is_party_summon_cast
            && legaia_engine_effects::summon::module_stages_side_effect(def.id)
        {
            self.stage_seru_side_effect(caster, def.id, targets)
        } else {
            None
        };

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
                caster_ability_bits: ability_bits,
                target_mdef: self.battle.defense.get(t as usize).copied().unwrap_or(0),
                target_hp: actor.battle.hp,
                target_hp_max: actor.battle.max_hp,
                target_mp: actor.battle.mp,
                target_alive: actor.battle.liveness != 0,
                target_weakness: legaia_engine_battle::spells::ElementMask::default(),
            };
            let mut outcome = cast_spell(def, t, &snap);
            // Override only the magnitude of a damaging monster special-attack;
            // heals / buffs / non-table moves fall through unchanged. The
            // arts/physical kernel already folds the enemy→party affinity in.
            if let Some(power) = move_power
                && let legaia_engine_battle::spells::SpellOutcome::Damage { amount, .. } =
                    &mut outcome
                && let Some(faithful) = self.enemy_move_predamage(caster, t, power, def.id)
            {
                *amount = faithful;
            } else if let legaia_engine_battle::spells::SpellOutcome::Damage { amount, .. } =
                &mut outcome
            {
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
            // The three **ally-side** player Seru casts do not roll a spell
            // magnitude at all in retail: their module's own tick arm computes
            // the restore from the caster's magic level and stores it. The
            // spell catalog's scalar is a placeholder, so the tick kernel
            // supplies the magnitude here - the same posture
            // `player_summon_predamage` takes for the damaging ones. The same
            // arm credits the spell's XP per healed seat, which banks with the
            // damage path's below.
            if is_party_summon_cast
                && let legaia_engine_battle::spells::SpellOutcome::Heal { amount, .. } =
                    &mut outcome
                && let Some(a) =
                    self.seru_tick_heal_amount(caster, def.id, snap.target_hp, snap.target_hp_max)
            {
                *amount = a;
                summon_xp_gain = summon_xp_gain.saturating_add(self.seru_tick_heal_xp(
                    caster,
                    def.id,
                    snap.target_hp,
                    snap.target_hp_max,
                ));
            }
            // Shiny Seru: +35% on the final magnitude (9999-capped), after the
            // affinity / summon roll so it stacks on the spell's normal output.
            if shiny_cast
                && let legaia_engine_battle::spells::SpellOutcome::Damage { amount, .. } =
                    &mut outcome
            {
                let boosted = (*amount as u32
                    * (100 + legaia_engine_battle::seru_learning::SHINY_DAMAGE_BONUS_PCT))
                    / 100;
                *amount = boosted.min(9999) as u16;
            }
            // Per-hit spell-XP gain (FUN_801ddb30 tail): the final damage
            // delta against the target's live/max HP at the moment of the hit
            // (the snapshot taken above, before the fold applies the damage).
            if trains_spell_xp
                && !jedo_banish
                && let legaia_engine_battle::spells::SpellOutcome::Damage { amount, .. } = &outcome
            {
                summon_xp_gain = summon_xp_gain.saturating_add(
                    legaia_engine_battle_vm::battle_formulas::summon_spell_xp_gain(
                        *amount as u32,
                        snap.target_hp,
                        snap.target_hp_max,
                        group_target,
                    ),
                );
            }
            // A retail-compare seed of a monster cast's landed hit
            // ([`crate::world::BattleState::forced_melee_hits`]): the cast on
            // the ring's front slot takes the amount the capture's ring
            // pushed instead of the replay's own roll (which still drew).
            if caster >= self.party.party_count
                && let legaia_engine_battle::spells::SpellOutcome::Damage { amount, .. } =
                    &mut outcome
                && self
                    .battle
                    .forced_melee_hits
                    .front()
                    .is_some_and(|&(slot, _)| slot == t)
                && let Some((_, known)) = self.battle.forced_melee_hits.pop_front()
                && known != 0
            {
                *amount = known;
            }
            let damaged = matches!(
                outcome,
                legaia_engine_battle::spells::SpellOutcome::Damage { .. }
            );
            self.fold_spell_outcome(outcome);
            // The finisher's per-element stat switch runs once per hit, on the
            // hit's own target - a cure class (light) and a party target write
            // nothing, which `apply_hit` already encodes.
            if damaged && let Some((kind, pct)) = side_effect {
                self.apply_seru_side_effect(t, kind, pct);
            }
        }
        // Bank the accrued XP and run the once-per-cast level-up check
        // (FUN_801e70bc fires at summon return, state 0x36).
        if trains_spell_xp {
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
        if legaia_engine_effects::summon::PLAYER_SUMMON_IDS.contains(&def.id) {
            // A player summon id resolves to a per-summon overlay: request
            // the summon-creature spawn (the retail cast band's `FUN_8003EC70`
            // overlay load - see `legaia_engine_effects::summon`). A band-owned cast's spawn
            // is the stager's (already out and walking by the strike).
            if charge_mp {
                self.request_summon_spawn(def.id, fx_origin);
            }
        } else {
            // Every other move (enemy specials + non-summon spells) that carries
            // a move-power effect list requests its move-FX scene-graph spawn -
            // the `+0x12`/`+0x16` `0x801f6324` prototype records the retail
            // strike setup (`FUN_801e09f8`) spawns on the launch / on-contact
            // transitions. No-op when the move has no FX entries.
            //
            // A caster whose committed effect script still has its
            // terminator ahead hands the lists to the homing flight that
            // terminator seeds instead (`World::seed_homing_slots`), which
            // spawns them along the way and on landing, as retail does.
            //
            // A flight its terminator already seeded holds the lists
            // (`CastFxState::homing_holds_lists`): only the move's sound cue
            // surfaces, as on the deferred path.
            if self.casting.homing_holds_lists == Some(caster) {
                self.casting.homing_holds_lists = None;
                if let Some(cue) = self
                    .tables
                    .move_power
                    .as_ref()
                    .and_then(|cat| cat.fx_for_move_id(def.id))
                    .map(|fx| fx.sound_cue_id)
                    .filter(|&c| c != 0)
                {
                    self.casting.pending_move_fx_cue = Some(cue);
                }
            } else {
                self.casting.homing_takes_lists = self.effect_script_terminates(caster as usize);
                self.request_move_fx_spawn(def.id, fx_origin);
            }
        }
        true
    }
}
