//! Per-actor battle stat modifiers: equipment resist, escape roll, spirit-art
//! gauge, and buff / debuff application. Split out of `battle.rs` as additional
//! `impl World` blocks; no logic change from the original inline definitions.

use super::*;

impl World {
    /// The defender's equipment-derived resist / spirit-gain flags for an
    /// actor slot: the first two words of the occupying character's
    /// accessory-passive ability bitfield (record `+0xF4`/`+0xF8`, rebuilt
    /// from the eight equip slots by [`Self::refresh_party_ability_bits`]) -
    /// exactly the two words retail's damage finisher `FUN_801ddb30` indexes
    /// (elemental-guard passives `0x1D..=0x24`, AP Boost `0x28`/`0x29`).
    /// Enemy slots (and unresolvable roster slots) carry no resistance.
    ///
    /// REF: FUN_801ddb30 (resist-word source)
    pub(in crate::world) fn defender_resist(
        &self,
        slot: u8,
    ) -> vm::battle_formulas::DefenderResist {
        if slot >= self.party.party_count {
            return Default::default();
        }
        let Some(member) = self
            .party
            .roster
            .members
            .get(self.party_roster_slot(slot as usize))
        else {
            return Default::default();
        };
        let bits = member.ability_bits();
        vm::battle_formulas::DefenderResist::from_ability_words(
            u32::from_le_bytes([bits[0], bits[1], bits[2], bits[3]]),
            u32::from_le_bytes([bits[4], bits[5], bits[6], bits[7]]),
        )
    }

    /// The defence half a melee command reads on `slot`: UDF (`+0x15C`) when
    /// `(command - 0x0C) % 10 < 5`, LDF (`+0x160`) otherwise
    /// ([`vm::battle_formulas::physical_defense_is_udf`], `FUN_801EC3E4` at
    /// `0x801ECE14`).
    ///
    /// Both party slots ([`Self::seed_party_battle_stats`]) and monster slots
    /// (battle entry, from the catalog's UDF / LDF pair) carry a real split.
    /// A slot with none configured - a synthetic battle that wrote only
    /// [`crate::world::BattleState::defense`] - falls back to that scalar, so both halves
    /// answer the same value and the parity pick is a no-op for it.
    pub fn physical_defense_of(&self, slot: u8, command: u8) -> u16 {
        let idx = slot as usize;
        if let Some(Some((udf, ldf))) = self.battle.defense_split.get(idx) {
            return if vm::battle_formulas::physical_defense_is_udf(command) {
                *udf
            } else {
                *ldf
            };
        }
        self.battle.defense.get(idx).copied().unwrap_or(0)
    }

