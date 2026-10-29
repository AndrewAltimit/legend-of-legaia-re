//! The per-seat stat block's own steps: the working / base halves the
//! damage kernels read, the stat buffs a Magic cast applies and their
//! per-turn expiry, and the UDF / LDF defence pick.

use crate::battle_state::BattleBuff;
use crate::battle_state::BattleState;

impl BattleState {
    /// The defence half a melee command reads on `slot`: UDF (`+0x15C`) when
    /// `(command - 0x0C) % 10 < 5`, LDF (`+0x160`) otherwise
    /// ([`legaia_engine_battle_vm::battle_formulas::physical_defense_is_udf`], `FUN_801EC3E4` at
    /// `0x801ECE14`).
    ///
    /// Both party slots (`World::seed_party_battle_stats`) and monster slots
    /// (battle entry, from the catalog's UDF / LDF pair) carry a real split.
    /// A slot with none configured - a synthetic battle that wrote only
    /// `legaia_engine_battle_loop::battle_state::BattleState::defense` - falls back to that scalar, so both halves
    /// answer the same value and the parity pick is a no-op for it.
    pub fn physical_defense_of(&self, slot: u8, command: u8) -> u16 {
        let idx = slot as usize;
        if let Some(Some((udf, ldf))) = self.defense_split.get(idx) {
            return if legaia_engine_battle_vm::battle_formulas::physical_defense_is_udf(command) {
                *udf
            } else {
                *ldf
            };
        }
        self.defense.get(idx).copied().unwrap_or(0)
    }

