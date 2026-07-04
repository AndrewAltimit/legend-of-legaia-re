//! Item/spell/art catalogs, tactical-arts rows, item use, stat raises, battle stat setters, XP/loot/steal, and shop flow.
//!
//! Split out of `world.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

impl World {
    /// Set the item catalog the battle / field menu consults for item
    /// actions. Replaces any prior catalog. Engines populate this at
    /// boot time (typically from the vanilla catalog).
    ///
    /// When a real on-disc item-effect table has been installed via
    /// [`Self::set_item_effects`], its field/battle usability flags are applied
    /// onto the new catalog so the item-menu gating matches retail.
    pub fn set_item_catalog(&mut self, catalog: crate::items::ItemCatalog) {
        let mut catalog = catalog;
        if let Some(table) = &self.item_effects {
            catalog.apply_effect_flags(table);
            catalog.apply_stat_items(table);
            catalog.apply_buff_items(table);
            catalog.apply_action_gauge_items(table);
        }
        self.item_catalog = catalog;
    }

    /// Install the real on-disc item-effect descriptor table. Subsequent
    /// [`Self::set_item_catalog`] calls apply its usability flags; this also
    /// re-applies them to the catalog already installed.
    pub fn set_item_effects(&mut self, table: legaia_asset::item_effect::ItemEffectTable) {
        self.item_catalog.apply_effect_flags(&table);
        self.item_catalog.apply_stat_items(&table);
        self.item_catalog.apply_buff_items(&table);
        self.item_catalog.apply_action_gauge_items(&table);
        self.item_effects = Some(table);
    }

    /// Install the spell catalog used by the player-driven battle Magic
    /// submenu. Engines call this at battle init (commonly
    /// [`crate::spells::SpellCatalog::vanilla`]).
    pub fn set_spell_catalog(&mut self, catalog: crate::spells::SpellCatalog) {
        self.spell_catalog = catalog;
    }

    /// Stage one decoded art record for the player-driven battle Arts submenu,
    /// keyed by `(character, art constant)`. Engines call this at battle init
    /// for every art the party can run (parsed from disc PROT entry `0x05C4`)
    /// so a saved chain ending in that art deals its real per-strike power.
    pub fn set_art_record(
        &mut self,
        character: legaia_art::Character,
        action: legaia_art::ActionConstant,
        record: legaia_art::ArtRecord,
    ) {
        self.art_records.insert((character, action), record);
    }

    /// Bulk-install art records (see [`World::set_art_record`]). Existing
    /// entries for the same key are replaced.
    pub fn set_art_records(
        &mut self,
        records: impl IntoIterator<
            Item = (
                (legaia_art::Character, legaia_art::ActionConstant),
                legaia_art::ArtRecord,
            ),
        >,
    ) {
        self.art_records.extend(records);
    }

    /// Resolve a party slot to the [`legaia_art::Character`] whose art tables
    /// apply. Party slots 0/1/2 are Vahn/Noa/Gala; out-of-range slots (story
    /// guests, monsters) fall back to Vahn so the lookup never panics.
    pub(crate) fn caster_character(&self, slot: u8) -> legaia_art::Character {
        crate::battle_arts::character_for_slot(slot)
    }

    /// Build the Arts submenu rows for `caster` from their saved chains. For
    /// each chain, the longest staged art record whose command string the
    /// chain ends with ([`crate::battle_arts::chain_matches_record`]) supplies
    /// the real power profile; chains with no matching record fall back to a
    /// synthetic profile derived from the directional commands.
    pub(crate) fn build_battle_arts_rows(&self, caster: u8) -> Vec<crate::battle_arts::ArtRow> {
        use crate::battle_arts::{
            ArtRow, chain_matches_record, miracle_for_chain, power_from_record, super_for_chain,
            synthetic_power,
        };
        // `caster` is the battle ordinal; chains + art catalog belong to the
        // occupying CHARACTER (roster slot per the present-party composition).
        let char_slot = self.party_roster_slot(caster as usize) as u8;
        let character = self.caster_character(char_slot);
        self.saved_chains
            .iter()
            .filter(|c| c.char_slot == char_slot)
            .map(|c| {
                // Miracle Arts win over a plain art-record match: a chain whose
                // directional string is the caster's Miracle Art replaces the
                // whole queue with the finisher sequence (the retail order:
                // Miracle replacement runs before any tail Super expansion).
                if let Some(miracle) = miracle_for_chain(character, &c.sequence) {
                    let (power, enemy_effect) = self.miracle_strike_profile(character, miracle);
                    return ArtRow {
                        name: c.name.clone(),
                        power,
                        enemy_effect,
                        miracle: Some(miracle.name),
                        super_art: None,
                    };
                }
                // Super Arts next (after Miracle, matching the retail order):
                // recognize the chain's named-art sequence from the caster's
                // art catalog and tail-match it against the caster's Super art
                // sequences (connectors abstracted - see `super_for_chain`).
                let caster_records = || {
                    self.art_records
                        .iter()
                        .filter(|((ch, _), _)| *ch == character)
                        .map(|(_, rec)| rec)
                };
                if let Some(sa) = super_for_chain(character, &c.sequence, caster_records()) {
                    let (power, enemy_effect) = self.super_strike_profile(character, sa);
                    return ArtRow {
                        name: c.name.clone(),
                        power,
                        enemy_effect,
                        miracle: None,
                        super_art: Some(sa.name),
                    };
                }
                let best = self
                    .art_records
                    .iter()
                    .filter(|((ch, _), _)| *ch == character)
                    .filter(|(_, rec)| chain_matches_record(&c.sequence, rec))
                    .max_by_key(|(_, rec)| rec.commands.len());
                match best {
                    Some((_, rec)) => {
                        let (power, enemy_effect) = power_from_record(rec);
                        ArtRow {
                            name: c.name.clone(),
                            power,
                            enemy_effect,
                            miracle: None,
                            super_art: None,
                        }
                    }
                    None => ArtRow {
                        name: c.name.clone(),
                        power: synthetic_power(&c.sequence),
                        enemy_effect: legaia_art::EnemyEffect::None,
                        miracle: None,
                        super_art: None,
                    },
                }
            })
            .collect()
    }