    /// Roll the Run command's escape chance - the retail `FUN_801E791C`
    /// formula (the routine battle-action state `0x64` calls, the writer of
    /// the `_DAT_8007726C` outcome pointer). Party score = per-slot
    /// `(SPD*3)>>1 + missingHP>>4` over every party slot (downed included);
    /// enemy score = `SPD + missingHP>>5` over every enemy slot; two 15-bit
    /// rand draws in retail order; the Chicken Heart (Escape Boost, ability
    /// bit 52) and Chicken King (Great Escape, bit 55) accessory bits fold
    /// from the *living* party members' second ability word (`record+0xF8`).
    ///
    /// The second source of the assured arm is the **latched formation
    /// advantage** [`World::battle_formation_latched`] (`ctx+0x291`): after a
    /// pre-emptive strike the party roll is set equal to the enemy roll, so the
    /// compare cannot fail. "Assured" is still the wrong word for it - retail
    /// tests `ctx+0x287` *after* that compare, so a pre-emptive strike into a
    /// scripted no-flee battle is caught anyway.
    ///
    /// The scripted no-escape flag (`ctx+0x287`) is the engine's
    /// [`crate::world::BattleState::no_escape`], set at scripted-battle entry
    /// ([`World::trigger_scripted_battle`] - the boss fights); the forced
    /// flee `_DAT_8007bac0 & 0x100` passes as unset.
    ///
    /// PORT: FUN_801E791C (roll + compare via
    /// `battle_formulas::escape_roll`; the success-side flee staging stays
    /// with the run band in `battle_action`)
    pub(in crate::world) fn roll_battle_escape(&mut self) -> bool {
        use vm::battle_formulas::{
            EscapeActor, EscapeFlags, escape_enemy_score, escape_party_score, escape_roll,
        };
        let party_n = (self.party.party_count as usize).min(self.actors.len());
        let fold = |i: usize| EscapeActor {
            speed: self.battle.speed.get(i).copied().unwrap_or(0),
            hp: self.actors[i].battle.hp,
            max_hp: self.actors[i].battle.max_hp,
        };
        let party: Vec<EscapeActor> = (0..party_n).map(fold).collect();
        let enemies: Vec<EscapeActor> = (party_n..self.actors.len()).map(fold).collect();
        let mut flags = EscapeFlags {
            no_escape: self.battle.no_escape,
            ..EscapeFlags::default()
        };
        // `ctx+0x291` - the latched formation advantage. A pre-emptive strike
        // sets `roll_p = roll_e` so the `roll_p < roll_e` compare cannot fail.
        // Note this is the *latched* copy: it is only non-`None` because
        // `World::latch_battle_formation` copied it out of `+0x290` at battle
        // start, before the seeder's lockout cleared it.
        flags.fold_formation_latch(self.battle_formation_latched());
        for slot in 0..party_n {
            if self.actors[slot].battle.liveness == 0 {
                continue;
            }
            if let Some(member) = self.party.roster.members.get(self.party_roster_slot(slot)) {
                let bits = member.ability_bits();
                flags.fold_ability_word1(u32::from_le_bytes([bits[4], bits[5], bits[6], bits[7]]));
            }
        }
        let rand = [self.next_rand() as u16, self.next_rand() as u16];
        escape_roll(
            escape_party_score(&party),
            escape_enemy_score(&enemies),
            flags,
            rand,
        )
    }

