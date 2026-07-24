//! Seru capture, shiny-enemy roll, summon spell-XP accrual, and seru-registry /
//! trade install. Split out of `battle.rs` as additional `impl World` blocks; no
//! logic change from the original inline definitions.

use super::*;

impl World {
    /// Default chance (percent) that a capturable enemy spawns shiny.
    /// Matches the `--shiny-seru` randomizer default and the rare-encounter
    /// feel the feature is named for.
    pub const DEFAULT_SHINY_CHANCE_PCT: u8 = 2;

    /// Set the per-battle shiny-enemy chance (percent, clamped to `0..=100`).
    /// `0` disables shiny enemies.
    pub fn set_shiny_chance_pct(&mut self, pct: u8) {
        self.shiny_chance_pct = pct.min(100);
    }

    /// Roll for a shiny capturable enemy at battle entry. On a hit (chance
    /// [`Self::shiny_chance_pct`]) one capturable monster slot (a monster the
    /// catalog maps to a Seru) is chosen, its battle stats boosted by
    /// [`crate::seru_learning::SHINY_DAMAGE_BONUS_PCT`]% (HP / ATK / DEF /
    /// SPD), and its slot recorded in [`Self::shiny_enemy_slots`] so a capture
    /// marks the learned spell shiny. Mirrors the retail `--shiny-seru` battle
    /// hook (`FUN_800513F0` → cave routine). Idempotent per battle; the slot
    /// sets are cleared by [`Self::enter_battle_from_formation`] first.
    pub(in crate::world) fn roll_shiny_enemy(&mut self, first_monster: u8) {
        if self.shiny_chance_pct == 0 {
            return;
        }
        if (self.next_rng() % 100) as u8 >= self.shiny_chance_pct {
            return;
        }
        // Gather capturable monster slots (those whose monster id maps to a
        // Seru in the catalog - the only enemies whose capture yields a spell).
        let candidates: Vec<u8> = (first_monster as usize..self.actors.len())
            .filter(|&s| {
                self.actors[s].battle.liveness != 0
                    && self.actors[s].battle.max_hp > 0
                    && self.actors[s]
                        .battle_monster_id
                        .and_then(|mid| self.monster_catalog.get(mid))
                        .is_some_and(|d| d.seru_id.is_some())
            })
            .map(|s| s as u8)
            .collect();
        if candidates.is_empty() {
            return;
        }
        let pick = candidates[(self.next_rng() as usize) % candidates.len()];
        self.boost_shiny_stats(pick);
        self.shiny_enemy_slots.insert(pick);
    }

    /// Apply the shiny +35% stat boost to one battle slot's combat stats.
    fn boost_shiny_stats(&mut self, slot: u8) {
        let pct = crate::seru_learning::SHINY_DAMAGE_BONUS_PCT;
        let bump = |v: u16| (((v as u32) * (100 + pct)) / 100).min(u16::MAX as u32) as u16;
        let s = slot as usize;
        if let Some(a) = self.actors.get_mut(s) {
            a.battle.hp = bump(a.battle.hp);
            a.battle.max_hp = bump(a.battle.max_hp);
        }
        if let Some(v) = self.battle_attack.get_mut(s) {
            *v = bump(*v);
        }
        if let Some(v) = self.battle_defense.get_mut(s) {
            *v = bump(*v);
        }
        if let Some(v) = self.battle_speed.get_mut(s) {
            *v = bump(*v);
        }
    }

    /// Resolve a capture-spell roll against the monster in `target`. The
    /// effective chance scales with the monster's missing-HP fraction (full
    /// `hit_pct` only near death, zero at full HP) - mirroring retail capture,
    /// which is reliable only on a weakened Seru. On success the monster is
    /// downed (so it counts toward the wipe) and its id is logged into
    /// [`Self::battle_captures`] for post-battle Seru learning.
    pub(in crate::world) fn resolve_capture(&mut self, target: u8, hit_pct: u8) {
        let (hp, max, monster_id, alive) = match self.actors.get(target as usize) {
            Some(a) => (
                a.battle.hp as u32,
                a.battle.max_hp as u32,
                a.battle_monster_id,
                a.battle.liveness != 0,
            ),
            None => return,
        };
        if !alive || max == 0 {
            return;
        }
        let missing = max.saturating_sub(hp);
        let effective = (hit_pct as u32 * missing / max).min(100);
        let roll = self.next_rng() % 100;
        if roll >= effective {
            return;
        }
        if let Some(a) = self.actors.get_mut(target as usize) {
            a.battle.hp = 0;
            a.battle.liveness = 0;
        }
        if let Some(id) = monster_id {
            self.battle_captures.push(id);
            // A shiny enemy's capture marks the learned spell shiny (+35%
            // damage forever). Tracked in parallel; resolved in
            // `resolve_captures`.
            if self.shiny_enemy_slots.contains(&target) {
                self.shiny_captures.push(id);
            }
        }
    }

