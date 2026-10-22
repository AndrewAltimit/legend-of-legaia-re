//! Party ability bits, the encounter-rate modifiers they feed, and the party
//! battle-stat seed.

use super::*;

impl World {
    /// Rebuild every party member's ability bitfield from their equipped
    /// items, plus the party-global mask.
    ///
    /// PORT: FUN_80042558 (ability-bit rebuild + global-mask OR arms, and
    /// the live HP / MP maxima through [`Self::rebuild_record_live_hp_mp`])
    ///
    /// Mirrors the retail aggregator's bitfield pass: each member's record
    /// `+0xF4` 4×u32 field is zeroed and re-derived from the eight equipment
    /// slots ([`legaia_engine_battle::accessory_passives::AccessoryPassives::bits_for_equipment`]),
    /// then all three records' words - joined or not - OR into the global mask (the engine's
    /// [`legaia_engine_menus::party_state::PartyState::party_ability_mask`], mirroring `DAT_80074358`). The rebuilt
    /// word 0 also lands in [`legaia_engine_menus::party_state::PartyState::character_ability_bits`] - the mask the
    /// MP-cost consumers read (`Self::build_battle_spell_session` /
    /// `cast_spell_on_slots` / the battle-action VM host) - together with the
    /// party-wide-scoped bits any member contributes, so a party-wide passive
    /// is visible through every member's effective mask.
    ///
    /// No-op while [`crate::world::DiscTables::accessory_passives`] is empty (the disc-free
    /// default), so synthetic setups that write `character_ability_bits`
    /// directly keep their values.
    ///
    /// Called from [`Self::seed_party_battle_stats`] (battle entry / stat
    /// refresh) and from the equip-commit paths, mirroring retail's
    /// rebuild-on-every-aggregator-pass behaviour.
    pub fn refresh_party_ability_bits(&mut self) {
        use legaia_engine_battle::accessory_passives::ABILITY_WORDS;
        if self.tables.accessory_passives.is_empty() {
            return;
        }
        let pc = (self.party.party_count.min(3) as usize).min(self.party.roster.members.len());
        // Retail walks the three character records `0x80084708 + k*0x414`,
        // `k = 0..3` (`slti v0,s1,0x3` / `addiu s3,s3,0x414` at
        // `0x80042D8C..0x80042D94`), with no active-member test: a record
        // that has left the party is still rebuilt, and its passives still
        // OR into the global mask (`0x80042D38..0x80042D88`).
        let records = self.party.roster.members.len().min(3);
        let mut by_record = [[0u32; ABILITY_WORDS]; 3];
        let mut global = [0u32; ABILITY_WORDS];
        for (rslot, own_words) in by_record.iter_mut().enumerate().take(records) {
            let equip = self.party.roster.members[rslot].equipment().slots;
            let words = self.tables.accessory_passives.bits_for_equipment(&equip);
            // Rebuild the record-side bitfield (retail zeroes `+0xF4..+0x103`
            // at `0x800425EC..0x800425F8` and re-derives it from the eight
            // equipment bytes `+0x196..`, `0x80042620..0x8004270C`). Bytes
            // past the four words stay zero - passive indices live below 0x40.
            let mut bytes = [0u8; legaia_save::ABILITY_BITS_LEN];
            for (w, word) in words.iter().enumerate() {
                bytes[w * 4..w * 4 + 4].copy_from_slice(&word.to_le_bytes());
            }
            self.party.roster.members[rslot].set_ability_bits(bytes);
            // The same pass rebuilds the record's live HP / MP maxima from
            // its base window under the bits just derived, so an equipment
            // change - the equip screen, a shop's equip-now, a script's
            // `0x3B` - moves the maxima with the accessory. Without it the
            // ending's unequip left a Max HP +10% on a record whose
            // accessory was gone.
            self.rebuild_record_live_hp_mp(rslot);
            for (g, w) in global.iter_mut().zip(words.iter()) {
                *g |= *w;
            }
            *own_words = words;
        }
        // `slot` is the battle ordinal; the bits belong to the character
        // occupying it.
        let mut own = [[0u32; ABILITY_WORDS]; 3];
        for (slot, own_words) in own.iter_mut().enumerate().take(pc) {
            let rslot = self.party_roster_slot(slot);
            if let Some(words) = by_record.get(rslot) {
                *own_words = *words;
            } else if let Some(member) = self.party.roster.members.get(rslot) {
                // A record past the three the pass walks keeps whatever its
                // `+0xF4` words already hold.
                let bytes = member.ability_bits();
                for (w, word) in own_words.iter_mut().enumerate() {
                    if let Some(b) = bytes.get(w * 4..w * 4 + 4) {
                        *word = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                    }
                }
            }
        }
        self.party.party_ability_mask = global;
        // Effective per-member mask for the u32 consumers: own bits plus the
        // party-wide-scoped bits any member contributes (the engine shape of
        // "consumers test the global mask for party-wide passives").
        let pw = self.tables.accessory_passives.party_wide_mask();
        for (bits, own_words) in self
            .party
            .character_ability_bits
            .iter_mut()
            .zip(own.iter())
            .take(pc)
        {
            *bits = own_words[0] | (global[0] & pw[0]);
        }
    }