    /// The enemy-side flee decision for the monster in `slot` - the roll the
    /// action picker's once-per-pass checkpoint makes (`FUN_801E9FD4` calling
    /// `FUN_801EC0DC` with the monster's pool slot).
    ///
    /// Side scores fold live HP/max-HP off the actors and live ATK off the
    /// [`crate::world::BattleState::attack`] sidecar (retail reads actor `+0x158`); the
    /// party's No Escape / Chicken Guard bit folds from each living member's
    /// second ability word exactly as [`Self::roll_battle_escape`] folds its
    /// escape accessories. The fleeing monster's INT (`+0x168`) comes from the
    /// monster catalog - the engine carries no live INT sidecar, and no battle
    /// buff writes INT, so the catalog stat is the live value.
    ///
    /// Draws battle RNG through the closure in retail call order (two draws,
    /// plus the 1-in-8 gate draw only when the score compare passes).
    ///
    /// REF: FUN_801EC0DC
    pub(in crate::world) fn monster_flee_roll(&mut self, slot: u8) -> bool {
        use vm::battle_formulas::FleeActor;
        let pc = (self.party.party_count as usize).min(self.actors.len());
        let fold = |world: &Self, i: usize| FleeActor {
            hp: world.actors[i].battle.hp,
            max_hp: world.actors[i].battle.max_hp,
            atk: world.battle.attack.get(i).copied().unwrap_or(0),
        };
        let party: Vec<FleeActor> = (0..pc).map(|i| fold(self, i)).collect();
        let monsters: Vec<FleeActor> = (pc..self.actors.len()).map(|i| fold(self, i)).collect();
        let ability_word1: Vec<u32> = (0..pc)
            .map(|i| {
                self.party
                    .roster
                    .members
                    .get(self.party_roster_slot(i))
                    .map(|m| {
                        let bits = m.ability_bits();
                        u32::from_le_bytes([bits[4], bits[5], bits[6], bits[7]])
                    })
                    .unwrap_or(0)
            })
            .collect();
        let target = fold(self, slot as usize);
        let target_int = self
            .actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| self.tables.monster_catalog.get(id))
            .map(|d| d.intel)
            .unwrap_or(0);
        let no_escape = u8::from(self.battle.no_escape);
        vm::battle_formulas::monster_escape_roll(
            no_escape,
            &party,
            &monsters,
            &ability_word1,
            target,
            target_int,
            || self.next_rand(),
        )
    }

    /// Accrue the defender's spirit-art gauge (`actor+0x170`) from a hit that
    /// landed `over` damage, the spirit stage of the shared damage finisher
    /// `FUN_801ddb30`. Runs for *any* defender (the base `pct` term is
    /// unconditional); the two equipment "spirit gain up" bits (AP Boost 1/2,
    /// ability-bitfield word 1 `0x100`/`0x200`) apply to a party defender via
    /// [`Self::defender_resist`], so a Mettle Ring / Mettle Armband holder
    /// charges faster - and an enemy resolves to the no-gain default. Draws
    /// no RNG, so the determinism stream is untouched; `over` is the
    /// post-mitigation damage already computed by the caller (the pre-nullify
    /// value retail accrues from).
    ///
    /// PORT: FUN_801ddb30 (spirit-gauge stage)
    pub(in crate::world) fn accrue_spirit_gauge(&mut self, defender_slot: u8, over: u16) {
        let defender_is_party = defender_slot < self.party.party_count;
        let resist = self.defender_resist(defender_slot);
        let Some(a) = self.actors.get_mut(defender_slot as usize) else {
            return;
        };
        a.battle.spirit_gauge = vm::battle_formulas::spirit_gauge_fill(
            over as u32,
            a.battle.max_hp,
            a.battle.spirit_gauge,
            resist,
            defender_is_party,
        );
    }

    /// The current spirit-art gauge value (0..=100) for an actor slot, or `0`
    /// for an out-of-range slot. The HUD reads this to draw the spirit bar and
    /// the command menu reads [`Self::spirit_gauge_full`] to gate the Spirit-Art
    /// option.
    pub fn spirit_gauge(&self, slot: u8) -> u16 {
        self.actors
            .get(slot as usize)
            .map(|a| a.battle.spirit_gauge)
            .unwrap_or(0)
    }

    /// `true` when `slot`'s spirit-art gauge has reached its ceiling (100), the
    /// retail condition for a Spirit-Art being available
    /// ([`vm::battle_action::ActionState::SpiritArtsEntry`]).
    pub fn spirit_gauge_full(&self, slot: u8) -> bool {
        self.spirit_gauge(slot) >= 100
    }

    /// Apply (or refresh) a stat buff / debuff on `slot`. The delta is written
    /// straight into the matching per-slot battle scalar so it changes damage
    /// the same frame: `Attack`/`MagicAttack`/`Defense` map to
    /// [`crate::world::BattleState::attack`] / [`crate::world::BattleState::magic`] / [`crate::world::BattleState::defense`]
    /// (`MagicDefense` reuses `battle_defense`, the spell-defense proxy).
    ///
    /// **Stat-up buffs (`magnitude > 0`) use the retail multiplicative ramp.**
    /// Retail's stat-up selectors (1..7) raise the live stat by ×6/5 (clamped to
    /// `0xFFFF`) - [`vm::battle_formulas::buff_ramp`], pinned from the SM dump -
    /// not by a flat additive delta. So a positive buff ramps the scalar by +20%
    /// of its *current* value (the per-spell `magnitude` value is now only a
    /// sign hint for the pinned scalar stats). **Debuffs (`magnitude <= 0`) stay
    /// additive**: retail's debuff scaling is not yet pinned, so the engine keeps
    /// the saturating additive model rather than fabricate a factor.
    ///
    /// The recorded `applied_delta` is the exact `u16` change either way (for
    /// precise undo on expiry). Accuracy / Evasion / Speed have no live-loop
    /// scalar; the buff is tracked with a zero delta so the turn timer still
    /// runs. Re-casting the same `(slot, stat)` refreshes: the old delta is
    /// reverted first (so the ramp re-applies from the base, no compounding on
    /// refresh).
    pub(in crate::world) fn apply_battle_buff(
        &mut self,
        slot: u8,
        stat: crate::spells::BuffStat,
        magnitude: i16,
        turns: u8,
    ) {
        // Refresh: revert + drop any existing buff on this (slot, stat).
        if let Some(pos) = self
            .battle
            .buffs
            .iter()
            .position(|b| b.slot == slot && b.stat == stat)
        {
            let old = self.battle.buffs.remove(pos);
            self.add_to_buff_scalar(old.slot, old.stat, -old.applied_delta);
        }
        if turns == 0 {
            return;
        }
        let applied_delta = if magnitude > 0 {
            // Retail stat-up: ×6/5 ramp of the current scalar (pinned).
            self.ramp_buff_scalar(slot, stat)
        } else {
            // Debuff: additive (retail factor unpinned), saturating at 0.
            self.add_to_buff_scalar(slot, stat, magnitude)
        };
        self.battle.buffs.push(BattleBuff {
            slot,
            stat,
            applied_delta,
            turns,
        });
    }

    /// Apply the retail `×6/5` stat-up ramp ([`vm::battle_formulas::buff_ramp`])
    /// to the per-slot scalar backing `stat`, returning the exact `u16` change.
    /// Stats with no live-loop scalar (Accuracy / Evasion / Speed) return `0`.
    ///
    /// A Defense ramp is taken from the slot's **UDF half** when it carries a
    /// [`crate::world::BattleState::defense_split`] - see [`Self::move_defense_split`] for why
    /// the scalar alone is the wrong basis.
    fn ramp_buff_scalar(&mut self, slot: u8, stat: crate::spells::BuffStat) -> i16 {
        use crate::spells::BuffStat;
        if matches!(stat, BuffStat::Defense | BuffStat::MagicDefense)
            && let Some(Some((udf, _))) = self.battle.defense_split.get(slot as usize).copied()
        {
            let delta = (i32::from(vm::battle_formulas::buff_ramp(udf)) - i32::from(udf)) as i16;
            return self.add_to_buff_scalar(slot, stat, delta);
        }
        let scalar = match stat {
            BuffStat::Attack => self.battle.attack.get_mut(slot as usize),
            BuffStat::MagicAttack => self.battle.magic.get_mut(slot as usize),
            BuffStat::Defense | BuffStat::MagicDefense => {
                self.battle.defense.get_mut(slot as usize)
            }
            BuffStat::Accuracy | BuffStat::Evasion | BuffStat::Speed => None,
        };
        let Some(scalar) = scalar else { return 0 };
        let before = *scalar;
        let after = vm::battle_formulas::buff_ramp(before);
        *scalar = after;
        (after as i32 - before as i32) as i16
    }

    /// Add `delta` to the per-slot scalar backing `stat` and return the exact
    /// change made (after `u16` saturation). Stats with no live-loop scalar
    /// return `0`. A Defense change also moves both halves of the slot's
    /// defence split ([`Self::move_defense_split`]).
    pub(super) fn add_to_buff_scalar(
        &mut self,
        slot: u8,
        stat: crate::spells::BuffStat,
        delta: i16,
    ) -> i16 {
        use crate::spells::BuffStat;
        if matches!(stat, BuffStat::Defense | BuffStat::MagicDefense) {
            self.move_defense_split(slot, delta);
        }
        let scalar = match stat {
            BuffStat::Attack => self.battle.attack.get_mut(slot as usize),
            BuffStat::MagicAttack => self.battle.magic.get_mut(slot as usize),
            BuffStat::Defense | BuffStat::MagicDefense => {
                self.battle.defense.get_mut(slot as usize)
            }
            BuffStat::Accuracy | BuffStat::Evasion | BuffStat::Speed => None,
        };
        let Some(scalar) = scalar else { return 0 };
        let before = *scalar as i32;
        let after = (before + delta as i32).clamp(0, u16::MAX as i32);
        *scalar = after as u16;
        (after - before) as i16
    }

    /// Move both halves of `slot`'s defence split by `delta`, saturating at
    /// zero. No-op for a slot with no split.
    ///
    /// The physical path reads the split, not the [`crate::world::BattleState::defense`]
    /// scalar ([`Self::physical_defense_of`]), so a Defense buff that touched
    /// only the scalar changed nothing a swing could see. That was already true
    /// for every party slot - [`Self::seed_party_battle_stats`] writes the split
    /// and never the scalar, so the scalar sat at `0` and the `×6/5` ramp of `0`
    /// is `0` - and seeding the monster band's split would have extended the
    /// same inertness to enemies. Retail's "Defense Up" raises `defense_high`
    /// and `defense_low` together, which is what moving both halves models.
    ///
    /// **Bounded divergence:** retail ramps each facet by its own `×6/5`; the
    /// engine applies one delta, taken from the UDF half, to both. The stored
    /// `applied_delta` stays a single exactly-reversible number, so a buff that
    /// expires restores the pair it found.
    fn move_defense_split(&mut self, slot: u8, delta: i16) {
        if let Some(Some((udf, ldf))) = self.battle.defense_split.get_mut(slot as usize) {
            let shift = |v: &mut u16| {
                *v = (i32::from(*v) + i32::from(delta)).clamp(0, i32::from(u16::MAX)) as u16;
            };
            shift(udf);
            shift(ldf);
        }
    }

    /// Tick the buffs on `slot` at the start of its turn: decrement each, and
    /// revert + drop those that reach zero.
    pub(in crate::world) fn tick_battle_buffs_on_turn(&mut self, slot: u8) {
        let mut expired: Vec<BattleBuff> = Vec::new();
        self.battle.buffs.retain_mut(|b| {
            if b.slot != slot {
                return true;
            }
            b.turns = b.turns.saturating_sub(1);
            if b.turns == 0 {
                expired.push(*b);
                false
            } else {
                true
            }
        });
        for b in expired {
            self.add_to_buff_scalar(b.slot, b.stat, -b.applied_delta);
        }
    }
}