    /// Resolve a Miracle Art's per-strike power profile from its
    /// finisher-replacement queue. Runs the canonical command resolution
    /// ([`legaia_engine_vm::battle_action::resolve_action_queue`]), which
    /// replaces the directional input with the Miracle's component-art queue,
    /// then turns each art constant in that queue into strikes:
    ///
    /// - if the `(character, art)` record is staged ([`Self::set_art_record`]),
    ///   the art contributes its real damage power bytes + status effect;
    /// - otherwise it contributes one tier-0 (`x12`) synthetic strike, the same
    ///   graceful-degradation profile [`crate::battle_arts::synthetic_power`]
    ///   uses when no disc art data is loaded.
    ///
    /// The first staged component art's status effect is adopted for the whole
    /// finisher. Result is clamped to [`crate::battle_arts::MAX_ART_HITS`] and
    /// floored at one strike.
    fn miracle_strike_profile(
        &self,
        character: legaia_art::Character,
        miracle: &legaia_art::MiracleArt,
    ) -> (Vec<legaia_art::power::PowerByte>, legaia_art::EnemyEffect) {
        let queue =
            legaia_engine_vm::battle_action::resolve_action_queue(character, miracle.commands, &[]);
        self.art_actions_strike_profile(character, queue.actions().iter().copied())
    }

    /// Resolve a **Super Art**'s per-strike power profile from its
    /// finisher-replacement queue ([`legaia_art::SuperArt::replace`]). The
    /// replacement keeps the leading component arts and ends in the Super
    /// finisher constant(s) (e.g. Tri-Somersault → `… 1A 2B 2B 2B`), so each art
    /// constant in it contributes a strike via the shared resolver
    /// ([`Self::art_actions_strike_profile`]) - real [`ArtRecord`] power where the
    /// `(character, art)` record is staged, else a tier-0 synthetic strike.
    ///
    /// [`ArtRecord`]: legaia_art::ArtRecord
    fn super_strike_profile(
        &self,
        character: legaia_art::Character,
        sa: &legaia_art::SuperArt,
    ) -> (Vec<legaia_art::power::PowerByte>, legaia_art::EnemyEffect) {
        let actions = sa
            .replace
            .iter()
            .filter_map(|&b| legaia_art::ActionConstant::from_byte(b));
        self.art_actions_strike_profile(character, actions)
    }

    /// Turn a queue of [`ActionConstant`](legaia_art::ActionConstant)s into a
    /// per-strike power profile: each art constant resolves to its staged
    /// [`ArtRecord`](legaia_art::ArtRecord) power bytes + status effect, or one
    /// tier-0 (`x12`) synthetic strike when that art's record isn't loaded (the
    /// graceful-degradation fallback the no-disc-data path uses). The first
    /// staged status effect is adopted for the whole finisher; the result is
    /// clamped to [`crate::battle_arts::MAX_ART_HITS`] and floored at one strike.
    /// Shared by the Miracle and Super finisher resolvers.
    fn art_actions_strike_profile(
        &self,
        character: legaia_art::Character,
        actions: impl Iterator<Item = legaia_art::ActionConstant>,
    ) -> (Vec<legaia_art::power::PowerByte>, legaia_art::EnemyEffect) {
        use crate::battle_arts::MAX_ART_HITS;
        use legaia_art::power::PowerByte;
        // Synthetic UDF x12 - the tier-0 high strike a component art with no
        // staged record degrades to.
        const SYNTH_UDF_X12: u8 = 0x16;

        let mut power: Vec<PowerByte> = Vec::new();
        let mut enemy_effect = legaia_art::EnemyEffect::None;
        for action in actions {
            if !action.is_art() {
                continue;
            }
            match self.art_records.get(&(character, action)) {
                Some(rec) => {
                    let (mut bytes, effect) = crate::battle_arts::power_from_record(rec);
                    if enemy_effect == legaia_art::EnemyEffect::None {
                        enemy_effect = effect;
                    }
                    power.append(&mut bytes);
                }
                None => power.push(PowerByte::from_byte(SYNTH_UDF_X12)),
            }
            if power.len() >= MAX_ART_HITS as usize {
                break;
            }
        }
        power.truncate(MAX_ART_HITS as usize);
        if power.is_empty() {
            power.push(PowerByte::from_byte(SYNTH_UDF_X12));
        }
        (power, enemy_effect)
    }

    /// Pull the cross-character saved-chain library out as a
    /// [`crate::tactical_arts_editor::ChainLibrary`] - what the field menu's
    /// Tactical Arts editor browses + edits. The editor mutates the returned
    /// library; the engine writes the result back with
    /// [`Self::store_chain_library`] so the edit reaches the next battle's
    /// Arts menu (via `Self::build_battle_arts_rows`) and the next save
    /// (via [`Self::saved_chains`]).
    pub fn chain_library(&self) -> crate::tactical_arts_editor::ChainLibrary {
        crate::tactical_arts_editor::ChainLibrary::from_records(&self.saved_chains)
    }

    /// Write an edited [`crate::tactical_arts_editor::ChainLibrary`] back into
    /// [`Self::saved_chains`], replacing the whole library. This is the bridge
    /// that closes the loop the field menu opens with [`Self::chain_library`]:
    /// once stored, a chain composed in the editor is selectable in battle and
    /// persists across `save_full` / `load_full`.
    pub fn store_chain_library(&mut self, lib: &crate::tactical_arts_editor::ChainLibrary) {
        self.saved_chains = lib.to_records();
    }