    /// Apply (or refresh) a stat buff / debuff on `slot`. The delta is written
    /// straight into the matching per-slot battle scalar so it changes damage
    /// the same frame: `Attack`/`MagicAttack`/`Defense` map to
    /// `legaia_engine_battle_loop::battle_state::BattleState::attack` / `legaia_engine_battle_loop::battle_state::BattleState::magic` / `legaia_engine_battle_loop::battle_state::BattleState::defense`
    /// (`MagicDefense` reuses `battle_defense`, the spell-defense proxy).
    ///
    /// **Stat-up buffs (`magnitude > 0`) use the retail multiplicative ramp.**
    /// Retail's stat-up selectors (1..7) raise the live stat by ×6/5 (clamped to
    /// `0xFFFF`) - `vm::battle_formulas::buff_ramp`, pinned from the SM dump -
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
    pub fn apply_battle_buff(
        &mut self,
        slot: u8,
        stat: legaia_engine_battle::spells::BuffStat,
        magnitude: i16,
        turns: u8,
    ) {
        // Refresh: revert + drop any existing buff on this (slot, stat).
        if let Some(pos) = self
            .buffs
            .iter()
            .position(|b| b.slot == slot && b.stat == stat)
        {
            let old = self.buffs.remove(pos);
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
        self.buffs.push(BattleBuff {
            slot,
            stat,
            applied_delta,
            turns,
        });
    }

    /// Apply the retail `×6/5` stat-up ramp (`vm::battle_formulas::buff_ramp`)
    /// to the per-slot scalar backing `stat`, returning the exact `u16` change.
    /// Stats with no live-loop scalar (Accuracy / Evasion / Speed) return `0`.
    ///
    /// A Defense ramp is taken from the slot's **UDF half** when it carries a
    /// `legaia_engine_battle_loop::battle_state::BattleState::defense_split` - see [`Self::move_defense_split`] for why
    /// the scalar alone is the wrong basis.
    pub fn ramp_buff_scalar(
        &mut self,
        slot: u8,
        stat: legaia_engine_battle::spells::BuffStat,
    ) -> i16 {
        use legaia_engine_battle::spells::BuffStat;
        if matches!(stat, BuffStat::Defense | BuffStat::MagicDefense)
            && let Some(Some((udf, _))) = self.defense_split.get(slot as usize).copied()
        {
            let delta = (i32::from(legaia_engine_battle_vm::battle_formulas::buff_ramp(udf))
                - i32::from(udf)) as i16;
            return self.add_to_buff_scalar(slot, stat, delta);
        }
        let scalar = match stat {
            BuffStat::Attack => self.attack.get_mut(slot as usize),
            BuffStat::MagicAttack => self.magic.get_mut(slot as usize),
            BuffStat::Defense | BuffStat::MagicDefense => self.defense.get_mut(slot as usize),
            BuffStat::Accuracy | BuffStat::Evasion | BuffStat::Speed => None,
        };
        let Some(scalar) = scalar else { return 0 };
        let before = *scalar;
        let after = legaia_engine_battle_vm::battle_formulas::buff_ramp(before);
        *scalar = after;
        (after as i32 - before as i32) as i16
    }

    /// Add `delta` to the per-slot scalar backing `stat` and return the exact
    /// change made (after `u16` saturation). Stats with no live-loop scalar
    /// return `0`. A Defense change also moves both halves of the slot's
    /// defence split ([`Self::move_defense_split`]).
    pub fn add_to_buff_scalar(
        &mut self,
        slot: u8,
        stat: legaia_engine_battle::spells::BuffStat,
        delta: i16,
    ) -> i16 {
        use legaia_engine_battle::spells::BuffStat;
        if matches!(stat, BuffStat::Defense | BuffStat::MagicDefense) {
            self.move_defense_split(slot, delta);
        }
        let scalar = match stat {
            BuffStat::Attack => self.attack.get_mut(slot as usize),
            BuffStat::MagicAttack => self.magic.get_mut(slot as usize),
            BuffStat::Defense | BuffStat::MagicDefense => self.defense.get_mut(slot as usize),
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
    /// The physical path reads the split, not the `legaia_engine_battle_loop::battle_state::BattleState::defense`
    /// scalar ([`Self::physical_defense_of`]), so a Defense buff that touched
    /// only the scalar changed nothing a swing could see. That was already true
    /// for every party slot - `World::seed_party_battle_stats` writes the split
    /// and never the scalar, so the scalar sat at `0` and the `×6/5` ramp of `0`
    /// is `0` - and seeding the monster band's split would have extended the
    /// same inertness to enemies. Retail's "Defense Up" raises `defense_high`
    /// and `defense_low` together, which is what moving both halves models.
    ///
    /// **Bounded divergence:** retail ramps each facet by its own `×6/5`; the
    /// engine applies one delta, taken from the UDF half, to both. The stored
    /// `applied_delta` stays a single exactly-reversible number, so a buff that
    /// expires restores the pair it found.
    pub fn move_defense_split(&mut self, slot: u8, delta: i16) {
        if let Some(Some((udf, ldf))) = self.defense_split.get_mut(slot as usize) {
            let shift = |v: &mut u16| {
                *v = (i32::from(*v) + i32::from(delta)).clamp(0, i32::from(u16::MAX)) as u16;
            };
            shift(udf);
            shift(ldf);
        }
    }

    /// Tick the buffs on `slot` at the start of its turn: decrement each, and
    /// revert + drop those that reach zero.
    pub fn tick_battle_buffs_on_turn(&mut self, slot: u8) {
        let mut expired: Vec<BattleBuff> = Vec::new();
        self.buffs.retain_mut(|b| {
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

    /// Copy every working stat halfword into its **base** twin, for every
    /// seat - retail's record -> actor copy writing both `sh`s of each pair
    /// (`FUN_80054CB0` for a monster, `FUN_80053CB8` for a party member).
    ///
    /// Called once at battle entry, after both sides' stats are seeded. From
    /// there the base halves are frozen: nothing in a fight writes one except
    /// a Seru side-effect debuff, which is exactly why the stager can use
    /// "base != record" as its "something already moved this stat" gate
    /// (`World::enemy_stat_compare`).
    ///
    /// REF: FUN_80054CB0 (the `sh` pairs at `0x80055160..0x8005530C`; the
    /// routine is ported at
    /// [`legaia_engine_battle::monster_catalog::monster_def_from_record`] and its arithmetic
    /// at [`legaia_engine_battle::monster_catalog::MonsterDef::installed_stats`])
    pub fn sync_battle_stat_bases(&mut self) {
        for slot in 0..self.attack.len() {
            self.attack_base[slot] = self.attack[slot];
            self.defense_base[slot] = self.defense_split[slot];
            self.speed_base[slot] = self.speed[slot];
            self.accuracy_base[slot] = self.accuracy[slot];
        }
    }

    /// Reset every party-member's AP gauge for a new turn. Refills to
    /// `base_ap`, clears the Spirit-charged flag.
    pub fn reset_party_ap(&mut self) {
        for g in self.ap_gauges.iter_mut() {
            g.reset_for_turn();
        }
    }

    /// Set the per-slot weapon attack used by Tactical-Art strike damage
    /// resolution. Engines call this when a character equips / unequips a
    /// weapon, or once at battle init from the active stat record.
    pub fn set_battle_attack(&mut self, slot: u8, atk: u16) {
        if let Some(s) = self.attack.get_mut(slot as usize) {
            *s = atk;
        }
    }

    /// Set the per-slot magic attack scalar used by battle Magic damage
    /// resolution. Engines call this at battle init from the active stat
    /// record's magic stat.
    pub fn set_battle_magic(&mut self, slot: u8, mag: u16) {
        if let Some(s) = self.magic.get_mut(slot as usize) {
            *s = mag;
        }
    }

    /// Set the per-slot generic defense - used when no UDF / LDF split is
    /// configured for the slot.
    pub fn set_battle_defense(&mut self, slot: u8, def: u16) {
        if let Some(s) = self.defense.get_mut(slot as usize) {
            *s = def;
        }
    }

    /// Set per-slot UDF / LDF defense override. Replaces any prior value.
    /// Pass `None` to revert to [`Self::set_battle_defense`].
    pub fn set_battle_defense_split(&mut self, slot: u8, udf_ldf: Option<(u16, u16)>) {
        if let Some(s) = self.defense_split.get_mut(slot as usize) {
            *s = udf_ldf;
        }
    }

    /// Resolve the defense value to use against a single Tactical-Art
    /// strike. Used by the world's `BattleActionHost::apply_art_strike`
    /// impl. Public so engines can call the same lookup directly when
    /// they want to apply art strikes outside the SM (e.g. for testing).
    pub fn resolve_battle_defense(
        &self,
        target_slot: u8,
        info: &legaia_engine_battle_vm::battle_action::ArtStrikeInfo,
    ) -> u16 {
        let idx = target_slot as usize;
        // If we have a UDF / LDF split for the slot, pick the half that
        // matches the strike's power target. Otherwise fall back to the
        // single defense value.
        if let Some(Some((udf, ldf))) = self.defense_split.get(idx)
            && let Some(legaia_art::power::PowerByte::Damage(p)) = info.power
        {
            return match p.target {
                legaia_art::power::PowerTarget::Udf => *udf,
                legaia_art::power::PowerTarget::Ldf => *ldf,
            };
        }
        self.defense.get(idx).copied().unwrap_or(0)
    }
}