impl World {
    /// Copy every working stat halfword into its **base** twin, for every
    /// seat - retail's record -> actor copy writing both `sh`s of each pair
    /// (`FUN_80054CB0` for a monster, `FUN_80053CB8` for a party member).
    ///
    /// Called once at battle entry, after both sides' stats are seeded. From
    /// there the base halves are frozen: nothing in a fight writes one except
    /// a Seru side-effect debuff, which is exactly why the stager can use
    /// "base != record" as its "something already moved this stat" gate
    /// ([`Self::enemy_stat_compare`]).
    ///
    /// REF: FUN_80054CB0 (the `sh` pairs at `0x80055160..0x8005530C`; the
    /// routine is ported at
    /// [`crate::monster_catalog::monster_def_from_record`] and its arithmetic
    /// at [`crate::monster_catalog::MonsterDef::installed_stats`])
    pub(in crate::world) fn sync_battle_stat_bases(&mut self) {
        for slot in 0..self.battle.attack.len() {
            self.battle.attack_base[slot] = self.battle.attack[slot];
            self.battle.defense_base[slot] = self.battle.defense_split[slot];
            self.battle.speed_base[slot] = self.battle.speed[slot];
            self.battle.accuracy_base[slot] = self.battle.accuracy[slot];
        }
    }