    /// Use an item from the catalog against a target slot. Wraps the
    /// `items::apply_effect` resolution and folds the outcome back into
    /// world state (HP/MP deltas, status cure, revive HP). Returns the
    /// resolved [`crate::items::ItemOutcome`] so the engine can drive
    /// dialog / SFX / visual cues.
    ///
    /// Returns [`crate::items::ItemOutcome::NoEffect`] when:
    ///   - the item id is not in the catalog,
    ///   - or the target slot is out of range.
    ///
    /// HP / MP changes are clamped to the actor's max values. Cure /
    /// CureAll outcomes also clear the corresponding entries from the
    /// `StatusEffectTracker`.
    pub fn use_item(&mut self, item_id: u8, target_slot: u8) -> crate::items::ItemOutcome {
        let entry = match self.item_catalog.get(item_id) {
            Some(e) => *e,
            None => return crate::items::ItemOutcome::NoEffect,
        };
        // Permanent multi-stat boost (class-6 Water line): resolved from the
        // on-disc effect table (which this path needs anyway), not the pure
        // table-less `apply_effect`.
        if let crate::items::ItemEffect::StatUp = entry.effect {
            return self.apply_stat_up_item(item_id, target_slot);
        }
        // One-battle stat buff (class-7 Elixir): ramps the target's battle-actor
        // stat scalars by ×6/5, resolved from the on-disc table.
        if let crate::items::ItemEffect::BattleBuff = entry.effect {
            return self.apply_buff_item(item_id, target_slot);
        }
        // One-battle action-gauge extension (class-5 Fury Boost): extends the
        // target's AP gauge for the rest of the battle.
        if let crate::items::ItemEffect::ActionGauge = entry.effect {
            return self.apply_fury_boost_item(target_slot);
        }
        let idx = target_slot as usize;
        // BattleActor holds `mp` but not `max_mp`; engines that wire the
        // character record into the actor populate it via a sibling field.
        // For the snapshot we use the character_max_mp accessor (defaults
        // to `mp` itself when not separately tracked, which gives a
        // conservative "MP already capped" reading).
        let status_mask = self
            .status_effects
            .statuses(target_slot)
            .iter()
            .fold(0u8, |m, s| m | crate::items::status_bit(s.kind));
        let snapshot = match self.actors.get(idx) {
            Some(a) => crate::items::TargetSnapshot {
                hp: a.battle.hp,
                hp_max: a.battle.max_hp,
                mp: a.battle.mp,
                mp_max: self
                    .character_max_mp
                    .get(idx)
                    .copied()
                    .unwrap_or(a.battle.mp),
                is_dead: a.battle.hp == 0 && a.battle.max_hp > 0,
                status_mask,
            },
            None => return crate::items::ItemOutcome::NoEffect,
        };
        let outcome = crate::items::apply_effect(entry.effect, &snapshot);
        match outcome {
            crate::items::ItemOutcome::HealedHp { amount } => {
                if let Some(a) = self.actors.get_mut(idx) {
                    a.battle.hp = a.battle.hp.saturating_add(amount).min(a.battle.max_hp);
                }
            }
            crate::items::ItemOutcome::HealedMp { amount } => {
                if let Some(a) = self.actors.get_mut(idx) {
                    let cap = self.character_max_mp.get(idx).copied().unwrap_or(u16::MAX);
                    a.battle.mp = a.battle.mp.saturating_add(amount).min(cap);
                }
            }
            crate::items::ItemOutcome::Cured { kind } => {
                self.status_effects.cure(target_slot, kind);
            }
            crate::items::ItemOutcome::CuredAll => {
                self.status_effects.cure_all(target_slot);
            }
            crate::items::ItemOutcome::Revived { hp_after } => {
                if let Some(a) = self.actors.get_mut(idx) {
                    a.battle.hp = hp_after.min(a.battle.max_hp);
                }
            }
            crate::items::ItemOutcome::SpiritGained { amount } if idx < self.ap_gauges.len() => {
                // Refund AP into the active actor's gauge if it's a party slot.
                self.ap_gauges[idx].refund(amount);
            }
            crate::items::ItemOutcome::DamageDealt { amount } => {
                // Offensive item (e.g. Bomb): subtract HP from the enemy slot
                // and down it if it reaches zero.
                if let Some(a) = self.actors.get_mut(idx) {
                    a.battle.hp = a.battle.hp.saturating_sub(amount);
                    if a.battle.hp == 0 {
                        a.battle.liveness = 0;
                    }
                }
            }
            crate::items::ItemOutcome::CaptureRolled { strength } => {
                // Capture item: roll against the enemy's missing-HP fraction
                // (shared with the Magic capture path); a success downs the
                // monster and logs its id into `battle_captures`.
                self.resolve_capture(target_slot, strength.min(u8::MAX as u16) as u8);
            }
            crate::items::ItemOutcome::EscapeRequested => {
                // Escape item (e.g. Goblin Foot): flag the encounter to end;
                // the battle item-menu tick returns to the field.
                self.battle_escaped = true;
            }
            crate::items::ItemOutcome::StatRaised { target, delta } => {
                // Permanent stat-up consumable (Power Tonic, Vital Tonic, ...):
                // raise the persistent roster record and refresh the live
                // derived values so the gain shows immediately and survives a
                // save. These items are field-only.
                self.apply_stat_raise(idx, target, delta);
            }
            _ => {}
        }
        outcome
    }