    /// `true` when any party member's rebuilt ability bitfield carries
    /// passive bit `index` (0..=127; retail uses 0..=0x3F).
    ///
    /// PORT: FUN_800431D0
    ///
    /// The port of retail's global-mask bit test
    /// (`DAT_80074358[index >> 5] & 1 << (index & 0x1F)`), against the
    /// engine's [`legaia_engine_menus::party_state::PartyState::party_ability_mask`]. Point-of-use consumers of the
    /// party-wide passives (encounter rate, escape, battle-end rewards) call
    /// this.
    pub fn party_has_ability(&self, index: u8) -> bool {
        self.party
            .party_ability_mask
            .get((index >> 5) as usize)
            .is_some_and(|w| w & (1u32 << (index & 0x1F)) != 0)
    }

    /// Resolve the current accessory / status encounter-rate modifiers
    /// (`FUN_801D9E1C`'s four pre-roll tests): the High/Low Encounter
    /// passives (ability bits `0x3B` / `0x3C`, via
    /// [`Self::party_has_ability`] = `FUN_800431D0`) and system flags
    /// `0x1D` / `0x1E` (via [`Self::system_flag_test`] = `FUN_8003CE64`).
    /// Refreshed onto the active region tracker before each step roll.
    pub fn encounter_rate_modifiers(
        &self,
    ) -> legaia_engine_battle::region_encounter::EncounterRateModifiers {
        legaia_engine_battle::region_encounter::EncounterRateModifiers {
            high_encounter: self.party_has_ability(0x3B),
            low_encounter: self.party_has_ability(0x3C),
            flag_high: self.system_flag_test(0x1D),
            flag_low: self.system_flag_test(0x1E),
        }
    }