    /// The `(base halfword, raw record field)` pairs the Seru side-effect
    /// stager compares for an enemy seat.
    ///
    /// The record side is the monster archive record's own
    /// `[AGL, ATK, UDF, LDF, INT, SPD]` block plus its MP - never the
    /// installed stats - because the whole point of the compare is to notice
    /// that the battle loader's boost profile (or an earlier debuff) moved
    /// the actor off the record.
    ///
    /// `None` for a party seat, or a monster seat whose catalog entry carries
    /// no record block (a synthetic monster): the stager's compare is then
    /// skipped, which is the same answer retail gives for a party target.
    ///
    /// REF: FUN_801F3D3C (`0x801F3EB4` jump table, the six compare arms)
    pub(in crate::world) fn enemy_stat_compare(
        &self,
        slot: u8,
    ) -> Option<vm::seru_side_effect::EnemyCompare> {
        use vm::seru_side_effect::{EnemyCompare, StatCompare};
        if (slot as usize) < self.party.party_count as usize {
            return None;
        }
        let id = self.actors.get(slot as usize)?.battle_monster_id?;
        let def = self.tables.monster_catalog.get(id)?;
        if def.raw_stats == [0u16; 6] {
            return None;
        }
        let raw = def.raw_stats;
        let i = slot as usize;
        let (udf_base, _ldf_base) = self.battle.defense_base.get(i).copied().flatten()?;
        let agl_base = self.actors.get(i)?.battle.agl_base;
        Some(EnemyCompare {
            udf: StatCompare {
                base: udf_base,
                record: raw[2],
            },
            agl: StatCompare {
                base: agl_base,
                record: raw[0],
            },
            atk: StatCompare {
                base: self.battle.attack_base.get(i).copied().unwrap_or(0),
                record: raw[1],
            },
            spd: StatCompare {
                base: self.battle.speed_base.get(i).copied().unwrap_or(0),
                record: raw[5],
            },
            int: StatCompare {
                base: self.battle.accuracy_base.get(i).copied().unwrap_or(0),
                record: raw[4],
            },
            // The dark compare reads the MP **base** half `+0x152`, which both
            // boost profiles copy straight from the record and which no battle
            // write moves (a dark hit shaves current MP `+0x150` only). So the
            // pair is equal for the whole fight and dark always stages.
            mp: StatCompare {
                base: def.mp,
                record: def.mp,
            },
        })
    }

