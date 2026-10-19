//! Using an item: the `use_item` entry point and the per-effect appliers
//! (status clear, fury boost, arts book, point card, buffs and stat raises).

use super::*;
use legaia_engine_mode::mode::SceneMode;

impl World {
    /// Use an item from the catalog against a target slot. Wraps the
    /// `items::apply_effect` resolution and folds the outcome back into
    /// world state (HP/MP deltas, status cure, revive HP). Returns the
    /// resolved [`legaia_engine_menus::items::ItemOutcome`] so the engine can drive
    /// dialog / SFX / visual cues.
    ///
    /// Returns [`legaia_engine_menus::items::ItemOutcome::NoEffect`] when:
    ///   - the item id is not in the catalog,
    ///   - or the target slot is out of range.
    ///
    /// HP / MP changes are clamped to the actor's max values. Cure /
    /// CureAll outcomes also clear the corresponding entries from the
    /// `StatusEffectTracker`.
    pub fn use_item(
        &mut self,
        item_id: u8,
        target_slot: u8,
    ) -> legaia_engine_menus::items::ItemOutcome {
        let entry = match self.tables.item_catalog.get(item_id) {
            Some(e) => *e,
            None => return legaia_engine_menus::items::ItemOutcome::NoEffect,
        };
        // Permanent multi-stat boost (class-6 Water line): resolved from the
        // on-disc effect table (which this path needs anyway), not the pure
        // table-less `apply_effect`.
        if let legaia_engine_menus::items::ItemEffect::StatUp = entry.effect {
            return self.apply_stat_up_item(item_id, target_slot);
        }
        // One-battle stat buff (class-7 Elixir): ramps the target's battle-actor
        // stat scalars by ×6/5, resolved from the on-disc table.
        if let legaia_engine_menus::items::ItemEffect::BattleBuff = entry.effect {
            return self.apply_buff_item(item_id, target_slot);
        }
        // One-battle action-gauge extension (class-5 Fury Boost): extends the
        // target's AP gauge for the rest of the battle.
        if let legaia_engine_menus::items::ItemEffect::ActionGauge = entry.effect {
            return self.apply_fury_boost_item(target_slot);
        }
        // Hyper-Art book (class 11/12/13): the applier's ordered insert into a
        // fixed character's displayed-skill list. The target the player picked
        // is not read at all - the class picks the record.
        if let legaia_engine_menus::items::ItemEffect::ArtsBook = entry.effect {
            return self.apply_arts_book_item(item_id);
        }
        // Point Card strike (class 14): discharge the bank onto the target.
        if let legaia_engine_menus::items::ItemEffect::PointCardStrike = entry.effect {
            return self.apply_point_card_strike(target_slot);
        }
        let idx = target_slot as usize;
        // Which record is authoritative depends on where the use happens
        // (retail: the 0x414-byte roster record at `0x80084708 + n*0x414` is
        // the persistent truth; battle actors are projections seeded from it
        // at battle load and flowed back at teardown via
        // `persist_battle_party_hp`):
        //
        // - In battle, `target_slot` is a party ordinal and the live
        //   `BattleActor` mirror is the record retail's item handler writes;
        //   the roster receives the final values at battle end.
        // - Out of battle, `target_slot` is a ROSTER slot (the field menu's
        //   target strip enumerates `roster.members`) and retail's menu heal
        //   writes the roster record directly - healing only the mirror left
        //   the field menu (which re-reads the roster) showing no change.
        //   The mapped party actor's mirror, when one is live, is refreshed
        //   too so field-side consumers of the projection stay consistent.
        let field_roster =
            self.mode != SceneMode::Battle && self.party.roster.members.get(idx).is_some();
        // BattleActor holds `mp` but not `max_mp`; engines that wire the
        // character record into the actor populate it via a sibling field.
        // For the snapshot we use the character_max_mp accessor (defaults
        // to `mp` itself when not separately tracked, which gives a
        // conservative "MP already capped" reading).
        let status_mask = self
            .battle
            .status_effects
            .statuses(target_slot)
            .iter()
            .fold(0u8, |m, s| {
                m | legaia_engine_menus::items::status_bit(s.kind)
            });
        let snapshot = if field_roster {
            let hms = self.party.roster.members[idx].hp_mp_sp();
            legaia_engine_menus::items::TargetSnapshot {
                hp: hms.hp_cur,
                hp_max: hms.hp_max,
                mp: hms.mp_cur,
                mp_max: hms.mp_max,
                is_dead: hms.hp_cur == 0 && hms.hp_max > 0,
                status_mask,
            }
        } else {
            match self.actors.get(idx) {
                Some(a) => legaia_engine_menus::items::TargetSnapshot {
                    hp: a.battle.hp,
                    hp_max: a.battle.max_hp,
                    mp: a.battle.mp,
                    mp_max: self
                        .tables
                        .character_max_mp
                        .get(idx)
                        .copied()
                        .unwrap_or(a.battle.mp),
                    is_dead: a.battle.hp == 0 && a.battle.max_hp > 0,
                    status_mask,
                },
                None => return legaia_engine_menus::items::ItemOutcome::NoEffect,
            }
        };
        if let Some(outcome) = self.apply_status_clear_item(item_id, target_slot, snapshot.hp) {
            return outcome;
        }
        let outcome = legaia_engine_menus::items::apply_effect(entry.effect, &snapshot);
        match outcome {
            legaia_engine_menus::items::ItemOutcome::HealedHp { amount } => {
                if field_roster {
                    let rec = &mut self.party.roster.members[idx];
                    let mut hms = rec.hp_mp_sp();
                    hms.hp_cur = hms.hp_cur.saturating_add(amount).min(hms.hp_max);
                    rec.set_hp_mp_sp(hms);
                    self.mirror_roster_hp_mp(idx);
                } else if let Some(a) = self.actors.get_mut(idx) {
                    a.battle.hp = a.battle.hp.saturating_add(amount).min(a.battle.max_hp);
                }
            }
            legaia_engine_menus::items::ItemOutcome::HealedMp { amount } => {
                if field_roster {
                    let rec = &mut self.party.roster.members[idx];
                    let mut hms = rec.hp_mp_sp();
                    hms.mp_cur = hms.mp_cur.saturating_add(amount).min(hms.mp_max);
                    rec.set_hp_mp_sp(hms);
                    self.mirror_roster_hp_mp(idx);
                } else if let Some(a) = self.actors.get_mut(idx) {
                    let cap = self
                        .tables
                        .character_max_mp
                        .get(idx)
                        .copied()
                        .unwrap_or(u16::MAX);
                    a.battle.mp = a.battle.mp.saturating_add(amount).min(cap);
                }
            }
            legaia_engine_menus::items::ItemOutcome::Cured { kind } => {
                self.battle.status_effects.cure(target_slot, kind);
            }
            legaia_engine_menus::items::ItemOutcome::CuredAll => {
                self.battle.status_effects.cure_all(target_slot);
            }
            legaia_engine_menus::items::ItemOutcome::Revived { hp_after } => {
                if field_roster {
                    let rec = &mut self.party.roster.members[idx];
                    let mut hms = rec.hp_mp_sp();
                    hms.hp_cur = hp_after.min(hms.hp_max);
                    rec.set_hp_mp_sp(hms);
                    self.mirror_roster_hp_mp(idx);
                } else if let Some(a) = self.actors.get_mut(idx) {
                    // Retail's `+0x14C` is live HP and the liveness flag at
                    // once, so a revive that writes HP stands the member back
                    // up. The port keeps the two apart: raising HP alone left
                    // a member every HP reader counts alive (the monster AI
                    // picks it) and every liveness reader counts down, still
                    // kneeling in its downed chain. The readout seed is
                    // `apply_battle_item`'s.
                    let before = a.battle.hp;
                    a.battle.hp = hp_after.min(a.battle.max_hp);
                    if a.battle.hp > 0 {
                        a.battle.liveness = 1;
                    }
                    if before == 0 {
                        self.stand_revived_party_member(idx);
                    }
                }
            }
            legaia_engine_menus::items::ItemOutcome::SpiritGained { amount }
                if idx < self.battle.ap_gauges.len() =>
            {
                // Refund AP into the active actor's gauge if it's a party slot.
                self.battle.ap_gauges[idx].refund(amount);
            }
            legaia_engine_menus::items::ItemOutcome::DamageDealt { amount } => {
                // Offensive item (e.g. Bomb): subtract HP from the enemy slot
                // and down it if it reaches zero.
                if let Some(a) = self.actors.get_mut(idx) {
                    a.battle.hp = a.battle.hp.saturating_sub(amount);
                    if a.battle.hp == 0 {
                        a.battle.liveness = 0;
                    }
                }
            }
            legaia_engine_menus::items::ItemOutcome::CaptureRolled { strength } => {
                // Capture item: roll against the enemy's missing-HP fraction
                // (shared with the Magic capture path); a success downs the
                // monster and logs its id into `battle_captures`.
                self.resolve_capture(target_slot, strength.min(u8::MAX as u16) as u8);
            }
            legaia_engine_menus::items::ItemOutcome::EscapeRequested => {
                // Escape item (e.g. Goblin Foot): flag the encounter to end;
                // the battle item-menu tick returns to the field.
                self.battle.escaped = true;
            }
            legaia_engine_menus::items::ItemOutcome::StatRaised { target, delta } => {
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

    /// Refresh the live `BattleActor` HP/MP mirror of the party member whose
    /// present-party mapping ([`Self::party_roster_slot`]) points at roster
    /// slot `rslot`, from that roster record. Field-side sibling of
    /// [`Self::resync_party_actors_from_roster`] scoped to one record: a
    /// field-menu item just wrote the persistent record, and any live
    /// projection of it must not keep the stale value. No-op when no party
    /// ordinal maps to the slot (e.g. a reserve member).
    /// The applier's **selector-8** arm for an item whose disc effect class is
    /// `8` (Antidote): `None` for any other class, so the caller falls through
    /// to the catalog path.
    ///
    /// Retail's arm is not "cure the one ailment the item names". It skips a
    /// target at zero HP outright (`beq v0,zero` at `0x80041BCC`), and
    /// otherwise masks the target's packed status word with `0xFFFC` - clearing
    /// **both** poison bits, Venom `0x0001` and Toxic `0x0002` - whatever the
    /// item's tier byte says. The word is the tracker's packed `+0x16E` /
    /// `+0x12E` view ([`StatusEffectTracker::display_flags`]), and every
    /// tracked kind whose bit the mask removed is cured.
    ///
    /// A target the mask leaves unchanged reports `NoEffect`, as the catalog's
    /// cure does for an unafflicted target.
    ///
    /// [`StatusEffectTracker::display_flags`]: legaia_engine_battle_vm::status_effects::StatusEffectTracker::display_flags
    fn apply_status_clear_item(
        &mut self,
        item_id: u8,
        target_slot: u8,
        hp: u16,
    ) -> Option<legaia_engine_menus::items::ItemOutcome> {
        use legaia_engine_battle_vm::status_effects::StatusKind;
        let class = self.tables.item_effects.as_ref()?.effect(item_id)?.class;
        if class != legaia_engine_battle_vm::battle_action::EFFECT_SELECTOR_STATUS_CLEAR {
            return None;
        }
        let word = self.battle.status_effects.display_flags(target_slot);
        let Some(after) = legaia_engine_battle_vm::battle_action::selector_status_clear(hp, word)
        else {
            return Some(legaia_engine_menus::items::ItemOutcome::NoEffect);
        };
        let cleared = word & !after;
        let mut first = None;
        for kind in [StatusKind::Venom, StatusKind::Toxic] {
            if kind.display_bit() & cleared != 0
                && self.battle.status_effects.cure(target_slot, kind)
            {
                first.get_or_insert(kind);
            }
        }
        Some(match first {
            Some(kind) => legaia_engine_menus::items::ItemOutcome::Cured { kind },
            None => legaia_engine_menus::items::ItemOutcome::NoEffect,
        })
    }

    /// Project roster record `rslot`'s HP / MP onto the party actor that
    /// mirrors it (if that character is in the present party). Every field
    /// write to a record's pools goes through here, so the mirrors a battle
    /// seats from - and [`Self::save_party`] writes back - never go stale.
    pub fn mirror_roster_hp_mp(&mut self, rslot: usize) {
        let Some(rec) = self.party.roster.members.get(rslot) else {
            return;
        };
        let hms = rec.hp_mp_sp();
        let n = (self.party.party_count as usize).min(self.actors.len());
        for member in 0..n {
            if self.party_roster_slot(member) == rslot {
                let a = &mut self.actors[member].battle;
                a.hp = hms.hp_cur.min(hms.hp_max);
                a.max_hp = hms.hp_max;
                a.mp = hms.mp_cur.min(hms.mp_max);
                a.liveness = if hms.hp_cur > 0 { 1 } else { 0 };
            }
        }
    }

    /// Apply a one-battle action-gauge extension
    /// ([`legaia_engine_menus::items::ItemEffect::ActionGauge`], Fury Boost) to party
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
    /// flag, no extra gauge). Returns [`legaia_engine_menus::items::ItemOutcome::NoEffect`] for
    /// a non-party slot.
    ///
    /// The flag itself is the battle actor's `+0x1F9` byte
    /// ([`legaia_engine_battle_vm::battle_action::BattleActor::spirit_shield`]), and
    /// that is what the live gauge reads: the round boundary
    /// ([`crate::battle_round::BattleRound::boundary`], `FUN_801D88CC` loop A)
    /// restores `+0x154` - the arts entry's command pool - to
    /// `base * 7 / 5 + 8` (cap `0x120`) while it is up. The `ApGauge` budget
    /// alone reached neither, so a Fury Boost bought a player nothing.
    fn apply_fury_boost_item(
        &mut self,
        target_slot: u8,
    ) -> legaia_engine_menus::items::ItemOutcome {
        let idx = target_slot as usize;
        if idx >= self.battle.ap_gauges.len() {
            return legaia_engine_menus::items::ItemOutcome::NoEffect;
        }
        if let Some(a) = self.actors.get_mut(idx) {
            a.battle.spirit_shield = 1;
        }
        // Already boosted this battle: retail re-sets the same flag, no compound.
        if self.battle.fury_boost[idx].is_none() {
            let gauge = &mut self.battle.ap_gauges[idx];
            let before = gauge.base_ap;
            let after = ((before as u16 * 7) / 5) as u8;
            let delta = after.saturating_sub(before);
            gauge.set_base_ap(after);
            // Extend the live gauge so the longer budget is usable this turn.
            gauge.current_ap = gauge.current_ap.saturating_add(delta);
            self.battle.fury_boost[idx] = Some(delta);
        }
        legaia_engine_menus::items::ItemOutcome::ActionGaugeExtended
    }

    /// Apply a Hyper-Art **book** ([`legaia_engine_menus::items::ItemEffect::ArtsBook`], the
    /// class-`11`/`12`/`13` consumables Fire / Wind / Thunder Book I..III).
    ///
    /// The engine's stand-in for applier selectors `0x0B`..`0x0D`
    /// (`FUN_800402F4`, arm `0x80041FB4`). Two shapes of that arm the port
    /// keeps exactly:
    ///
    /// * **The target the player picked is ignored.** The record written is
    ///   `class - 11`, i.e. roster slot 0 / 1 / 2, computed from the descriptor
    ///   alone (`addiu v1,v1,-0xb` at `0x80041FC0`). Using Fire Book I on Noa
    ///   still teaches Vahn.
    /// * **The art id is the descriptor's `tier` byte**, which is why the three
    ///   book lines do not share a tier space.
    ///
    /// The insert itself is
    /// [`legaia_engine_battle_vm::battle_action::selector_insert_displayed_skill`] -
    /// the ordered (ascending-by-id) insert into the record's `+0x185` count /
    /// `+0x186` id list, which `WorldFieldHost::learned_arts` already reads
    /// back for the battle AI's auto-fill queue.
    ///
    /// Retail's out-of-battle leg also raises the "learned" notification
    /// (`jal 0x80035C00` at `0x8004208C`, skipped when the mode word
    /// `0x8007B83C` is `0x15`); the engine surfaces that as the returned
    /// [`legaia_engine_menus::items::ItemOutcome::ArtLearned`] instead of a side channel, so
    /// a host can bannerise it on the field and stay quiet in battle.
    ///
    /// Returns [`legaia_engine_menus::items::ItemOutcome::NoEffect`] when no effect table is
    /// installed, when the id is not a book, when the class names a roster slot
    /// the party does not have, or when the list is already full.
    fn apply_arts_book_item(&mut self, item_id: u8) -> legaia_engine_menus::items::ItemOutcome {
        let Some(eff) = self
            .tables
            .item_effects
            .as_ref()
            .and_then(|t| t.effect(item_id))
        else {
            return legaia_engine_menus::items::ItemOutcome::NoEffect;
        };
        if !legaia_engine_menus::items::ARTS_BOOK_CLASSES.contains(&eff.class) {
            return legaia_engine_menus::items::ItemOutcome::NoEffect;
        }
        self.teach_hyper_art(eff.class, eff.tier)
    }

    /// Applier selectors `0x0B`..`0x0D` (`FUN_800402F4`, arm `0x80041FB4`)
    /// with an explicit `(class, tier)`: insert art `tier` into roster slot
    /// `class - 11`'s displayed-skill list. The arts books reach it through
    /// their descriptor; the field VM's `4C C7 <char> <art>` (arm
    /// `0x801E28D4`, `FUN_800402F4(char + 0x0B, art, 0, 0)`) reaches it
    /// directly - the masters who teach a Hyper Art in conversation.
    ///
    /// PORT: FUN_800402F4 (selectors `0x0B`..`0x0D`)
    pub(crate) fn teach_hyper_art(
        &mut self,
        class: u8,
        tier: u8,
    ) -> legaia_engine_menus::items::ItemOutcome {
        if !legaia_engine_menus::items::ARTS_BOOK_CLASSES.contains(&class) {
            return legaia_engine_menus::items::ItemOutcome::NoEffect;
        }
        let rslot = usize::from(class - legaia_engine_menus::items::ARTS_BOOK_CLASS_BASE);
        let Some(rec) = self.party.roster.members.get_mut(rslot) else {
            return legaia_engine_menus::items::ItemOutcome::NoEffect;
        };
        let mut list = rec.displayed_skills();
        let mut count = list.count;
        let Some(position) =
            legaia_engine_battle_vm::battle_action::selector_insert_displayed_skill(
                &mut list.ids,
                &mut count,
                tier,
            )
        else {
            return legaia_engine_menus::items::ItemOutcome::NoEffect;
        };
        list.count = count;
        rec.set_displayed_skills(list);
        legaia_engine_menus::items::ItemOutcome::ArtLearned {
            character: rslot as u8,
            art_id: tier,
            position: position as u8,
        }
    }

    /// Discharge the **Point Card** bank onto `target_slot` - the engine's
    /// stand-in for applier selector `0x0E` (`FUN_800402F4`, arm `0x8004209C`).
    ///
    /// The arm spends `min(bank, 0x270F)` of the counter retail keeps at
    /// `0x800845B4` (the engine's [`legaia_engine_minigame_scenes::minigame_state::MinigameState::point_card`],
    /// the same purse a shop buy credits), writes the remainder back, applies
    /// the amount as HP damage with the band's kill-capable clamp, and stages
    /// the victim's reaction clip through the cast band's three-leg pick. The
    /// arithmetic is
    /// [`legaia_engine_battle_vm::battle_action::selector_point_card`]; this is the
    /// seat that gives it a bank and a victim.
    ///
    /// **No shipped item reaches it.** Decoding all 256 static item rows
    /// through the effect descriptors finds no class-`14` row, so on the retail
    /// disc this is reachable code over unreachable data; an edited effect
    /// table opens it. The catalog seeder
    /// ([`legaia_engine_menus::items::ItemCatalog::apply_point_card_items`]) therefore sweeps
    /// the id space rather than naming ids.
    fn apply_point_card_strike(
        &mut self,
        target_slot: u8,
    ) -> legaia_engine_menus::items::ItemOutcome {
        use legaia_engine_battle_vm::battle_action::{PointCardVictim, selector_point_card};
        let counter = self.minigames.point_card.max(0) as u32;
        let idx = target_slot as usize;
        let Some(a) = self.actors.get_mut(idx) else {
            return legaia_engine_menus::items::ItemOutcome::NoEffect;
        };
        let p = |i: usize| a.battle.params.get(i - 0x1DF).copied().unwrap_or(0);
        let victim = PointCardVictim {
            hp: a.battle.hp,
            flinch_anim: p(0x1EF),
            knockdown_anim: p(0x1F1),
            reaction_gate: p(0x1F2),
            restage: 0,
        };
        let Some(d) = selector_point_card(counter, victim) else {
            return legaia_engine_menus::items::ItemOutcome::NoEffect;
        };
        a.battle.hp = d.hp;
        a.battle.queued_anim = d.staged_anim;
        if a.battle.max_hp > 0 && a.battle.hp == 0 {
            a.battle.liveness = 0;
        }
        self.minigames.point_card = d.remaining as i32;
        legaia_engine_menus::items::ItemOutcome::PointCardSpent {
            spent: d.spent,
            remaining: d.remaining,
        }
    }

    /// Apply a one-battle stat buff ([`legaia_engine_menus::items::ItemEffect::BattleBuff`],
    /// the class-7 Elixirs) to `target_slot`. The buffed stats are resolved from
    /// the installed on-disc item-effect table; each is ramped ×6/5 for the rest
    /// of the battle through the shared buff path ([`Self::apply_battle_buff`],
    /// the same machinery as buff *spells*) - so it reuses the precise
    /// revert-on-expiry / revert-at-battle-end bookkeeping. `Defense` ramps the
    /// single defence scalar; `Agility` maps to the accuracy/evasion proxy (no
    /// live scalar yet, so it only runs the turn timer, like a buff spell on
    /// Speed). Returns [`legaia_engine_menus::items::ItemOutcome::NoEffect`] when no table is
    /// installed or the id isn't a one-battle buff.
    fn apply_buff_item(
        &mut self,
        item_id: u8,
        target_slot: u8,
    ) -> legaia_engine_menus::items::ItemOutcome {
        use legaia_engine_battle::spells::BuffStat;
        use legaia_game_tables::item_effect::{StatItemEffect, StatTarget};
        // "One battle": a turn count large enough to outlast the encounter; the
        // buff is reverted wholesale at battle end (`finish_battle`).
        const ONE_BATTLE: u8 = u8::MAX;
        // Positive magnitude selects the retail ×6/5 multiplicative ramp in
        // `apply_battle_buff` (the value itself is only a sign hint there).
        const BUFF_SIGN: i16 = 1;

        let resolved = self
            .tables
            .item_effects
            .as_ref()
            .and_then(|t| t.stat_effect(item_id));
        let Some(StatItemEffect::BuffOneBattle(stats)) = resolved else {
            return legaia_engine_menus::items::ItemOutcome::NoEffect;
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
            legaia_engine_menus::items::ItemOutcome::NoEffect
        } else {
            legaia_engine_menus::items::ItemOutcome::Buffed { count }
        }
    }

    /// Apply a permanent multi-stat boost ([`legaia_engine_menus::items::ItemEffect::StatUp`],
    /// the class-6 *Water* line) to party slot `target_slot`. The per-stat
    /// changes are resolved from the installed on-disc item-effect table via the
    /// item's `(class, tier)` descriptor (`legaia_game_tables::item_effect`).
    ///
    /// Retail's apply handler raises the record's **base** window
    /// (`+0x11C..+0x12C`) and nothing else: Life Water is `lhu / addiu 0x10 /
    /// sh` on `+0x11C` with the `9999` clamp (`0x8004119C..0x800411C0`), and
    /// every other arm has the same shape on its own halfword. The live
    /// window follows because the menu's use routine calls the stat
    /// aggregator straight after the handler (`jal 0x800402F4` at
    /// `0x801D8538`, `jal 0x80042558` at `0x801D8540`), and the aggregator
    /// rebuilds each live value from its base
    /// ([`Self::rebuild_live_hp_mp`]). So a raised maximum is the base plus
    /// whatever percent boost the equipment carries, and current HP / MP are
    /// only clamped to it - the gain is not refilled.
    ///
    /// Adding the increment to the live maximum instead left it one short
    /// of the rebuilt value under a Max HP +10% accessory (`537 + 53` against
    /// `573 + 16`), and the next battle - which derives its ceiling from the
    /// base - healed the member past the record's maximum.
    ///
    /// The combat stats take the base raise and the same increment on the
    /// live value: the aggregator's equipment and percent arms for them are
    /// the battle seed's ([`Self::seed_party_battle_stats`]), which reads the
    /// base window. `Defense` raises both facets, as the handler does.
    /// Returns [`legaia_engine_menus::items::ItemOutcome::NoEffect`] when no
    /// table is installed or the id isn't a permanent stat-up.
    ///
    /// REF: FUN_800402F4 (class-6 arms, jump table `0x800151B0`),
    /// FUN_801D8308 (`0x801D8538..0x801D8540`), FUN_80042558
    fn apply_stat_up_item(
        &mut self,
        item_id: u8,
        target_slot: u8,
    ) -> legaia_engine_menus::items::ItemOutcome {
        use legaia_game_tables::item_effect::{StatItemEffect, StatTarget};
        let idx = target_slot as usize;
        let resolved = self
            .tables
            .item_effects
            .as_ref()
            .and_then(|t| t.stat_effect(item_id));
        let Some(StatItemEffect::Permanent(changes)) = resolved else {
            return legaia_engine_menus::items::ItemOutcome::NoEffect;
        };
        let Some(rec) = self.party.roster.members.get_mut(idx) else {
            return legaia_engine_menus::items::ItemOutcome::NoEffect;
        };
        let mut base = rec.record_stats();
        let mut live = rec.live_stats();
        let mut count = 0u8;
        for ch in &changes {
            // `lhu / addiu delta / sh`, then the `sltiu cap + 1` clamp.
            let raise = |v: u16| v.saturating_add(ch.delta).min(ch.cap);
            // The live value moves by what the base actually gained.
            let follow = |live: u16, old: u16, new: u16| live.saturating_add(new - old);
            match ch.stat {
                StatTarget::MaxHp => base.hp_max = raise(base.hp_max),
                StatTarget::MaxMp => base.mp_max = raise(base.mp_max),
                StatTarget::Attack => {
                    let new = raise(base.atk);
                    live.atk = follow(live.atk, base.atk, new);
                    base.atk = new;
                }
                StatTarget::Defense => {
                    let (udf, ldf) = (raise(base.udf), raise(base.ldf));
                    live.udf = follow(live.udf, base.udf, udf);
                    live.ldf = follow(live.ldf, base.ldf, ldf);
                    (base.udf, base.ldf) = (udf, ldf);
                    // Two halfwords, two raises.
                    count = count.saturating_add(1);
                }
                StatTarget::Speed => {
                    let new = raise(base.spd);
                    live.spd = follow(live.spd, base.spd, new);
                    base.spd = new;
                }
                StatTarget::Intelligence => {
                    let new = raise(base.int);
                    live.int = follow(live.int, base.int, new);
                    base.int = new;
                }
                StatTarget::Agility => {
                    let new = raise(base.agl);
                    live.agl = follow(live.agl, base.agl, new);
                    base.agl = new;
                }
            }
            count = count.saturating_add(1);
        }
        rec.set_record_stats(base);
        rec.set_live_stats(live);
        self.rebuild_live_hp_mp(idx);
        self.seed_party_battle_stats();
        if count == 0 {
            legaia_engine_menus::items::ItemOutcome::NoEffect
        } else {
            legaia_engine_menus::items::ItemOutcome::StatsRaised { count }
        }
    }

    /// Rebuild roster record `rslot`'s live HP / MP maxima from its base
    /// window ([`Self::rebuild_record_live_hp_mp`]) and carry them to the
    /// member's battle actor.
    pub fn rebuild_live_hp_mp(&mut self, rslot: usize) {
        self.rebuild_record_live_hp_mp(rslot);
        self.mirror_roster_hp_mp(rslot);
    }

    /// Rebuild roster record `rslot`'s live HP / MP maxima from its base
    /// window, the way the stat aggregator does, and publish the MP ceiling.
    /// The record only: the battle actor is not touched, so the aggregator
    /// pass can run mid-battle, where the actor and not the record holds the
    /// live HP.
    ///
    /// The aggregator copies the base maxima over the live ones
    /// (`+0x11C -> +0x104`, `+0x11E -> +0x108`, `0x800425C4..0x80042600`),
    /// adds the equipment's percent boosts off the **base** - ability bit 0
    /// is a tenth of base HP (`multu 0xCCCCCCCD` / `srl 3`), bit 1 a quarter
    /// (`srl 2`), bits 2 and 3 the same two for MP
    /// (`0x800429A0..0x80042A5C`) - clamps them to `9999` and `999`
    /// (`0x80042C0C..0x80042C38`), and lowers a current value that now
    /// exceeds its maximum (`0x80042CE4..0x80042D18`).
    ///
    /// A record with an empty base window is left alone: it was never built
    /// by the aggregator (a synthetic party that sets its live maxima
    /// directly), and a rebuild would zero it.
    ///
    /// REF: FUN_80042558
    pub(crate) fn rebuild_record_live_hp_mp(&mut self, rslot: usize) {
        let Some(rec) = self.party.roster.members.get(rslot) else {
            return;
        };
        let base = rec.record_stats();
        if base.hp_max == 0 {
            return;
        }
        let bits = self
            .tables
            .accessory_passives
            .bits_for_equipment(&rec.equipment().slots)[0];
        let boosted = |base: u16, tenth: bool, quarter: bool, cap: u16| {
            let mut v = base;
            if tenth {
                v = v.saturating_add(base / 10);
            }
            if quarter {
                v = v.saturating_add(base >> 2);
            }
            v.min(cap)
        };
        let mut hms = rec.hp_mp_sp();
        hms.hp_max = boosted(base.hp_max, bits & 0x1 != 0, bits & 0x2 != 0, 9999);
        hms.mp_max = boosted(base.mp_max, bits & 0x4 != 0, bits & 0x8 != 0, 999);
        hms.hp_cur = hms.hp_cur.min(hms.hp_max);
        hms.mp_cur = hms.mp_cur.min(hms.mp_max);
        self.party.roster.members[rslot].set_hp_mp_sp(hms);
        let n = (self.party.party_count as usize).min(self.actors.len());
        for member in 0..n {
            if self.party_roster_slot(member) == rslot {
                self.set_character_max_mp(member as u8, hms.mp_max);
            }
        }
    }

    /// Apply a permanent [`legaia_engine_menus::items::ItemOutcome::StatRaised`] to party
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
    fn apply_stat_raise(
        &mut self,
        idx: usize,
        target: legaia_engine_menus::items::StatBoostTarget,
        delta: u16,
    ) {
        use legaia_engine_menus::items::StatBoostTarget as T;
        const STAT_CAP_FALLBACK: u16 = 999;
        const HPMP_CAP: u16 = 9999;
        if self.party.roster.members.get(idx).is_none() {
            return;
        }
        match target {
            T::HpMax => {
                {
                    let rec = &mut self.party.roster.members[idx];
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
                    let rec = &mut self.party.roster.members[idx];
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
                    let rec = &mut self.party.roster.members[idx];
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
}