    /// Apply a one-battle action-gauge extension
    /// ([`crate::items::ItemEffect::ActionGauge`], Fury Boost) to party
    /// `target_slot`. Retail sets the actor `+0x1F9` flag, and the action-SM
    /// gauge-build phase then sizes the gauge as `gauge_stat * 7 / 5 + 8`
    /// (clamped) instead of the base length. The engine models the AP gauge as a
    /// discrete per-turn budget rather than a continuous pixel length, so it
    /// approximates the extension by raising the slot's [`ApGauge::base_ap`] by
    /// the retail `×7/5` ratio (the `+8` pixel term and the gauge-stat source are
    /// not representable here). The boost persists for the battle (`base_ap`
    /// survives [`ApGauge::reset_for_turn`]) and the live gauge gains the delta
    /// immediately; it is reverted at battle end ([`Self::finish_battle`]).
    /// Idempotent within a battle (a second Fury Boost re-sets the already-set
    /// flag, no extra gauge). Returns [`crate::items::ItemOutcome::NoEffect`] for
    /// a non-party slot.
    fn apply_fury_boost_item(&mut self, target_slot: u8) -> crate::items::ItemOutcome {
        let idx = target_slot as usize;
        if idx >= self.ap_gauges.len() {
            return crate::items::ItemOutcome::NoEffect;
        }
        // Already boosted this battle: retail re-sets the same flag, no compound.
        if self.fury_boost[idx].is_none() {
            let gauge = &mut self.ap_gauges[idx];
            let before = gauge.base_ap;
            let after = ((before as u16 * 7) / 5) as u8;
            let delta = after.saturating_sub(before);
            gauge.set_base_ap(after);
            // Extend the live gauge so the longer budget is usable this turn.
            gauge.current_ap = gauge.current_ap.saturating_add(delta);
            self.fury_boost[idx] = Some(delta);
        }
        crate::items::ItemOutcome::ActionGaugeExtended
    }

    /// Apply a one-battle stat buff ([`crate::items::ItemEffect::BattleBuff`],
    /// the class-7 Elixirs) to `target_slot`. The buffed stats are resolved from
    /// the installed on-disc item-effect table; each is ramped ×6/5 for the rest
    /// of the battle through the shared buff path ([`Self::apply_battle_buff`],
    /// the same machinery as buff *spells*) - so it reuses the precise
    /// revert-on-expiry / revert-at-battle-end bookkeeping. `Defense` ramps the
    /// single defence scalar; `Agility` maps to the accuracy/evasion proxy (no
    /// live scalar yet, so it only runs the turn timer, like a buff spell on
    /// Speed). Returns [`crate::items::ItemOutcome::NoEffect`] when no table is
    /// installed or the id isn't a one-battle buff.
    fn apply_buff_item(&mut self, item_id: u8, target_slot: u8) -> crate::items::ItemOutcome {
        use crate::spells::BuffStat;
        use legaia_asset::item_effect::{StatItemEffect, StatTarget};
        // "One battle": a turn count large enough to outlast the encounter; the
        // buff is reverted wholesale at battle end (`finish_battle`).
        const ONE_BATTLE: u8 = u8::MAX;
        // Positive magnitude selects the retail ×6/5 multiplicative ramp in
        // `apply_battle_buff` (the value itself is only a sign hint there).
        const BUFF_SIGN: i16 = 1;

        let resolved = self
            .item_effects
            .as_ref()
            .and_then(|t| t.stat_effect(item_id));
        let Some(StatItemEffect::BuffOneBattle(stats)) = resolved else {
            return crate::items::ItemOutcome::NoEffect;
        };
        let mut count = 0u8;
        for stat in stats {
            let buff_stat = match stat {
                StatTarget::Attack => BuffStat::Attack,
                StatTarget::Defense => BuffStat::Defense,
                StatTarget::Speed => BuffStat::Speed,
                // AGL drives accuracy + evasion (both proxy it); one entry
                // models the AGL buff.
                StatTarget::Agility => BuffStat::Accuracy,
                // The permanent-only stats never appear in a class-7 buff; skip
                // defensively rather than fabricate a battle scalar for them.
                StatTarget::MaxHp | StatTarget::MaxMp | StatTarget::Intelligence => continue,
            };
            self.apply_battle_buff(target_slot, buff_stat, BUFF_SIGN, ONE_BATTLE);
            count = count.saturating_add(1);
        }
        if count == 0 {
            crate::items::ItemOutcome::NoEffect
        } else {
            crate::items::ItemOutcome::Buffed { count }
        }
    }

    /// Apply a permanent multi-stat boost ([`crate::items::ItemEffect::StatUp`],
    /// the class-6 *Water* line) to party slot `target_slot`. The per-stat
    /// changes are resolved from the installed on-disc item-effect table via the
    /// item's `(class, tier)` descriptor (`legaia_asset::item_effect`), then each
    /// is applied through the shared [`Self::apply_stat_raise`] persistent path.
    /// `Defense` raises both defence facets (DEF-up + DEF-down), matching the
    /// retail apply handler. Returns [`crate::items::ItemOutcome::NoEffect`] when
    /// no table is installed or the id isn't a permanent stat-up.
    fn apply_stat_up_item(&mut self, item_id: u8, target_slot: u8) -> crate::items::ItemOutcome {
        use crate::items::StatBoostTarget as T;
        use legaia_asset::item_effect::{StatItemEffect, StatTarget};
        let idx = target_slot as usize;
        // Resolve to an owned value so the immutable table borrow is dropped
        // before the mutable `apply_stat_raise` calls.
        let resolved = self
            .item_effects
            .as_ref()
            .and_then(|t| t.stat_effect(item_id));
        let Some(StatItemEffect::Permanent(changes)) = resolved else {
            return crate::items::ItemOutcome::NoEffect;
        };
        let mut raises: Vec<(T, u16)> = Vec::with_capacity(changes.len() + 1);
        for ch in &changes {
            match ch.stat {
                StatTarget::MaxHp => raises.push((T::HpMax, ch.delta)),
                StatTarget::MaxMp => raises.push((T::MpMax, ch.delta)),
                StatTarget::Attack => raises.push((T::Attack, ch.delta)),
                // The retail handler writes both defence facets for one item.
                StatTarget::Defense => {
                    raises.push((T::Udf, ch.delta));
                    raises.push((T::Ldf, ch.delta));
                }
                StatTarget::Speed => raises.push((T::Speed, ch.delta)),
                StatTarget::Intelligence => raises.push((T::Intelligence, ch.delta)),
                StatTarget::Agility => raises.push((T::Agility, ch.delta)),
            }
        }
        let count = raises.len().min(u8::MAX as usize) as u8;
        for (target, delta) in raises {
            self.apply_stat_raise(idx, target, delta);
        }
        if count == 0 {
            crate::items::ItemOutcome::NoEffect
        } else {
            crate::items::ItemOutcome::StatsRaised { count }
        }
    }