    /// Run the Seru side-effect **stager** for one player Seru-magic cast and
    /// return the `(kind, percent)` the damage finisher will subtract on every
    /// hit of it, or `None` when nothing was staged.
    ///
    /// This is retail's `FUN_801F3D3C`, which the cast's summon module calls
    /// once as the cast commits - not per hit. The port runs it at the same
    /// seam: [`World::cast_spell_on_slots_prepaid`], right before the
    /// per-target fold, so the single suppression `rand()` draw lands in the
    /// same place in the stream.
    ///
    /// Inputs, each read off the live world:
    /// - **level**: the caster's per-spell magic level (record `+0x161`
    ///   parallel to the `+0x13D` id list), via
    ///   [`Self::caster_magic_power_byte`]. Below `3` nothing stages and
    ///   **no draw is taken**.
    /// - **summon element**: the summon creature's record `+0x1D`.
    /// - **scripted**: [`crate::world::BattleState::scripted_fight`].
    /// - **affinity**: `matrix[summon element][first living enemy element]`,
    ///   the scripted roll's bypass.
    /// - **target**: the cast's first enemy seat's compare pairs, or
    ///   `Party` for a party-side cast.
    ///
    /// Returns `None` with no draw when the side-effect table is not
    /// installed (a disc-free host), so those battles keep a bit-identical
    /// RNG stream.
    ///
    /// REF: FUN_801f3d3c (this is the live wiring; the routine itself is
    /// ported at [`legaia_engine_vm::seru_side_effect::stage_side_effect`])
    pub(in crate::world) fn stage_seru_side_effect(
        &mut self,
        caster: u8,
        spell_id: u8,
        targets: &[u8],
    ) -> Option<(legaia_asset::seru_side_effect::SideEffectKind, u8)> {
        use vm::seru_side_effect::{StagerInputs, StagerOutcome, StagerTarget, stage_side_effect};
        let table = self.tables.seru_side_effects.clone()?;
        let level = self.caster_magic_power_byte(caster, spell_id);
        // The summon creature's record element. Prefer the disc-resolved
        // per-spell map: the monster catalog only carries the scene's own
        // monsters, so its by-name lookup answers `None` for the summon in
        // nearly every fight.
        let summon_element = self
            .tables
            .summon_elements
            .get(&spell_id)
            .copied()
            .or_else(|| self.summon_attacker_element(spell_id))?;
        // The stager's compare arm walks the enemy row for a group cast and
        // takes the first living seat; a single-enemy cast compares that seat.
        // A party-side target skips the compare entirely.
        let first_enemy = targets
            .iter()
            .copied()
            .find(|&t| (t as usize) >= self.party.party_count as usize);
        let target = match first_enemy {
            None => StagerTarget::Party,
            Some(t) => match self.enemy_stat_compare(t) {
                Some(cmp) => StagerTarget::Enemy(cmp),
                None => StagerTarget::NoLivingEnemy,
            },
        };
        // `matrix[summon element][target element]` - the same read
        // `cast_affinity_pct` makes, but seeded from the element resolved
        // above rather than from the catalog-by-name lookup.
        let affinity_pct = match (first_enemy, self.tables.element_affinity.as_ref()) {
            (Some(t), Some(aff)) => self
                .battle_slot_element(t)
                .and_then(|def_el| aff.affinity_pct(summon_element, def_el))
                .unwrap_or(100),
            _ => 100,
        };
        let inp = StagerInputs {
            level,
            summon_element,
            scripted: self.battle.scripted_fight,
            affinity_pct,
            target,
        };
        // The one draw retail takes on this path (`jal 0x80056798` inside the
        // scripted arm) comes off the shared cursor, lazily - `stage` calls
        // the closure only on the arm that rolls, so a random encounter and a
        // sub-level-3 cast advance it not at all.
        let outcome = stage_side_effect(&table, &inp, || self.next_rand() as i32);
        // The stager's own store: retail writes the staged percent into
        // `0x801F6960` (`0x801F4444..`), which is the latch the summon's
        // return-from-fade pass reads to decide whether to print
        // "No effect." (`FUN_801F3C34`, ported at
        // `legaia_engine_vm::move_no_effect_guard::queued_magic_message` and
        // live from action state `0x36`). Nothing wrote the port's copy, so
        // every levelled cast announced itself as a miss even when its effect
        // had landed. Written on every player Seru cast - the percent when
        // something staged, `0` otherwise - so it cannot go stale across
        // casts.
        self.battle_ctx.follow_up_pending = match outcome {
            StagerOutcome::Staged { amount, .. } => amount,
            _ => 0,
        };
        match outcome {
            StagerOutcome::Staged { kind, amount, .. } => Some((kind, amount)),
            _ => None,
        }
    }