    /// Seed each party combatant's battle attack / defense from the roster's
    /// live stats plus equipped-gear bonuses.
    ///
    /// For every party slot whose roster record carries real stats (live
    /// attack `> 0`), this resolves a [`legaia_engine_battle::battle_stats::BattleStats`] from
    /// the character's base attack / UDF / LDF and the modifiers of the items
    /// in its equipment slots ([`legaia_engine_battle::battle_stats::compute_battle_stats_default`]
    /// against [`crate::world::DiscTables::equipment_table`]), then writes
    /// [`crate::world::BattleState::attack`] (= the attack **without** the equipment sum,
    /// which retail folds per command at swing time - see
    /// [`crate::world::BattleState::equip_atk`]) and
    /// [`crate::world::BattleState::defense_split`] (= resolved UDF / LDF). Slots with a
    /// zeroed roster record are left untouched, so synthetic battles that set
    /// `battle_attack` directly keep their values.
    ///
    /// Called automatically from the live-loop battle entry; also public so a
    /// host can refresh stats after an equipment change without re-entering the
    /// battle.
    pub fn seed_party_battle_stats(&mut self) {
        // Rebuild the per-member ability bitfields (accessory passives) first
        // so both the stat fold below and the MP-cost consumers read fresh
        // equipment-derived bits, mirroring retail's single-pass aggregator.
        self.refresh_party_ability_bits();
        let pc = self.party.party_count.min(3) as usize;
        for slot in 0..pc {
            // `slot` stays the battle ordinal for every live mirror written
            // below; the stats are read off the occupying character's record.
            let Some(rec) = self.party.roster.members.get(self.party_roster_slot(slot)) else {
                continue;
            };
            let live = rec.live_stats();
            // A zeroed roster carries no real stats; don't clobber any value a
            // synthetic battle set directly.
            if live.atk == 0 {
                continue;
            }
            // Per-turn AP budget scales with the character's level (retail base
            // = 4 + level/10, capped 10). Captured before the `ap_gauges`
            // mutable borrow below; `rec` still owns the immutable roster borrow
            // through the stat fold.
            let ap_base = legaia_engine_battle::ap_gauge::ap_base_for_level(rec.level());
            // The MP ceiling the battle surfaces read. `character_max_mp` is
            // the only max-MP the world carries: the battle actor holds
            // current MP and no ceiling, so the battle party bar's MP field,
            // the item screen's target rows and the in-battle MP-restore clamp
            // all read this vector. Captured off the roster record here for the
            // same reason the AP base is - this is the one place every host's
            // party reaches on its way into a fight, and seeding it only from
            // `play-window`'s `--party` branch left every other entry (New
            // Game, `--seed-party`, a loaded save, the browser play page) with
            // a zero ceiling: no MP on the battle bar and an MP potion that
            // restores nothing.
            let record_mp_max = rec.hp_mp_sp().mp_max;
            // Aggregator base stats: prefer the record-side base window
            // (`+0x11C..`, [`legaia_save::RecordStats`]) - the window retail's
            // `FUN_80042558` rebuilds the live stats FROM - so the accessory
            // percent boosts compute from the true base (a record imported
            // from a retail save carries the last rebuilt, already-boosted
            // values in its live window). Fall back to the live window for
            // synthetic records that only populate live stats. Only taken
            // when a passives catalog is installed: without one no percent
            // boost is re-applied, so the live window (which may carry them)
            // remains the better effective source.
            let recs = rec.record_stats();
            let (agl, atk, udf, ldf, spd, int) =
                if !self.tables.accessory_passives.is_empty() && recs.atk != 0 {
                    (recs.agl, recs.atk, recs.udf, recs.ldf, recs.spd, recs.int)
                } else {
                    (live.agl, live.atk, live.udf, live.ldf, live.spd, live.int)
                };
            let base_max_hp = if recs.hp_max != 0 {
                recs.hp_max
            } else {
                rec.hp_mp_sp().hp_max
            };
            let record = legaia_engine_battle::battle_stats::StatRecord {
                base_attack: atk,
                base_udf: udf,
                base_ldf: ldf,
                base_accuracy: agl,
                base_evasion: agl,
                base_spd: spd,
                base_int: int,
                equip: rec.equipment().slots,
            };
            let stats = legaia_engine_battle::battle_stats::compute_battle_stats_with_passives(
                &record,
                &self.tables.equipment_table,
                &self.tables.accessory_passives,
                &[],
                &legaia_engine_battle::battle_stats::StatusModifiers::default(),
            );
            // Max-HP percent passives (indices 0x00 / 0x01) rebuild the
            // effective max HP from the base, capped at 9999 - applied to the
            // live battle actor, the engine's effective-max-HP holder. Only
            // touched when a boost bit is present so synthetic max-HP setups
            // keep their values. (Retail's max-MP boosts, indices 0x02/0x03,
            // have no engine consumer yet: the battle actor carries current
            // MP only, no max-MP mirror.)
            let pwords = self
                .tables
                .accessory_passives
                .bits_for_equipment(&record.equip);
            if pwords[0] & 0x3 != 0 {
                let mut max_hp = base_max_hp;
                if pwords[0] & 0x1 != 0 {
                    max_hp = max_hp.saturating_add(base_max_hp / 10);
                }
                if pwords[0] & 0x2 != 0 {
                    max_hp = max_hp.saturating_add(base_max_hp / 4);
                }
                let max_hp = max_hp.min(9999);
                if let Some(a) = self.actors.get_mut(slot) {
                    a.battle.max_hp = max_hp;
                    // Retail clamps current HP to the rebuilt max.
                    a.battle.hp = a.battle.hp.min(max_hp);
                }
            }
            // ATK is the one equipment-fed line retail does NOT seed with its
            // equipment bonus: `FUN_80053CB8` folds UDF / LDF / SPD from the
            // equipment table and leaves the actor's ATK at the record value.
            // The equipment attack reaches a swing at execution time instead,
            // when `FUN_801EC3E4` adds *half* of the one slot the command
            // reads (footwear for High / Low, slot 2 or 3 for the arm
            // commands, the sum of all five for an art). So the seeded
            // attack is the aggregator's ATK minus the equipment sum - the
            // base plus any accessory percent boost - and the per-slot bytes
            // go to `battle_equip_atk` for `land_melee_hit` to fold per hit.
            let mut equip_atk = [0u8; legaia_engine_battle_vm::battle_formulas::EQUIP_SLOTS];
            let mut equip_atk_sum: u16 = 0;
            for (i, &id) in record.equip.iter().enumerate() {
                if id == 0 {
                    continue;
                }
                if let Some(m) = self.tables.equipment_table.get(id) {
                    let b = m.atk.clamp(0, 255) as u8;
                    equip_atk[i] = b;
                    equip_atk_sum = equip_atk_sum.saturating_add(u16::from(b));
                }
            }
            if let Some(s) = self.battle.attack.get_mut(slot) {
                *s = stats.atk.saturating_sub(equip_atk_sum);
            }
            if let Some(s) = self.battle.equip_atk.get_mut(slot) {
                *s = equip_atk;
            }
            // The `+0x168` halfword is the character's **INT**, not an
            // AGL-derived accuracy: `FUN_80053CB8` loads record `+0x11A`
            // (the live INT, `0x80053F88`) into `+0x16A` and copies it to
            // `+0x168` (`0x800541CC`), with no equipment fold - the loop at
            // `0x80054010..0x80054144` adds only the UDF / LDF / SPD bytes.
            // Every reader treats it as INT: the summon roll's `caster*2`
            // term, the defender roll's `INT*2` against a monster special,
            // the AGL-status roll and selector 9. So the line is the
            // aggregator's INT (base plus any accessory percent boost) less
            // the equipment INT bytes, which retail shows in the menu only.
            let equip_int: i32 = record
                .equip
                .iter()
                .filter(|&&id| id != 0)
                .filter_map(|&id| self.tables.equipment_table.get(id))
                .map(|m| i32::from(m.int))
                .sum();
            let int_line = (i32::from(stats.int) - equip_int).clamp(0, 999) as u16;
            if let Some(s) = self.battle.accuracy.get_mut(slot) {
                *s = int_line;
            }
            if let Some(s) = self.battle.evasion.get_mut(slot) {
                *s = int_line;
            }
            // Resolved SPD (base + the equipment table's footwear bonus). This
            // used to be computed into `StatRecord::base_spd`, folded by the
            // aggregator, and then dropped on the floor - the slot kept the raw
            // `live_stats().spd` that `load_party` / `set_active_party` seeded,
            // so every SPD point a party member's gear granted was invisible to
            // turn order, the formation-advantage roll and the escape roll, all
            // three of which read `battle_speed`.
            if let Some(s) = self.battle.speed.get_mut(slot) {
                *s = stats.spd;
            }
            self.set_battle_defense_split(slot as u8, Some((stats.udf, stats.ldf)));
            // Seed the AP gauge base from the captured level. Base only - the
            // round-start `reset_party_ap` refills `current_ap` to it, and Fury
            // Boost still extends from / reverts to this base. Re-seeding
            // mid-battle (a permanent stat-up item also calls this) is a no-op:
            // the level is unchanged, so the base is rewritten to the same value
            // and the live balance is untouched.
            if let Some(g) = self.battle.ap_gauges.get_mut(slot) {
                g.set_base_ap(ap_base);
            }
            // The action gauge pair `+0x154` / `+0x156` - the pool the arts
            // entry seeds from (`0x801D4E10`) and the gauge a Spirit turn
            // extends at the round boundary (`FUN_801D88CC`). Battle setup
            // writes the base from the character's live AGL `+0x110` and
            // copies it into the live half (`docs/subsystems/level-up.md`);
            // without it every party actor read `0`, so the boundary's
            // Spirit restore extended nothing and the arts entry fell back to
            // the record. A mid-battle refresh moves the live half only when
            // it still sits on the old base, so it never undoes a Spirit
            // extension or a spent gauge.
            if let Some(a) = self.actors.get_mut(slot) {
                let old_base = a.battle.agl_base;
                a.battle.agl_base = live.agl;
                if a.battle.agl == old_base || a.battle.agl == 0 {
                    a.battle.agl = live.agl;
                }
            }
            // Only when the record carries a real ceiling: a zeroed one would
            // clobber a value a synthetic battle set directly, the same reason
            // the `live.atk == 0` guard above skips the whole slot.
            if record_mp_max > 0 {
                self.set_character_max_mp(slot as u8, record_mp_max);
            }
        }
    }
}