    /// Drain the monster ids captured this battle (see [`Self::battle_captures`]).
    pub fn drain_battle_captures(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.battle_captures)
    }

    /// Install the master [`crate::seru_learning::SeruRegistry`]. Boot wires
    /// this once; `Self::finish_battle` consults it to bank capture points.
    pub fn set_seru_registry(&mut self, registry: crate::seru_learning::SeruRegistry) {
        self.seru_registry = registry;
    }

    /// Install the magic-XP threshold table from `SCUS_942.54` bytes
    /// ([`crate::magic_xp::thresholds_from_scus`], the static table at
    /// `0x8007656C`). Boot wires this once; without it summon casts accrue
    /// spell XP but never level the spell up. Returns whether the table
    /// decoded.
    pub fn install_magic_xp_thresholds(&mut self, scus: &[u8]) -> bool {
        self.magic_xp_thresholds = crate::magic_xp::thresholds_from_scus(scus);
        self.magic_xp_thresholds.is_some()
    }

    /// Install the seru-trade config from `SCUS_942.54` bytes (the randomizer's
    /// `--seru-trade` blob in preserved rodata). Boot wires this once from the
    /// booted disc; returns whether an *enabled* config was found. When absent
    /// or disabled, [`Self::open_seru_trade`] yields `None` and vendors don't
    /// offer trades. See [`crate::seru_trade`].
    pub fn install_seru_trade_config(&mut self, scus: &[u8]) -> bool {
        self.seru_trade_config = legaia_asset::seru_trade::SeruTradeConfig::from_scus(scus);
        self.seru_trade_config.is_some_and(|c| c.enabled)
    }

    /// `true` when seru trading is enabled on this disc.
    pub fn seru_trade_enabled(&self) -> bool {
        self.seru_trade_config.is_some_and(|c| c.enabled)
    }

    /// Open a seru-trade session at `vendor_id` for the current party + play
    /// time. `None` when seru trading isn't enabled. The host renders the
    /// returned [`crate::seru_trade::SeruTradeSession`] and applies a confirmed
    /// trade via [`Self::apply_seru_trade`].
    pub fn open_seru_trade(&self, vendor_id: u16) -> Option<crate::seru_trade::SeruTradeSession> {
        let config = self.seru_trade_config.filter(|c| c.enabled)?;
        Some(crate::seru_trade::SeruTradeSession::open(
            config,
            vendor_id,
            self.play_time_seconds,
            &self.roster.members,
        ))
    }

    /// Apply a confirmed trade to the persistent roster (the spell list the next
    /// battle loads). Returns the outcome; on success the caller should
    /// [`crate::seru_trade::SeruTradeSession::refresh`] its open session so the
    /// offer list reflects the new owned set.
    pub fn apply_seru_trade(
        &mut self,
        offer: &legaia_asset::seru_trade::TradeOffer,
    ) -> crate::seru_trade::TradeResult {
        crate::seru_trade::apply_trade(&mut self.roster.members, offer)
    }

    /// Drain the summon-magic level-up events (`(party_slot, spell_id,
    /// new_level)`) resolved since the last drain - the engine analogue of
    /// the retail level-up banner (`FUN_801e70bc` fires UI element `0x65`).
    pub fn drain_magic_level_ups(&mut self) -> Vec<(u8, u8, u8)> {
        std::mem::take(&mut self.magic_level_ups)
    }

    /// Accrue summon spell-XP for `caster`'s cast of `spell_id` and resolve a
    /// level-up.
    ///
    /// PORT: FUN_801e70bc (summon-magic level-up check, battle overlay 0898;
    /// `ghidra/scripts/funcs/overlay_battle_action_801e70bc.txt`) - find the
    /// cast spell id in the caster record's spell-id list (`+0x13D`, bound
    /// `0x20`), compare the accrued XP (`+0x8` u32 array) against the
    /// threshold table at `0x8007656C` indexed by the spell's level byte
    /// (`+0x161` array), and bump the level (strict `threshold < xp`, level
    /// capped below 9 - kernel
    /// [`vm::battle_formulas::summon_magic_levels_up`]).
    ///
    /// `gain` is the summed per-target accrual from the damage finisher's
    /// spell-XP tail (`FUN_801ddb30`, kernel
    /// [`vm::battle_formulas::summon_spell_xp_gain`]) - the caller computes it
    /// per target hit, mirroring retail's one-finisher-call-per-hit shape.
    ///
    /// Retail re-checks the threshold once per summon return (state `0x36`),
    /// so at most one level is gained per cast - mirrored here. The leveled
    /// byte is what the next cast's magic-power stage reads
    /// ([`Self::caster_magic_power_byte`]). A level-up is recorded in
    /// [`Self::magic_level_ups`] (the banner the retail check fires as UI
    /// element `0x65`).
    ///
    /// Unmodelled retail gates (skips the engine doesn't reproduce): the
    /// per-battle no-reward flag `_DAT_8007BAC0` (scripted fights) and the
    /// unidentified accrual gate `_DAT_8007BDB8`.
    pub(in crate::world) fn accrue_summon_spell_xp(&mut self, caster: u8, spell_id: u8, gain: u32) {
        // Battle ordinal -> the occupying character's record (the XP holder).
        let char_slot = self.party_roster_slot(caster as usize);
        let Some(record) = self.roster.members.get_mut(char_slot) else {
            return;
        };
        let Some(slot) = crate::magic_xp::spell_slot(record, spell_id) else {
            // Retail falls through with slot 0x20 and touches bytes past the
            // arrays; the engine skips a spell the roster doesn't carry.
            return;
        };
        crate::magic_xp::add_spell_xp(record, slot, gain);

        let Some(thresholds) = self.magic_xp_thresholds else {
            return;
        };
        let mut list = record.spell_list();
        let level = list.levels[slot];
        let xp = crate::magic_xp::spell_xp(record, slot);
        if vm::battle_formulas::summon_magic_levels_up(spell_id, level, xp, &thresholds) {
            list.levels[slot] = level + 1;
            record.set_spell_list(list);
            self.magic_level_ups.push((caster, spell_id, level + 1));
            // Retail composes "<spell>'s magic level increased." into the
            // shared message buffer and raises UI element 0x65
            // (`FUN_801F452C`). The engine has one banner channel; the line
            // goes there so the text a host draws is the retail one rather
            // than an engine-invented string.
            let spell_name = self
                .spell_catalog
                .get(spell_id)
                .map(|d| d.name.clone())
                .unwrap_or_else(|| format!("Spell {spell_id:#04X}"));
            self.current_art_banner = Some(crate::tactical_arts::ArtLearnedBanner {
                text: crate::magic_xp::magic_level_increased_message(&spell_name),
                frames_remaining: crate::tactical_arts::ArtLearnedBanner::DEFAULT_FRAMES,
            });
        }
    }

    /// Resolve this battle's captured monsters into Seru-learning progress.
    ///
    /// Drains [`Self::battle_captures`] (so the list is always cleared), maps
    /// each captured monster id to its Seru id via [`Self::monster_catalog`],
    /// and banks capture points against [`Self::seru_log`] for every active
    /// party slot through [`crate::seru_learning::record_capture`]. Any Seru
    /// that crosses its learn threshold adds its spell to the character's
    /// learned list (which [`Self::build_battle_spell_session`] then offers).
    /// Accepted outcomes are stashed in [`Self::last_capture_outcomes`] for the
    /// host to drive the capture / learned banner. Monsters with no Seru, or
    /// any capture when the registry is empty, bank nothing.
    pub(super) fn resolve_captures(&mut self) {
        let captures = std::mem::take(&mut self.battle_captures);
        self.last_capture_outcomes.clear();
        self.current_capture_banner = None;
        if captures.is_empty() || self.seru_registry.is_empty() {
            return;
        }
        // Capture progress banks against CHARACTERS, not battle ordinals -
        // resolve the present party to roster slots before recording.
        let party_slots: Vec<u8> = (0..self.party_count.clamp(1, 3))
            .map(|i| self.party_roster_slot(i as usize) as u8)
            .collect();
        let seru_ids: Vec<u16> = captures
            .iter()
            .filter_map(|&mid| self.monster_catalog.get(mid).and_then(|d| d.seru_id))
            .collect();
        let mut first_accepted: Option<(u16, crate::seru_learning::CaptureOutcome)> = None;
        for sid in seru_ids {
            let outcome = crate::seru_learning::record_capture(
                &self.seru_registry,
                &mut self.seru_log,
                sid,
                &party_slots,
            );
            if outcome.accepted {
                // Retail's learn edge writes the character RECORD: the three
                // parallel spell arrays get the new id prepended at slot 0
                // (`FUN_801E92DC`). The engine's `SeruCaptureLog` list is a
                // read model beside it, so the record-side commit has to
                // happen here or a save round-trip loses the spell's own
                // level / XP slots.
                for learn in &outcome.learns {
                    if let Some(rec) = self.roster.members.get_mut(learn.char_slot as usize) {
                        crate::magic_xp::learn_spell_prepend(rec, learn.spell_id);
                    }
                }
                if first_accepted.is_none() {
                    first_accepted = Some((sid, outcome.clone()));
                }
                self.last_capture_outcomes.push(outcome);
            }
        }
        // Build the host-facing banner for the first accepted capture (a
        // single battle captures at most one Seru in practice). Names resolve
        // the Seru from the registry and the learned spell from the catalog.
        if let Some((sid, outcome)) = first_accepted {
            let seru_name = self
                .seru_registry
                .get(sid)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| format!("Seru {sid:#04X}"));
            let spell_catalog = &self.spell_catalog;
            let banner = crate::seru_learning::SeruCaptureSession::new(
                seru_name,
                sid,
                outcome,
                |char_slot, spell_id| {
                    let char_name = format!("Character {}", char_slot + 1);
                    let spell_name = spell_catalog
                        .get(spell_id)
                        .map(|d| d.name.clone())
                        .unwrap_or_else(|| format!("Spell {spell_id:#04X}"));
                    (char_name, spell_name)
                },
            );
            self.current_capture_banner = Some(banner);
        }

        // Mark shiny: every Seru captured as shiny flags its spell shiny for
        // each eligible party character (the same set `record_capture` awards
        // points to). This persists into the save's LGX4 block and grants the
        // +35% damage on every future cast - whether or not the spell was
        // newly learned this battle (re-capturing a shiny Seru you already
        // know still upgrades it). Mirrors the retail `+0x161` high-bit flag.
        let shiny = std::mem::take(&mut self.shiny_captures);
        if !shiny.is_empty() {
            let party_slots: Vec<u8> = (0..self.party_count.clamp(1, 3))
                .map(|i| self.party_roster_slot(i as usize) as u8)
                .collect();
            for mid in shiny {
                // Resolve to (spell_id, eligible slots) as owned data before
                // mutating the log (the registry borrow can't overlap the
                // mark_shiny &mut self).
                let resolved = self
                    .monster_catalog
                    .get(mid)
                    .and_then(|d| d.seru_id)
                    .and_then(|sid| self.seru_registry.get(sid))
                    .map(|seru| {
                        let elig: Vec<u8> = party_slots
                            .iter()
                            .copied()
                            .filter(|&slot| seru.can_be_learned_by(slot))
                            .collect();
                        (seru.spell_id, elig)
                    });
                if let Some((spell_id, elig)) = resolved {
                    for slot in elig {
                        self.seru_log.mark_shiny(slot, spell_id);
                    }
                }
            }
        }
    }

    /// Drain the capture outcomes from the most recently finished battle.
    pub fn drain_last_capture_outcomes(&mut self) -> Vec<crate::seru_learning::CaptureOutcome> {
        std::mem::take(&mut self.last_capture_outcomes)
    }
}
