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
        if slot >= self.party_count {
            return Default::default();
        }
        let Some(member) = self
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

    /// Roll the Run command's escape chance - the retail `FUN_801E791C`
    /// formula (the routine battle-action state `0x64` calls, the writer of
    /// the `_DAT_8007726C` outcome pointer). Party score = per-slot
    /// `(SPD*3)>>1 + missingHP>>4` over every party slot (downed included);
    /// enemy score = `SPD + missingHP>>5` over every enemy slot; two 15-bit
    /// rand draws in retail order; the Chicken Heart (Escape Boost, ability
    /// bit 52) and Chicken King (Great Escape, bit 55) accessory bits fold
    /// from the *living* party members' second ability word (`record+0xF8`).
    /// The scripted no-escape flag (`ctx+0x287`) is the engine's
    /// [`World::battle_no_escape`], set at scripted-battle entry
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
        let party_n = (self.party_count as usize).min(self.actors.len());
        let fold = |i: usize| EscapeActor {
            speed: self.battle_speed.get(i).copied().unwrap_or(0),
            hp: self.actors[i].battle.hp,
            max_hp: self.actors[i].battle.max_hp,
        };
        let party: Vec<EscapeActor> = (0..party_n).map(fold).collect();
        let enemies: Vec<EscapeActor> = (party_n..self.actors.len()).map(fold).collect();
        let mut flags = EscapeFlags {
            no_escape: self.battle_no_escape,
            ..EscapeFlags::default()
        };
        for slot in 0..party_n {
            if self.actors[slot].battle.liveness == 0 {
                continue;
            }
            if let Some(member) = self.roster.members.get(self.party_roster_slot(slot)) {
                let bits = member.ability_bits();
                flags.fold_ability_word1(u32::from_le_bytes([bits[4], bits[5], bits[6], bits[7]]));
            }
        }
        let rand = [
            (self.next_rng() & 0x7fff) as u16,
            (self.next_rng() & 0x7fff) as u16,
        ];
        escape_roll(
            escape_party_score(&party),
            escape_enemy_score(&enemies),
            flags,
            rand,
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
        let defender_is_party = defender_slot < self.party_count;
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
    /// [`Self::battle_attack`] / [`Self::battle_magic`] / [`Self::battle_defense`]
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
            .battle_buffs
            .iter()
            .position(|b| b.slot == slot && b.stat == stat)
        {
            let old = self.battle_buffs.remove(pos);
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
        self.battle_buffs.push(BattleBuff {
            slot,
            stat,
            applied_delta,
            turns,
        });
    }

    /// Apply the retail `×6/5` stat-up ramp ([`vm::battle_formulas::buff_ramp`])
    /// to the per-slot scalar backing `stat`, returning the exact `u16` change.
    /// Stats with no live-loop scalar (Accuracy / Evasion / Speed) return `0`.
    fn ramp_buff_scalar(&mut self, slot: u8, stat: crate::spells::BuffStat) -> i16 {
        use crate::spells::BuffStat;
        let scalar = match stat {
            BuffStat::Attack => self.battle_attack.get_mut(slot as usize),
            BuffStat::MagicAttack => self.battle_magic.get_mut(slot as usize),
            BuffStat::Defense | BuffStat::MagicDefense => {
                self.battle_defense.get_mut(slot as usize)
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
    /// return `0`.
    pub(super) fn add_to_buff_scalar(
        &mut self,
        slot: u8,
        stat: crate::spells::BuffStat,
        delta: i16,
    ) -> i16 {
        use crate::spells::BuffStat;
        let scalar = match stat {
            BuffStat::Attack => self.battle_attack.get_mut(slot as usize),
            BuffStat::MagicAttack => self.battle_magic.get_mut(slot as usize),
            BuffStat::Defense | BuffStat::MagicDefense => {
                self.battle_defense.get_mut(slot as usize)
            }
            BuffStat::Accuracy | BuffStat::Evasion | BuffStat::Speed => None,
        };
        let Some(scalar) = scalar else { return 0 };
        let before = *scalar as i32;
        let after = (before + delta as i32).clamp(0, u16::MAX as i32);
        *scalar = after as u16;
        (after - before) as i16
    }

    /// Tick the buffs on `slot` at the start of its turn: decrement each, and
    /// revert + drop those that reach zero.
    pub(in crate::world) fn tick_battle_buffs_on_turn(&mut self, slot: u8) {
        let mut expired: Vec<BattleBuff> = Vec::new();
        self.battle_buffs.retain_mut(|b| {
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