    /// Apply one hit's staged Seru side effect to `slot` - the finisher's
    /// per-element stat switch, run over the live halfword pairs.
    ///
    /// Retail's `FUN_801DDB30` runs this on the summon path once per hit, and
    /// a zero percent (nothing staged) subtracts zero, which is how a
    /// suppressed cast stays inert without a second gate. The port only calls
    /// it when the stager actually staged, so the zero case never arrives.
    ///
    /// The write-back mirrors retail's own halfword choice exactly: both
    /// halves of each DEF / ATK / SPD / INT pair, the AGL **base** only, and
    /// the **current** MP only.
    ///
    /// REF: FUN_801ddb30 (this is the live wiring of
    /// `0x801DE60C..0x801DE8EC`; the switch itself is ported at
    /// [`legaia_engine_vm::seru_side_effect::apply_hit`])
    pub(in crate::world) fn apply_seru_side_effect(
        &mut self,
        slot: u8,
        kind: legaia_asset::seru_side_effect::SideEffectKind,
        pct: u8,
    ) {
        use vm::seru_side_effect::{TargetStats, apply_hit};
        let i = slot as usize;
        if i >= self.battle.attack.len() || i >= self.actors.len() {
            return;
        }
        let (udf, ldf) = self.battle.defense_split[i]
            .unwrap_or((self.battle.defense[i], self.battle.defense[i]));
        let (udf_b, ldf_b) = self.battle.defense_base[i].unwrap_or((udf, ldf));
        let mut s = TargetStats {
            udf: (udf, udf_b),
            ldf: (ldf, ldf_b),
            atk: (self.battle.attack[i], self.battle.attack_base[i]),
            spd: (self.battle.speed[i], self.battle.speed_base[i]),
            int: (self.battle.accuracy[i], self.battle.accuracy_base[i]),
            agl_base: self.actors[i].battle.agl_base,
            mp: self.actors[i].battle.mp,
        };
        apply_hit(kind, pct, &mut s);
        self.battle.defense_split[i] = Some((s.udf.0, s.ldf.0));
        self.battle.defense_base[i] = Some((s.udf.1, s.ldf.1));
        self.battle.defense[i] = s.udf.0.max(s.ldf.0);
        self.battle.attack[i] = s.atk.0;
        self.battle.attack_base[i] = s.atk.1;
        self.battle.speed[i] = s.spd.0;
        self.battle.speed_base[i] = s.spd.1;
        self.battle.accuracy[i] = s.int.0;
        self.battle.accuracy_base[i] = s.int.1;
        // Evasion is the port's second name for the same retail halfword
        // (`+0x168`), so it follows the working half rather than drifting.
        self.battle.evasion[i] = s.int.0;
        self.actors[i].battle.agl_base = s.agl_base;
        self.actors[i].battle.mp = s.mp;
    }
}