    /// Apply a permanent [`crate::items::ItemOutcome::StatRaised`] to party
    /// slot `idx`: mutate the persistent character record and re-derive the
    /// live battle stats. HP/MP-max raises bump the live actor's caps (and
    /// current values) too; combat-stat raises land in the `+0x110` live-stat
    /// block that [`Self::seed_party_battle_stats`] reads.
    ///
    /// The exact retail cap / refill rules for stat-up consumables are not
    /// byte-pinned (the items are field-only and absent from the captured
    /// battle traces), so the engine uses self-consistent rules: combat stats
    /// cap at the record's per-stat cap constant (fallback `999`), HP/MP max
    /// cap at `9999`, and a max raise refills the gained amount.
    fn apply_stat_raise(&mut self, idx: usize, target: crate::items::StatBoostTarget, delta: u16) {
        use crate::items::StatBoostTarget as T;
        const STAT_CAP_FALLBACK: u16 = 999;
        const HPMP_CAP: u16 = 9999;
        if self.roster.members.get(idx).is_none() {
            return;
        }
        match target {
            T::HpMax => {
                {
                    let rec = &mut self.roster.members[idx];
                    let mut hms = rec.hp_mp_sp();
                    hms.hp_max = hms.hp_max.saturating_add(delta).min(HPMP_CAP);
                    hms.hp_cur = hms.hp_cur.saturating_add(delta).min(hms.hp_max);
                    rec.set_hp_mp_sp(hms);
                    let mut rs = rec.record_stats();
                    rs.hp_max = rs.hp_max.saturating_add(delta).min(HPMP_CAP);
                    rec.set_record_stats(rs);
                }
                if let Some(a) = self.actors.get_mut(idx) {
                    a.battle.max_hp = a.battle.max_hp.saturating_add(delta).min(HPMP_CAP);
                    a.battle.hp = a.battle.hp.saturating_add(delta).min(a.battle.max_hp);
                }
            }
            T::MpMax => {
                let new_max;
                {
                    let rec = &mut self.roster.members[idx];
                    let mut hms = rec.hp_mp_sp();
                    hms.mp_max = hms.mp_max.saturating_add(delta).min(HPMP_CAP);
                    hms.mp_cur = hms.mp_cur.saturating_add(delta).min(hms.mp_max);
                    rec.set_hp_mp_sp(hms);
                    let mut rs = rec.record_stats();
                    rs.mp_max = rs.mp_max.saturating_add(delta).min(HPMP_CAP);
                    rec.set_record_stats(rs);
                    new_max = hms.mp_max;
                }
                self.set_character_max_mp(idx as u8, new_max);
                if let Some(a) = self.actors.get_mut(idx) {
                    a.battle.mp = a.battle.mp.saturating_add(delta).min(new_max);
                }
            }
            // Combat stats live in the +0x110 block the stat resolver reads.
            // Accuracy + Evasion both derive from AGL there, so both land on
            // AGL (matching `seed_party_battle_stats`), as does the AGL raise
            // itself; Speed and Intelligence have their own halfwords.
            T::Attack
            | T::Udf
            | T::Ldf
            | T::Accuracy
            | T::Evasion
            | T::Agility
            | T::Speed
            | T::Intelligence => {
                {
                    let rec = &mut self.roster.members[idx];
                    let cap = match rec.record_stats().cap_constant {
                        0 => STAT_CAP_FALLBACK,
                        c => c,
                    };
                    let mut ls = rec.live_stats();
                    let bump = |v: u16| v.saturating_add(delta).min(cap);
                    match target {
                        T::Attack => ls.atk = bump(ls.atk),
                        T::Udf => ls.udf = bump(ls.udf),
                        T::Ldf => ls.ldf = bump(ls.ldf),
                        T::Accuracy | T::Evasion | T::Agility => ls.agl = bump(ls.agl),
                        T::Speed => ls.spd = bump(ls.spd),
                        T::Intelligence => ls.int = bump(ls.int),
                        T::HpMax | T::MpMax => {}
                    }
                    rec.set_live_stats(ls);
                }
                self.seed_party_battle_stats();
            }
        }
    }

    /// Set per-slot character max MP (mirrors `char_record[+0x140]`
    /// from the save record). Engines call this once per scene init -
    /// usually from `set_character_record_for_slot`. Unset slots default
    /// to `0`, which makes [`Self::use_item`] treat MP healing as a
    /// no-op for that slot.
    pub fn set_character_max_mp(&mut self, slot: u8, mp_max: u16) {
        let i = slot as usize;
        if i >= self.character_max_mp.len() {
            self.character_max_mp.resize(i + 1, 0);
        }
        self.character_max_mp[i] = mp_max;
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
        if let Some(s) = self.battle_attack.get_mut(slot as usize) {
            *s = atk;
        }
    }

    /// Set the per-slot magic attack scalar used by battle Magic damage
    /// resolution. Engines call this at battle init from the active stat
    /// record's magic stat.
    pub fn set_battle_magic(&mut self, slot: u8, mag: u16) {
        if let Some(s) = self.battle_magic.get_mut(slot as usize) {
            *s = mag;
        }
    }

    /// Set the per-slot generic defense - used when no UDF / LDF split is
    /// configured for the slot.
    pub fn set_battle_defense(&mut self, slot: u8, def: u16) {
        if let Some(s) = self.battle_defense.get_mut(slot as usize) {
            *s = def;
        }
    }

    /// Set per-slot UDF / LDF defense override. Replaces any prior value.
    /// Pass `None` to revert to [`Self::set_battle_defense`].
    pub fn set_battle_defense_split(&mut self, slot: u8, udf_ldf: Option<(u16, u16)>) {
        if let Some(s) = self.battle_defense_split.get_mut(slot as usize) {
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
        info: &legaia_engine_vm::battle_action::ArtStrikeInfo,
    ) -> u16 {
        let idx = target_slot as usize;
        // If we have a UDF / LDF split for the slot, pick the half that
        // matches the strike's power target. Otherwise fall back to the
        // single defense value.
        if let Some(Some((udf, ldf))) = self.battle_defense_split.get(idx)
            && let Some(legaia_art::power::PowerByte::Damage(p)) = info.power
        {
            return match p.target {
                legaia_art::power::PowerTarget::Udf => *udf,
                legaia_art::power::PowerTarget::Ldf => *ldf,
            };
        }
        self.battle_defense.get(idx).copied().unwrap_or(0)
    }

    /// Distribute `xp_reward` (the summed enemy EXP) to the surviving party
    /// members after a `BattleEndCause::MonsterWipe`. Mirrors the retail split
    /// ([`vm::battle_formulas::victory_exp_per_member`], `FUN_8004E568`):
    ///
    /// - The summed reward is scaled by 3/4 (`v - (v >> 2)`), then **ceiling**-
    ///   divided among the surviving (HP > 0) members - not a floor-divide of
    ///   the raw sum.
    /// - Dead members (HP == 0) receive zero XP and are excluded from the divisor.
    ///
    /// For each member that crosses a level threshold, bumps the roster
    /// record's HP/MP maxima, resyncs the live `BattleActor` mirror, pushes
    /// a [`BattleEvent::LevelUp`], and appends a [`LevelUpResult`] to the
    /// returned vec.
    ///
    /// If every party member is dead (TPK) but the caller still invokes this
    /// (e.g. a Phoenix Down style revive-after-victory), the split degenerates
    /// to a no-op - there are no alive recipients.
    pub fn apply_battle_xp(&mut self, xp_reward: u32) -> Vec<LevelUpResult> {
        let party_count = self.party_count as usize;
        // Living-member count drives the divisor. We pull HP from
        // `BattleActor` (the live mirror) so the resolver sees the
        // post-battle state, not the record's saved HP.
        let alive: Vec<u8> = (0..party_count as u8)
            .filter(|&i| self.actors.get(i as usize).is_some_and(|a| a.battle.hp > 0))
            .collect();
        if alive.is_empty() {
            return Vec::new();
        }
        // Retail scales the summed EXP by 3/4 then ceiling-divides among the
        // living members (`FUN_8004E568` `8004e568.txt:461`), NOT a plain
        // floor-divide of the raw sum.
        let per_member_xp =
            vm::battle_formulas::victory_exp_per_member(xp_reward, alive.len() as u32);
        if per_member_xp == 0 {
            return Vec::new();
        }
        let mut results = Vec::new();
        for member in alive {
            // XP / level state belongs to the CHARACTER (roster slot), while
            // the live HP/MP resync targets the battle ordinal's actor mirror.
            let char_id = self.party_roster_slot(member as usize) as u8;
            let result = self.level_up_tracker.grant_xp(char_id, per_member_xp);
            // Retail (`FUN_801E9504`) maintains the record's cumulative XP
            // (+0x0), next-level threshold (+0x4, slots-1/2 corrected), and
            // displayed-level byte (+0x130) on every grant; the Status menu
            // (`FUN_801D33D8`) draws those words verbatim. Mirror them so the
            // engine's status screen stays truthful even when no threshold
            // was crossed.
            let slot = char_id as usize;
            if slot < self.level_up_tracker.level.len() {
                let cur_level = self.level_up_tracker.level[slot];
                let next = self
                    .level_up_tracker
                    .threshold_for(slot, cur_level)
                    .unwrap_or(0);
                if let Some(rec) = self.roster.members.get_mut(slot) {
                    rec.set_cumulative_xp(self.level_up_tracker.xp[slot]);
                    rec.set_next_level_xp(next);
                    rec.set_magic_rank(cur_level);
                }
            }
            let Some(result) = result else {
                continue;
            };
            if let Some(rec) = self.roster.members.get_mut(char_id as usize) {
                LevelUpTracker::apply_to_record(&result, rec);
            }
            let new_hms = self
                .roster
                .members
                .get(char_id as usize)
                .map(|r| r.hp_mp_sp());
            if let (Some(actor), Some(hms)) = (self.actors.get_mut(member as usize), new_hms) {
                actor.battle.max_hp = hms.hp_max;
                actor.battle.hp = hms.hp_cur;
                actor.battle.mp = hms.mp_cur;
            }
            self.pending_battle_events.push(BattleEvent::LevelUp {
                char_id,
                new_level: result.new_level,
                hp_gained: result.hp_gained,
                mp_gained: result.mp_gained,
            });
            self.current_level_up_banner = Some(LevelUpBanner {
                char_id,
                new_level: result.new_level,
                hp_gained: result.hp_gained,
                mp_gained: result.mp_gained,
                frames_remaining: LevelUpBanner::DEFAULT_FRAMES,
            });
            results.push(result);
        }
        results
    }

    /// Resolve the victory spoils for `formation` (the reward half of
    /// `FUN_8004E568`): accumulate each dead enemy's gold as `gold >> 1`,
    /// finalize it through the +25% bonus + halve
    /// ([`vm::battle_formulas::victory_gold_finalize`]) and add it to
    /// [`World::money`]; sum the enemy EXP and distribute it (scaled 3/4,
    /// ceiling-split) via [`World::apply_battle_xp`]. Returns the aggregated
    /// [`BattleRewards`] (`gold` is the **credited** amount, not the raw sum) so
    /// engines can surface the post-battle banner ("got N XP, M gold,
    /// learned spell X").
    ///
    /// Monsters whose ids aren't in `catalog` contribute zero - the call
    /// silently skips them rather than failing, so a partially-populated
    /// catalog still drives a battle-end transition.
    pub fn apply_battle_loot(
        &mut self,
        formation: &crate::monster_catalog::FormationDef,
        catalog: &crate::monster_catalog::MonsterCatalog,
    ) -> BattleRewards {
        let mut xp_total: u32 = 0;
        // Accumulated `gold >> 1` over dead enemies (the victory-gold accumulator
        // in `FUN_8004E568`); finalized below via the `>> 1` halve + optional
        // +25% bonus. NOT the raw record-gold sum.
        let mut gold_acc: u32 = 0;
        let mut drops: Vec<u8> = Vec::new();
        for slot in &formation.slots {
            let Some(def) = catalog.get(slot.monster_id) else {
                continue;
            };
            xp_total = xp_total.saturating_add(def.exp as u32);
            gold_acc =
                gold_acc.saturating_add(vm::battle_formulas::victory_gold_per_monster(def.gold));
            if let Some(item_id) = def.drop_item
                && def.drop_rate_q8 > 0
            {
                // 1-in-256 fixed-point drop roll: pull one byte from the
                // deterministic RNG and compare. `drop_rate_q8 == 255`
                // makes the drop near-guaranteed (1/256 floor); `0`
                // already short-circuited above.
                let roll = (self.next_rng() & 0xFF) as u8;
                if roll < def.drop_rate_q8 {
                    drops.push(item_id);
                    let entry = self.inventory.entry(item_id).or_insert(0);
                    *entry = entry.saturating_add(1);
                }
            }
        }
        // The +25% gold bonus fires when a living party member carries bit
        // `0x10000` of the SECOND ability word (`FUN_8004E568` tests the u32
        // at record `+0xF8`): overall bit 48 of the `+0xF4` bitfield = byte 6,
        // mask 0x01 = accessory passive index 0x30 ("Gold Boost", the Golden
        // Book - see `docs/formats/accessory-passive-table.md`). "Living" =
        // post-battle battle HP > 0, the same set `apply_battle_xp` divides
        // EXP among.
        let party_count = self.party_count as usize;
        let more_gold = (0..party_count).any(|i| {
            self.actors.get(i).is_some_and(|a| a.battle.hp > 0)
                && self
                    .roster
                    .members
                    .get(self.party_roster_slot(i))
                    .is_some_and(|rec| rec.ability_bits()[6] & 0x01 != 0)
        });
        let gold_credited = vm::battle_formulas::victory_gold_finalize(gold_acc, more_gold);

        let level_ups = if xp_total > 0 {
            self.apply_battle_xp(xp_total)
        } else {
            Vec::new()
        };
        let new_money = (self.money as i64).saturating_add(gold_credited as i64);
        self.money = new_money.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        BattleRewards {
            xp: xp_total,
            gold: gold_credited,
            level_ups,
            drops,
        }
    }

    /// Resolve a **steal** attempt against `monster_id` using the per-monster
    /// steal table (the Evil God Icon mechanic). Rolls the monster's steal
    /// chance against the deterministic world RNG; on success the stolen item is
    /// added to [`Self::inventory`] and its id returned. Returns `None` when the
    /// monster has no steal (item `0` / chance `0`) or the roll misses.
    ///
    /// The steal item + chance live in a static `SCUS_942.54` table
    /// (`DAT_80077828`), **not** the monster record, so the caller passes the
    /// parsed [`legaia_asset::steal_table::StealTable`] (the engine reads the
    /// disc-resident data the randomizer edits). This is the steal counterpart
    /// to the drop grant in [`Self::apply_battle_loot`]: a percent roll
    /// (`rand % 100 < chance`) then the same `inventory` add. See
    /// `docs/formats/steal-table.md`.
    pub fn apply_steal(
        &mut self,
        monster_id: u16,
        steal_table: &legaia_asset::steal_table::StealTable,
    ) -> Option<u8> {
        let entry = steal_table.entry(monster_id).filter(|e| e.is_stealable())?;
        let roll = (self.next_rng() % 100) as u8;
        if roll < entry.chance_pct {
            let slot = self.inventory.entry(entry.item_id).or_insert(0);
            *slot = slot.saturating_add(1);
            Some(entry.item_id)
        } else {
            None
        }
    }

    /// Commit a shop **buy** transaction for the session's pending item: if the
    /// player can afford it, deduct the gold and add the item(s) to
    /// [`Self::inventory`], returning `(item_id, qty, gold_delta)` (the delta is
    /// negative). Returns `None` when the buy isn't valid (unaffordable, sell
    /// mode, no pending item - see [`crate::shop::ShopSession::try_buy`]).
    ///
    /// This is the engine's shop-purchase grant kernel, shared by the menu
    /// runtime's `ShopConfirm` commit and exercised directly by the shop /
    /// casino randomizer runtime oracles - the buy counterpart to
    /// [`Self::apply_steal`] / [`Self::apply_battle_loot`]. The item id sold is
    /// whatever the shop's stock holds, which for a town merchant is decoded
    /// straight from the scene's field-VM script (op `0x49`) the randomizer
    /// edits, so a patched shop id flows through here into the bag.
    pub fn buy_from_shop(&mut self, session: &crate::shop::ShopSession) -> Option<(u8, u8, i32)> {
        let (item_id, qty, delta) = session.try_buy(self.money)?;
        // Retail dims buy attempts past 98 held of one item id
        // ([`crate::shop::SHOP_HELD_CAP`]); refuse instead of silently
        // clamping so the menu's confirm mirrors the retail gate.
        let owned = *self.inventory.get(&item_id).unwrap_or(&0);
        if owned.saturating_add(qty) > crate::shop::SHOP_HELD_CAP {
            return None;
        }
        self.money = (self.money + delta).clamp(0, 9_999_999);
        let count = self.inventory.entry(item_id).or_insert(0);
        *count = count.saturating_add(qty);
        Some((item_id, qty, delta))
    }

    /// Build a [`crate::shop::ShopSession`] for the `idx`-th gold shop located in
    /// the active scene ([`Self::scene_shops`], decoded from the scene MAN +
    /// priced from the SCUS item table at scene entry). `None` when `idx` is out
    /// of range (no merchant, or the disc / item data was absent at boot, leaving
    /// the list empty).
    ///
    /// This is the bridge from the disc-sourced per-scene stock to the menu
    /// runtime: a host installs the returned session via
    /// [`crate::menu_runtime::MenuRuntime::open_shop`] when the player triggers
    /// the scene's merchant (field-VM op `0x49`).
    pub fn scene_shop_session(&self, idx: usize) -> Option<crate::shop::ShopSession> {
        let shop = self.scene_shops.get(idx)?;
        Some(crate::shop::ShopSession::new(shop.inventory.clone()))
    }

    /// Recognise + open a gold shop from a field-VM op-`0x49` sub-0 instruction
    /// (`instr` = the opcode byte onward: `[0x49][0x00][len][...][count][ids][name]`).
    ///
    /// The same strict, sellable-mask-gated record validation the shop catalog
    /// uses ([`legaia_asset::shop_stock::parse_record`]) rejects every non-shop
    /// op-0x49 sub-0 (inn / save prompts carry MES text, not a priced item
    /// list), so this only fires on a real merchant. Gated on
    /// [`Self::item_shop_data`] being installed - without prices there's no
    /// sellable mask (so a disc-free build can't false-positive) and no shop to
    /// price anyway. On a match it stages a priced [`crate::shop::ShopSession`]
    /// on [`Self::pending_field_shop`] and arms the op-0x49 gate; a no-op if a
    /// shop is already armed (single-open) or the record doesn't validate.
    ///
    /// Returns `true` when a shop was armed.
    pub fn try_arm_field_shop(&mut self, instr: &[u8]) -> bool {
        if self.field_shop_armed {
            return false;
        }
        let Some(data) = self.item_shop_data.as_ref() else {
            return false;
        };
        let mask = data.sellable_mask();
        let Some(rec) = legaia_asset::shop_stock::parse_record(instr, 0, Some(&mask)) else {
            return false;
        };
        let stock_ids: Vec<u8> = rec
            .id_offsets
            .iter()
            .filter_map(|&o| instr.get(o).copied())
            .collect();
        let items = stock_ids
            .iter()
            .map(|&id| crate::shop::ShopItem {
                item_id: id,
                price: data.price(id) as u32,
            })
            .collect();
        // Derive a stable per-vendor id from the shop's identity (name + stock)
        // so this vendor's seru-trade offers reseed independently of every other.
        let vendor_id = legaia_asset::seru_trade::vendor_id_from_shop(&rec.name, &stock_ids);
        let inv = crate::shop::ShopInventory::new(0, items);
        let mut session = crate::shop::ShopSession::new(inv);
        session.vendor_id = vendor_id;
        self.pending_field_shop = Some(session);
        self.field_shop_armed = true;
        self.field_shop_open = true;
        true
    }

    /// Drain the shop the field VM just opened (see [`Self::try_arm_field_shop`])
    /// so the host can drive its buy/sell UI. Returns `None` if no shop is
    /// pending. The op-0x49 gate stays armed until [`Self::finish_field_shop`].
    pub fn take_pending_field_shop(&mut self) -> Option<crate::shop::ShopSession> {
        self.pending_field_shop.take()
    }

    /// Mark the open field shop closed: the op-0x49 tristate flips Armed ->
    /// Done so the field VM resumes past the merchant op on its next step. The
    /// arm itself is cleared by the VM's resume (`op49_clear`).
    pub fn finish_field_shop(&mut self) {
        self.field_shop_open = false;
    }

    /// Record one use of `art_id` by `char_id` (roster index).
    ///
    /// Delegates to [`TacticalArtsTracker::notify_art_used`]. When the use
    /// count first crosses the learn threshold, this method:
    ///
    /// 1. Pushes [`BattleEvent::TacticalArtLearned`] onto
    ///    [`Self::pending_battle_events`].
    /// 2. Sets [`Self::current_art_banner`] with a 2-second display window
    ///    so the engine's HUD overlay can show "Learned Art #N!".
    ///
    /// Subsequent calls for the same `(char_id, art_id)` pair are no-ops.
    pub fn notify_art_used(&mut self, char_id: u8, art_id: u8) {
        if let Some(ev) = self.tactical_arts.notify_art_used(char_id, art_id) {
            let text = format!("Learned {}!", ev.name);
            self.current_art_banner = Some(ArtLearnedBanner {
                text,
                frames_remaining: ArtLearnedBanner::DEFAULT_FRAMES,
            });
            self.pending_battle_events
                .push(BattleEvent::TacticalArtLearned {
                    char_id: ev.char_id,
                    art_id: ev.art_id,
                });
        }
    }
}
