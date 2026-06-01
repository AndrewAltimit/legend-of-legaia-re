//! Battle command flow, submenu ticks, monster AI, initiative, capture
//! resolution, and battle teardown. Split out of `world.rs` as additional
//! `impl World` blocks.

use super::*;

use crate::battle_events::{BattleEvent, BattleHitFx};
use legaia_engine_vm as vm;
use vm::battle_action::{BattleEndCause, StepOutcome};

impl World {
    /// Open the player-driven command menu for party member `actor` and park
    /// the action SM. The action context's `active_actor` is set now; the
    /// queued action / target is filled in by [`Self::tick_battle_command`]
    /// once the player confirms. No-op unless [`Self::battle_player_driven`].
    pub(super) fn open_battle_command(&mut self, actor: u8) {
        if !self.battle_player_driven {
            return;
        }
        self.battle_ctx.active_actor = actor;
        self.battle_command = Some(crate::battle_input::BattleCommandSession::new(actor, actor));
    }

    /// Drive the open command session one frame from [`World::input`]. When the
    /// session resolves, arm the action SM with the chosen command + target
    /// (v0.1: a physical Attack) and clear the session so the SM resumes.
    /// On an abort (no valid target) it falls back to the first living monster
    /// so the loop never deadlocks.
    fn tick_battle_command(&mut self) {
        use crate::battle_input::{BattleCommandInput, Resolution};
        use crate::input::PadButton;
        use crate::target_picker::{CursorRow, SlotState};

        let Some(mut session) = self.battle_command.take() else {
            return;
        };

        let party_count = self.party_count.clamp(1, 3);
        let slot_at = |idx: usize| -> SlotState {
            match self.actors.get(idx) {
                Some(a) if a.battle.max_hp > 0 => SlotState::alive(true, a.battle.liveness != 0),
                _ => SlotState::default(),
            }
        };
        let mut party = [SlotState::default(); 3];
        for (i, p) in party.iter_mut().enumerate().take(party_count as usize) {
            *p = slot_at(i);
        }
        let mut monsters = [SlotState::default(); 5];
        for (i, m) in monsters.iter_mut().enumerate() {
            *m = slot_at(party_count as usize + i);
        }

        let ev = BattleCommandInput {
            up: self.input.just_pressed(PadButton::Up),
            down: self.input.just_pressed(PadButton::Down),
            left: self.input.just_pressed(PadButton::Left),
            right: self.input.just_pressed(PadButton::Right),
            cross: self.input.just_pressed(PadButton::Cross),
            circle: self.input.just_pressed(PadButton::Circle),
        };
        session.input(ev, party, monsters);

        match session.resolved() {
            Some(Resolution::Confirmed {
                // v0.1 only enables Attack, so `command` is always Attack here;
                // Arts/Magic/Item aren't wired into the live loop yet.
                command: _,
                target_row,
                target_slot,
            }) => {
                let target = match target_row {
                    CursorRow::Enemy => party_count + target_slot,
                    CursorRow::Ally => target_slot,
                };
                let actor = session.actor;
                if let Some(a) = self.actors.get_mut(actor as usize) {
                    a.battle.active_target = target;
                    a.battle.action_category = 3; // Attack
                }
                self.battle_ctx.active_actor = actor;
                self.battle_ctx.queued_action = 3;
                self.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
                // Session done; SM resumes next tick.
            }
            Some(Resolution::OpenArtsMenu) => {
                // Player picked Arts: hand off to the saved-chain submenu (same
                // pattern as Magic / Item). `tick_battle_arts_menu` drives until
                // the player runs an art (turn cycles via EndOfAction) or backs
                // out.
                self.battle_ctx.active_actor = session.actor;
                let rows = self.build_battle_arts_rows(session.actor);
                self.battle_arts_menu = Some(crate::battle_arts::BattleArtsSession::new(
                    session.actor,
                    session.actor,
                    rows,
                ));
            }
            Some(Resolution::OpenSpellMenu) => {
                // Player picked Magic: hand off to the spell submenu (same
                // pattern as Item). `tick_battle_spell_menu` drives until the
                // player casts (turn cycles via EndOfAction) or backs out.
                self.battle_ctx.active_actor = session.actor;
                match self.build_battle_spell_session(session.actor) {
                    Some(menu) => self.battle_spell_menu = Some(menu),
                    // No caster record / no catalog - don't strand the SM;
                    // reopen the command menu so the player can pick again.
                    None => self.open_battle_command(session.actor),
                }
            }
            Some(Resolution::OpenItemMenu) => {
                // Player picked Item: hand off to the inventory submenu. The
                // command session is dropped (already taken) and the action SM
                // stays parked; `tick_battle_item_menu` drives until the player
                // uses an item (turn cycles via EndOfAction) or backs out
                // (the command menu reopens for the same actor).
                self.battle_ctx.active_actor = session.actor;
                self.battle_item_menu = Some(self.build_battle_item_session());
            }
            Some(Resolution::Aborted) => {
                // No valid target the player could pick - arm a default strike
                // on the first living monster so the loop progresses.
                let actor = session.actor;
                let target = (party_count..self.actors.len() as u8)
                    .find(|&i| self.actors[i as usize].battle.liveness != 0)
                    .unwrap_or(party_count);
                if let Some(a) = self.actors.get_mut(actor as usize) {
                    a.battle.active_target = target;
                    a.battle.action_category = 3;
                }
                self.battle_ctx.active_actor = actor;
                self.battle_ctx.queued_action = 3;
                self.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
            }
            None => {
                // Still selecting - keep the session open for the next frame.
                self.battle_command = Some(session);
            }
        }
    }

    /// Drive the open battle Arts submenu one frame from [`World::input`].
    ///
    /// Edge-triggered pad → one [`crate::battle_arts::BattleArtsInput`] per
    /// frame. On a confirmed execution the art runs via [`Self::apply_battle_art`]
    /// (driving each strike's power byte through the real `apply_art_strike`
    /// path) and the action SM parks at `EndOfAction` so the live loop cycles to
    /// the next combatant. Backing out reopens the command menu.
    pub(super) fn tick_battle_arts_menu(&mut self) {
        use crate::battle_arts::{ArtsResolution, BattleArtsInput};
        use crate::input::PadButton;
        use crate::target_picker::SlotState;

        let Some(mut menu) = self.battle_arts_menu.take() else {
            return;
        };

        let party_count = self.party_count.clamp(1, 3);
        let slot_at = |idx: usize| -> SlotState {
            match self.actors.get(idx) {
                Some(a) if a.battle.max_hp > 0 => SlotState::alive(true, a.battle.liveness != 0),
                _ => SlotState::default(),
            }
        };
        let mut party = [SlotState::default(); 3];
        for (i, p) in party.iter_mut().enumerate().take(party_count as usize) {
            *p = slot_at(i);
        }
        let mut monsters = [SlotState::default(); 5];
        for (i, m) in monsters.iter_mut().enumerate() {
            *m = slot_at(party_count as usize + i);
        }

        let ev = BattleArtsInput {
            up: self.input.just_pressed(PadButton::Up),
            down: self.input.just_pressed(PadButton::Down),
            left: self.input.just_pressed(PadButton::Left),
            right: self.input.just_pressed(PadButton::Right),
            cross: self.input.just_pressed(PadButton::Cross),
            circle: self.input.just_pressed(PadButton::Circle),
        };
        menu.input(ev, party, monsters);

        match menu.resolved() {
            Some(ArtsResolution::Confirmed {
                art_index,
                target_row,
                target_slot,
            }) => {
                let caster = menu.actor;
                let (power, enemy_effect) = menu
                    .arts
                    .get(art_index as usize)
                    .map(|a| (a.power.clone(), a.enemy_effect))
                    .unwrap_or_default();
                self.apply_battle_art(caster, &power, enemy_effect, target_row, target_slot);
                self.battle_ctx.action_state =
                    vm::battle_action::ActionState::EndOfAction.as_byte();
            }
            Some(ArtsResolution::Aborted) => {
                let actor = self.battle_ctx.active_actor;
                self.open_battle_command(actor);
            }
            None => {
                self.battle_arts_menu = Some(menu);
            }
        }
    }

    /// Execute an art against the picked target through the real art-power
    /// path.
    ///
    /// Each [`legaia_art::PowerByte`] in `power` drives one strike through
    /// [`crate::art_strike::apply_art_strike`]: the byte's multiplier tier +
    /// UDF/LDF target are decoded, [`Self::resolve_battle_defense`] picks the
    /// matching defense half (when a UDF/LDF split is configured), and the
    /// per-strike damage is deducted. The art's `enemy_effect` is applied once
    /// after a landing hit (if the target survives). Summed damage surfaces as
    /// one HUD popup; the target is downed if its HP reaches zero.
    ///
    /// `power` comes from the matched art record when one is staged, else a
    /// synthetic per-direction profile (see [`Self::build_battle_arts_rows`]),
    /// so the same kernel handles both real and demo arts.
    fn apply_battle_art(
        &mut self,
        caster: u8,
        power: &[legaia_art::PowerByte],
        enemy_effect: legaia_art::EnemyEffect,
        target_row: crate::target_picker::CursorRow,
        target_slot: u8,
    ) {
        use crate::target_picker::CursorRow;
        use legaia_engine_vm::battle_action::ArtStrikeInfo;
        let party_count = self.party_count.clamp(1, 3);
        let target = match target_row {
            CursorRow::Enemy => party_count + target_slot,
            CursorRow::Ally => target_slot,
        } as usize;
        if target >= self.actors.len() {
            return;
        }
        let attack = self
            .battle_attack
            .get(caster as usize)
            .copied()
            .unwrap_or(0);
        let character = self.caster_character(caster);
        // Selector-9 accuracy/evasion terms (retail actor `+0x168`): the
        // attacker's accuracy vs the target's evasion. The roll engages only
        // when the ATTACKER has a seeded accuracy stat; an unseeded attacker
        // (`acc == 0`, the synthetic case) auto-hits AND consumes no RNG, so it
        // can't be made to whiff against a positive-evasion target and battles
        // without seeded stats keep their bit-identical streams.
        let attacker_acc = self
            .battle_accuracy
            .get(caster as usize)
            .copied()
            .unwrap_or(0);
        let target_eva = self.battle_evasion.get(target).copied().unwrap_or(0);
        let mut total: u32 = 0;
        let mut landed: u8 = 0;
        for (i, pb) in power.iter().enumerate() {
            if self.actors[target].battle.liveness == 0 {
                break;
            }
            // Minimal per-strike info: `apply_art_strike` + `resolve_battle_defense`
            // only read `power` + `enemy_effect`. `art` is a placeholder; the
            // live loop doesn't drive the per-art animation script.
            let info = ArtStrikeInfo {
                strike_index: i as u8,
                anim_byte: 0,
                actor_slot: caster,
                target_slot: target as u8,
                character,
                art: legaia_art::ActionConstant::Art1B,
                power: Some(*pb),
                dmg_timing: None,
                enemy_effect,
                hit_cue: None,
            };
            let defense = self.resolve_battle_defense(target as u8, &info);
            let outcome = crate::art_strike::apply_art_strike(attack, defense, &info);
            if let Some(dmg) = outcome.damage {
                // Roll the strike against the target's evasion. Only consume
                // RNG when the roll is meaningful (some stat seeded), so the
                // unseeded auto-hit path leaves the RNG stream untouched.
                let hit = if attacker_acc == 0 {
                    true
                } else {
                    let mut seed = self.next_rng();
                    legaia_engine_vm::battle_formulas::accuracy_roll(
                        attacker_acc,
                        target_eva,
                        &mut seed,
                    )
                };
                if hit {
                    let a = &mut self.actors[target].battle;
                    a.hp = a.hp.saturating_sub(dmg);
                    total = total.saturating_add(dmg as u32);
                    landed = landed.saturating_add(1);
                    if a.hp == 0 {
                        a.liveness = 0;
                    }
                }
            }
        }
        if landed > 0
            && enemy_effect != legaia_art::EnemyEffect::None
            && self.actors[target].battle.liveness != 0
        {
            self.status_effects
                .apply_from_enemy_effect(target as u8, enemy_effect);
        }
        if total > 0 {
            self.battle_hit_fx.push(BattleHitFx {
                target_slot: target as u8,
                amount: total.min(u16::MAX as u32) as u16,
                is_heal: false,
                is_crit: landed > 1,
            });
        }
    }

    /// Build the battle Magic submenu for `caster` (an actor-table / party-row
    /// index). Reads the caster's learned spells off their roster record and
    /// their live battle MP to grey out unaffordable rows. Returns `None` when
    /// there's no roster record for the slot (the caller reopens the command
    /// menu so the SM isn't stranded).
    pub(super) fn build_battle_spell_session(
        &self,
        caster: u8,
    ) -> Option<crate::battle_magic::BattleSpellSession> {
        let member = self.roster.members.get(caster as usize)?;
        let list = member.spell_list();
        let n = (list.count as usize).min(list.ids.len());
        // Union the roster's saved spell list with anything learned via Seru
        // capture this session, so a freshly-learned spell is immediately
        // castable without waiting for a save/load round-trip.
        let mut learned: Vec<u8> = list.ids[..n].to_vec();
        for &sid in self.seru_log.learned_spells(caster) {
            if !learned.contains(&sid) {
                learned.push(sid);
            }
        }
        let caster_mp = self
            .actors
            .get(caster as usize)
            .map(|a| a.battle.mp)
            .unwrap_or(0);
        Some(crate::battle_magic::BattleSpellSession::new(
            caster,
            caster,
            &learned,
            &self.spell_catalog,
            caster_mp,
        ))
    }

    /// Drive the open battle Magic submenu one frame from [`World::input`].
    ///
    /// Edge-triggered pad → one [`crate::battle_magic::BattleSpellInput`] per
    /// frame. On a confirmed cast the spell applies via [`Self::apply_battle_spell`]
    /// (MP deducted, HP / heal / cure / revive folded, popups surfaced) and the
    /// action SM parks at `EndOfAction` so the live loop cycles to the next
    /// combatant - a cast is the caster's whole turn, no strike fires. Backing
    /// out reopens the command menu for the same actor.
    pub(super) fn tick_battle_spell_menu(&mut self) {
        use crate::battle_magic::{BattleSpellInput, SpellResolution};
        use crate::input::PadButton;
        use crate::target_picker::SlotState;

        let Some(mut menu) = self.battle_spell_menu.take() else {
            return;
        };

        let party_count = self.party_count.clamp(1, 3);
        let slot_at = |idx: usize| -> SlotState {
            match self.actors.get(idx) {
                Some(a) if a.battle.max_hp > 0 => SlotState::alive(true, a.battle.liveness != 0),
                _ => SlotState::default(),
            }
        };
        let mut party = [SlotState::default(); 3];
        for (i, p) in party.iter_mut().enumerate().take(party_count as usize) {
            *p = slot_at(i);
        }
        let mut monsters = [SlotState::default(); 5];
        for (i, m) in monsters.iter_mut().enumerate() {
            *m = slot_at(party_count as usize + i);
        }

        let ev = BattleSpellInput {
            up: self.input.just_pressed(PadButton::Up),
            down: self.input.just_pressed(PadButton::Down),
            left: self.input.just_pressed(PadButton::Left),
            right: self.input.just_pressed(PadButton::Right),
            cross: self.input.just_pressed(PadButton::Cross),
            circle: self.input.just_pressed(PadButton::Circle),
        };
        menu.input(ev, &self.spell_catalog, party, monsters);

        match menu.resolved() {
            Some(SpellResolution::Confirmed {
                spell_id,
                target_row,
                target_slot,
            }) => {
                let caster = menu.actor;
                self.apply_battle_spell(caster, spell_id, target_row, target_slot);
                if self.battle_escaped {
                    // Escape spell succeeded: leave the encounter now (no loot,
                    // no game-over) instead of cycling the turn.
                    self.finish_battle();
                } else {
                    self.battle_ctx.action_state =
                        vm::battle_action::ActionState::EndOfAction.as_byte();
                }
            }
            Some(SpellResolution::Aborted) => {
                let actor = self.battle_ctx.active_actor;
                self.open_battle_command(actor);
            }
            None => {
                self.battle_spell_menu = Some(menu);
            }
        }
    }

    /// Cast `spell_id` from `caster` against the picked target and fold the
    /// outcome into world state. MP is deducted once up-front; the spell's
    /// [`crate::spells::SpellTarget`] shape decides which slots are affected
    /// (single → the picked slot; `AllEnemies` / `AllAllies` → the whole band),
    /// each resolved through [`crate::spells::cast_spell`]. Caster magic comes
    /// from [`Self::battle_magic`]; target magic-defense reuses
    /// [`Self::battle_defense`]. Damage / heal / cure / revive / buff / capture
    /// / escape all fold through [`Self::fold_spell_outcome`].
    fn apply_battle_spell(
        &mut self,
        caster: u8,
        spell_id: u8,
        target_row: crate::target_picker::CursorRow,
        target_slot: u8,
    ) {
        use crate::spells::SpellTarget;
        use crate::target_picker::CursorRow;

        let Some(def) = self.spell_catalog.get(spell_id).cloned() else {
            return;
        };
        let party_count = self.party_count.clamp(1, 3);
        let targets: Vec<u8> = match def.target {
            SpellTarget::OneEnemy | SpellTarget::OneAlly | SpellTarget::SelfOnly => {
                let abs = match target_row {
                    CursorRow::Enemy => party_count + target_slot,
                    CursorRow::Ally => target_slot,
                };
                vec![abs]
            }
            SpellTarget::AllEnemies => (party_count..self.actors.len() as u8).collect(),
            SpellTarget::AllAllies => (0..party_count).collect(),
        };
        self.cast_spell_on_slots(caster, &def, &targets);
    }

    /// Deduct `def`'s MP cost from `caster` and fold its effect onto each
    /// absolute actor slot in `targets`. Shared by the player cast path
    /// ([`Self::apply_battle_spell`], which resolves the cursor rows to slots
    /// from the player's perspective) and the monster-AI cast path
    /// ([`Self::apply_monster_spell`], which resolves party slots). MP is spent
    /// once up front; each target folds through [`Self::fold_spell_outcome`].
    /// Returns `false` (no MP spent, nothing folded) when the caster can't
    /// afford the cost.
    fn cast_spell_on_slots(
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
            let outcome = cast_spell(def, t, &snap);
            self.fold_spell_outcome(outcome);
        }
        // Cast band (live-loop path): a player Seru-magic id resolves to a
        // per-summon overlay. Request the spawn at the first real target's
        // battle position so the host can seat the summon scene-graph. The
        // engine equivalent of the retail cast band's `FUN_8003EC70` overlay
        // load (see `crate::summon`).
        if crate::summon::SERU_SUMMON_IDS.contains(&def.id) {
            let origin = targets
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
            self.request_summon_spawn(def.id, origin);
        }
        true
    }

    /// Fold a single-target [`crate::spells::SpellOutcome`] into live actor
    /// state and surface a HUD popup. Damage subtracts HP (and downs the
    /// target at zero); heals / revives add HP (capped); cures clear the
    /// target's status; buffs adjust a per-slot scalar with a turn timer
    /// ([`Self::apply_battle_buff`]); capture rolls vs the monster's weakened
    /// state ([`Self::resolve_capture`]); escape flags a return to the field
    /// ([`Self::battle_escaped`]). `Failed` is a no-op (MP already spent).
    pub(super) fn fold_spell_outcome(&mut self, outcome: crate::spells::SpellOutcome) {
        use crate::spells::SpellOutcome as O;
        match outcome {
            O::Damage { target, amount, .. } => {
                if let Some(a) = self.actors.get_mut(target as usize) {
                    a.battle.hp = a.battle.hp.saturating_sub(amount);
                    if a.battle.hp == 0 {
                        a.battle.liveness = 0;
                    }
                }
                self.battle_hit_fx.push(BattleHitFx {
                    target_slot: target,
                    amount,
                    is_heal: false,
                    is_crit: false,
                });
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

    /// Apply (or refresh) a stat buff / debuff on `slot`. The delta is written
    /// straight into the matching per-slot battle scalar so it changes damage
    /// the same frame: `Attack`/`MagicAttack`/`Defense` map to
    /// [`Self::battle_attack`] / [`Self::battle_magic`] / [`Self::battle_defense`]
    /// (`MagicDefense` reuses `battle_defense`, the spell-defense proxy).
    ///
    /// **Stat-up buffs (`magnitude > 0`) use the retail multiplicative ramp.**
    /// Retail's stat-up selectors (1..7) raise the live stat by ×6/5 (clamped to
    /// `0xFFFF`) — [`vm::battle_formulas::buff_ramp`], pinned from the SM dump —
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
    fn apply_battle_buff(
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
    fn add_to_buff_scalar(&mut self, slot: u8, stat: crate::spells::BuffStat, delta: i16) -> i16 {
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
    pub(super) fn tick_battle_buffs_on_turn(&mut self, slot: u8) {
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

    /// Resolve a capture-spell roll against the monster in `target`. The
    /// effective chance scales with the monster's missing-HP fraction (full
    /// `hit_pct` only near death, zero at full HP) - mirroring retail capture,
    /// which is reliable only on a weakened Seru. On success the monster is
    /// downed (so it counts toward the wipe) and its id is logged into
    /// [`Self::battle_captures`] for post-battle Seru learning.
    pub(super) fn resolve_capture(&mut self, target: u8, hit_pct: u8) {
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
        }
    }

    /// Drain the monster ids captured this battle (see [`Self::battle_captures`]).
    pub fn drain_battle_captures(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.battle_captures)
    }

    /// Install the master [`crate::seru_learning::SeruRegistry`]. Boot wires
    /// this once; [`Self::finish_battle`] consults it to bank capture points.
    pub fn set_seru_registry(&mut self, registry: crate::seru_learning::SeruRegistry) {
        self.seru_registry = registry;
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
    fn resolve_captures(&mut self) {
        let captures = std::mem::take(&mut self.battle_captures);
        self.last_capture_outcomes.clear();
        self.current_capture_banner = None;
        if captures.is_empty() || self.seru_registry.is_empty() {
            return;
        }
        let party_slots: Vec<u8> = (0..self.party_count.clamp(1, 3)).collect();
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
    }

    /// Drain the capture outcomes from the most recently finished battle.
    pub fn drain_last_capture_outcomes(&mut self) -> Vec<crate::seru_learning::CaptureOutcome> {
        std::mem::take(&mut self.last_capture_outcomes)
    }

    /// Build the battle-context inventory submenu from live world state:
    /// every item the player holds (`count > 0`), one party-member target row
    /// per configured party slot, then one enemy row per live monster slot
    /// (tagged `is_enemy`). Healing / cure / revive items validate against the
    /// party rows; offensive items (Bomb / capture / escape) validate against
    /// the enemy rows - the session routes the cursor to the correct side.
    pub(super) fn build_battle_item_session(&self) -> crate::inventory_use::InventoryUseSession {
        use crate::inventory_use::{InventoryContext, InventoryUseSession, TargetRow};
        let names = crate::field_menu_dispatch::roster_names(self);
        let items: Vec<u8> = self
            .inventory
            .iter()
            .filter_map(|(id, qty)| (*qty > 0).then_some(*id))
            .collect();
        let pc = self.party_count.clamp(1, 3) as usize;
        let mut targets: Vec<TargetRow> = (0..pc)
            .filter_map(|i| {
                let a = self.actors.get(i)?;
                // Skip unconfigured party slots (no battle stats).
                if a.battle.max_hp == 0 {
                    return None;
                }
                let mp_max = self.character_max_mp.get(i).copied().unwrap_or(0);
                let name = names
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| format!("P{}", i + 1));
                let mut row = TargetRow::new(i as u8, name).with_stats(
                    a.battle.hp,
                    a.battle.max_hp,
                    a.battle.mp,
                    mp_max,
                );
                row.alive = a.battle.liveness != 0;
                Some(row)
            })
            .collect();
        // Enemy rows: every monster slot that's configured for battle. Tagged
        // `is_enemy` so the session only accepts offensive items here.
        for slot in pc..self.actors.len() {
            let Some(a) = self.actors.get(slot) else {
                break;
            };
            if a.battle.max_hp == 0 || a.battle_monster_id.is_none() {
                continue;
            }
            let name = a
                .battle_monster_id
                .and_then(|id| self.monster_catalog.get(id))
                .map(|d| d.name.clone())
                .unwrap_or_else(|| format!("Enemy {}", slot - pc + 1));
            let mut row = TargetRow::new(slot as u8, name)
                .with_stats(a.battle.hp, a.battle.max_hp, 0, 0)
                .with_enemy(true);
            row.alive = a.battle.liveness != 0;
            targets.push(row);
        }
        InventoryUseSession::new(
            self.item_catalog.clone(),
            items,
            targets,
            InventoryContext::Battle,
        )
    }

    /// Drive the open battle inventory submenu one frame from [`World::input`].
    ///
    /// Edge-triggered pad → one [`crate::inventory_use::InventoryUseInput`] per
    /// frame. On a completed use the chosen item is applied authoritatively via
    /// [`Self::use_item`], one copy is consumed from the inventory, a heal /
    /// cure popup is surfaced for the HUD, and the action SM is parked at
    /// `EndOfAction` so the live loop cycles to the next combatant (no strike
    /// fires - using an item is the actor's whole turn). Backing out reopens
    /// the command menu for the same actor.
    pub(super) fn tick_battle_item_menu(&mut self) {
        use crate::input::PadButton;
        use crate::inventory_use::{InventoryUseEvent, InventoryUseInput, InventoryUseState};

        let Some(mut menu) = self.battle_item_menu.take() else {
            return;
        };

        let ev = if self.input.just_pressed(PadButton::Up) {
            Some(InventoryUseInput::Up)
        } else if self.input.just_pressed(PadButton::Down) {
            Some(InventoryUseInput::Down)
        } else if self.input.just_pressed(PadButton::Cross) {
            Some(InventoryUseInput::Confirm)
        } else if self.input.just_pressed(PadButton::Circle) {
            Some(InventoryUseInput::Cancel)
        } else {
            None
        };

        // The item under the cursor before the input - `current_item` reads
        // the `item_cursor` in TargetSelect, so this is the item that a Confirm
        // on a target row resolves to (the Done state no longer exposes it).
        let item_before = menu.current_item().map(|e| e.id);
        if let Some(ev) = ev {
            menu.input(ev);
        }
        let used = menu.drain_events().into_iter().find_map(|e| match e {
            InventoryUseEvent::Used { slot, .. } => Some(slot),
            _ => None,
        });

        if let Some(target_slot) = used {
            if let Some(item_id) = item_before {
                let outcome = self.use_item(item_id, target_slot);
                self.consume_item(item_id);
                self.push_item_use_fx(target_slot, outcome);
            }
            if self.battle_escaped {
                // Escape item succeeded: leave the encounter now (no loot, no
                // game-over) instead of cycling the turn.
                self.finish_battle();
            } else {
                // Using an item is the actor's whole turn: park at EndOfAction
                // so the live loop's re-arm block cycles to the next combatant.
                self.battle_ctx.action_state =
                    vm::battle_action::ActionState::EndOfAction.as_byte();
            }
            return;
        }

        match menu.state {
            InventoryUseState::Aborted => {
                // Backed out without using an item - reopen the command menu.
                let actor = self.battle_ctx.active_actor;
                self.open_battle_command(actor);
            }
            _ => {
                // Still browsing / target-selecting - keep the menu open.
                self.battle_item_menu = Some(menu);
            }
        }
    }

    /// Remove one copy of `item_id` from the inventory, dropping the entry
    /// when the count reaches zero. No-op when the player holds none.
    pub fn consume_item(&mut self, item_id: u8) {
        if let Some(qty) = self.inventory.get_mut(&item_id) {
            *qty = qty.saturating_sub(1);
            if *qty == 0 {
                self.inventory.remove(&item_id);
            }
        }
    }

    /// Surface a cosmetic HUD popup for a resolved item use. Heals / MP
    /// restores / revives push a heal-coloured number; offensive items push a
    /// damage-coloured number; cures push the status letter. The HP / status
    /// side is already applied by [`Self::use_item`]; this is presentation-only
    /// (drained via [`Self::drain_battle_hit_fx`]).
    fn push_item_use_fx(&mut self, target_slot: u8, outcome: crate::items::ItemOutcome) {
        use crate::items::ItemOutcome;
        let (amount, is_heal) = match outcome {
            ItemOutcome::HealedHp { amount } | ItemOutcome::HealedMp { amount } => (amount, true),
            ItemOutcome::Revived { hp_after } => (hp_after, true),
            ItemOutcome::DamageDealt { amount } => (amount, false),
            // Cures / capture / escape / stat boosts / no-effect: no number.
            _ => return,
        };
        if amount == 0 {
            return;
        }
        self.battle_hit_fx.push(BattleHitFx {
            target_slot,
            amount,
            is_heal,
            is_crit: false,
        });
    }

    /// Per-frame battle-side driver for the live gameplay loop. Gated by
    /// [`Self::live_gameplay_loop`] in [`Self::tick`].
    ///
    /// Wraps [`Self::step_battle`] with the host-side glue retail performs
    /// through the render + animation systems, so the battle resolves from
    /// `tick` alone:
    ///
    /// - **Damage application.** Drains this step's [`BattleEvent`]s and
    ///   folds [`BattleEvent::ApplyArtStrike`] damage into target HP. A
    ///   generic physical attack (no art) is applied on the
    ///   `AttackChain -> AttackRecovery` edge via [`Self::apply_basic_attack`].
    /// - **Liveness.** Any combatant whose HP hit zero is marked dead so the
    ///   SM's wipe scan sees it.
    /// - **Turn cycling.** When the SM idles at `EndOfAction` with monsters
    ///   still alive, the next party member is re-armed (v0.1 keeps monsters
    ///   passive - party turns only).
    /// - **Recovery edge.** Clears `ADVANCE_DONE` at `AttackRecovery`, the
    ///   edge the retail recovery animation drives.
    ///
    /// On [`StepOutcome::BattleComplete`] it runs [`Self::finish_battle`] to
    /// apply loot and return to the field.
    pub(super) fn live_battle_tick(&mut self) -> Option<StepOutcome> {
        use vm::battle_action::{ActionState, ActorFlags};

        // Player-driven: while the Arts submenu is open the action SM is
        // parked - drive it from the pad and return until the player runs an
        // art (turn cycles) or backs out (reopens the command menu).
        if self.battle_arts_menu.is_some() {
            self.tick_battle_arts_menu();
            return None;
        }

        // Player-driven: while the spell submenu is open the action SM is
        // parked - drive it from the pad and return until the player casts
        // (turn cycles) or backs out (reopens the command menu).
        if self.battle_spell_menu.is_some() {
            self.tick_battle_spell_menu();
            return None;
        }

        // Player-driven: while the inventory submenu is open the action SM is
        // parked - drive it from the pad and return until the player uses an
        // item (turn cycles) or backs out (reopens the command menu).
        if self.battle_item_menu.is_some() {
            self.tick_battle_item_menu();
            return None;
        }

        // Player-driven: while a command session is open the action SM is
        // parked - drive the command picker from the pad and return without
        // advancing the SM until the player confirms.
        if self.battle_command.is_some() {
            self.tick_battle_command();
            return None;
        }

        let outcome = self.step_battle();

        // Apply this step's damage events (art strikes carry a damage value;
        // the loop owns folding while live, so events are consumed here).
        let events = std::mem::take(&mut self.pending_battle_events);
        let mut art_strike_applied = false;
        for e in &events {
            if let BattleEvent::ApplyArtStrike {
                target_slot,
                outcome,
                ..
            } = e
            {
                art_strike_applied = true;
                // Surface the resolved strike damage for HUD popups (the
                // fold below applies the HP side; this is cosmetic only).
                if let Some(dmg) = outcome.damage
                    && dmg > 0
                {
                    self.battle_hit_fx.push(BattleHitFx {
                        target_slot: *target_slot,
                        amount: dmg,
                        is_heal: false,
                        is_crit: false,
                    });
                }
            }
            self.fold_battle_event(e);
        }

        // Generic physical attack: deal damage on the strike-landed edge when
        // no art strike already did.
        if let StepOutcome::Transition { from, to } = outcome
            && from == ActionState::AttackChain.as_byte()
            && to == ActionState::AttackRecovery.as_byte()
            && !art_strike_applied
        {
            self.apply_basic_attack();
        }

        // Mark the dead so the SM's liveness scan resolves the wipe.
        for a in self.actors.iter_mut() {
            if a.battle.max_hp > 0 && a.battle.hp == 0 {
                a.battle.liveness = 0;
            }
        }

        // Recovery-edge ADVANCE_DONE clear (retail clears this when the
        // recovery animation finishes; we simulate the same edge inline).
        let attacker = self.battle_ctx.active_actor as usize;
        if attacker < self.actors.len()
            && self.actors[attacker]
                .battle
                .flag_bits
                .has(ActorFlags::ADVANCE_DONE)
            && self.battle_ctx.action_state == ActionState::AttackRecovery.as_byte()
        {
            self.actors[attacker]
                .battle
                .flag_bits
                .clear(ActorFlags::ADVANCE_DONE);
        }

        // Re-arm the next combatant when the SM idles at EndOfAction, cycling
        // across the whole actor table (party AND monsters) in slot order so
        // monsters take their turns. Only re-arm while BOTH sides still have a
        // living member - if either side is wiped we leave the SM at
        // EndOfAction so its liveness scan resolves the wipe into
        // BattleComplete next step.
        let party_count = self.party_count.max(1);
        let n = self.actors.len() as u8;
        let party_alive = (0..party_count).any(|i| self.actors[i as usize].battle.liveness != 0);
        let monsters_alive = (party_count..n).any(|i| self.actors[i as usize].battle.liveness != 0);
        if self.battle_ctx.action_state == ActionState::EndOfAction.as_byte()
            && party_alive
            && monsters_alive
            && let Some(next) = self.next_combatant_by_initiative()
        {
            // Start-of-turn: age this actor's buffs / debuffs, reverting any
            // that expire this turn.
            self.tick_battle_buffs_on_turn(next);
            let next_is_party = next < party_count;
            if next_is_party && self.battle_player_driven {
                // Party turn under player control: pause the SM and let the
                // player pick the command. `tick_battle_command` arms the SM
                // on confirm.
                self.open_battle_command(next);
            } else if !next_is_party {
                // Monster turn: the AI picks a spell or a physical strike.
                self.take_monster_turn(next);
            } else {
                // Party turn when not player-driven: arm a generic physical
                // attack against the first living opponent.
                let target = self.first_living_opponent_of(next).unwrap_or(next);
                self.battle_ctx.active_actor = next;
                self.battle_ctx.queued_action = 3;
                self.battle_ctx.action_state = ActionState::Begin.as_byte();
                if let Some(a) = self.actors.get_mut(next as usize) {
                    a.battle.active_target = target;
                    a.battle.action_category = 3;
                }
            }
        }

        if matches!(outcome, StepOutcome::BattleComplete) {
            self.finish_battle();
        }
        Some(outcome)
    }

    /// Apply one generic physical strike from the active attacker to the
    /// first living combatant on the opposing side. v0.1 stand-in for the
    /// art-driven strike path: `damage = art_strike_damage_default(attack,
    /// defense, 16)` (≈ `attack - defense`, floored at 1) so a party with no
    /// configured weapon attack still chips the monster down - and, for a
    /// monster attacker, a monster with no configured attack still chips the
    /// party. The strike rolls against the target's evasion
    /// ([`legaia_engine_vm::battle_formulas::accuracy_roll`]); when neither the
    /// attacker's accuracy nor the target's evasion is seeded the roll auto-hits
    /// and consumes no RNG, so the unseeded synthetic loop still resolves
    /// exactly as before.
    ///
    /// The opposing side is chosen by the attacker's slot: party slots
    /// (`< party_count`) strike monsters; monster slots strike the party.
    pub(super) fn apply_basic_attack(&mut self) {
        let attacker = self.battle_ctx.active_actor as usize;
        let Some(target) = self.resolve_attack_target(attacker as u8) else {
            return;
        };
        let target = target as usize;
        let attack = self.battle_attack.get(attacker).copied().unwrap_or(0);
        let defense = self.battle_defense.get(target).copied().unwrap_or(0);
        let acc = self.battle_accuracy.get(attacker).copied().unwrap_or(0);
        let eva = self.battle_evasion.get(target).copied().unwrap_or(0);
        // Roll only when the attacker's accuracy is seeded; an unseeded
        // attacker (`acc == 0`) auto-hits and consumes no RNG.
        let hit = if acc == 0 {
            true
        } else {
            let mut seed = self.next_rng();
            vm::battle_formulas::accuracy_roll(acc, eva, &mut seed)
        };
        if !hit {
            return;
        }
        let dmg = vm::battle_formulas::art_strike_damage_default(attack, defense, 16);
        let a = &mut self.actors[target];
        a.battle.hp = a.battle.hp.saturating_sub(dmg);
        if a.battle.hp == 0 {
            a.battle.liveness = 0;
        }
        // Surface the strike for HUD damage popups.
        self.battle_hit_fx.push(BattleHitFx {
            target_slot: target as u8,
            amount: dmg,
            is_heal: false,
            is_crit: false,
        });
    }

    /// Resolve the slot a strike from `attacker` should land on. Honors a
    /// pre-selected [`battle::BattleActor::active_target`] when it points at a
    /// living actor on the opposing side (so the player's target-picker choice
    /// and the monster-AI target choice both take effect), otherwise falls back
    /// to [`Self::first_living_opponent_of`].
    fn resolve_attack_target(&self, attacker: u8) -> Option<u8> {
        let pc = self.party_count.max(1);
        let n = self.actors.len() as u8;
        let (lo, hi) = if attacker < pc { (pc, n) } else { (0, pc) };
        if let Some(a) = self.actors.get(attacker as usize) {
            let t = a.battle.active_target;
            if (lo..hi).contains(&t)
                && self
                    .actors
                    .get(t as usize)
                    .is_some_and(|x| x.battle.liveness != 0)
            {
                return Some(t);
            }
        }
        self.first_living_opponent_of(attacker)
    }

    /// Drive one monster's turn. Runs the action picker
    /// ([`Self::pick_monster_action`], the port of `FUN_801E9FD4`'s generic
    /// decision core) and either folds the chosen cast and parks the SM at
    /// `EndOfAction` (a spell is the whole turn, like the player magic path) or
    /// arms a physical strike for the action SM to run.
    pub(super) fn take_monster_turn(&mut self, slot: u8) {
        use vm::battle_action::ActionState;

        self.battle_ctx.active_actor = slot;
        match self.pick_monster_action(slot) {
            MonsterAction::Cast { spell_id, targets } => {
                let def = self.spell_catalog.get(spell_id).cloned();
                if let Some(def) = def
                    && self.cast_spell_on_slots(slot, &def, &targets)
                {
                    self.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
                    return;
                }
                // Cast didn't fold (no catalog entry / unaffordable after the
                // pick) - fall through to a physical strike.
                self.arm_monster_physical(slot);
            }
            MonsterAction::Physical { target } => {
                self.battle_ctx.queued_action = 3;
                self.battle_ctx.action_state = ActionState::Begin.as_byte();
                if let Some(a) = self.actors.get_mut(slot as usize) {
                    a.battle.active_target = target;
                    a.battle.action_category = 3;
                }
            }
        }
    }

    /// Arm a generic physical strike for monster `slot` against the first
    /// living party member (fallback when a picked cast can't fold).
    fn arm_monster_physical(&mut self, slot: u8) {
        use vm::battle_action::ActionState;
        let target = self.first_living_opponent_of(slot).unwrap_or(slot);
        self.battle_ctx.queued_action = 3;
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
        if let Some(a) = self.actors.get_mut(slot as usize) {
            a.battle.active_target = target;
            a.battle.action_category = 3;
        }
    }

    /// Monster-AI action picker - clean-room port of the **generic decision
    /// core** of `FUN_801E9FD4` (`overlay_battle_action_801e9fd4.txt`), the
    /// routine retail runs (from `recompute_battle_order` / `FUN_801DABA4`) to
    /// choose each monster's action.
    ///
    /// Faithful to the core: it rolls `rand % (1 + live_magic_count)` over the
    /// monster's own global magic-attack ids (record `+0x21..=+0x23`, carried on
    /// [`crate::monster_catalog::MonsterDef::magic_attacks`]); a roll of `0`
    /// picks a **physical** strike (target `rand % party_count`), otherwise it
    /// picks magic id `magic[roll-1]` and resolves the target by the spell's
    /// shape byte (`spell_table[id*0xC + 2] & 0x60`), modelled here through the
    /// catalog's [`crate::spells::SpellTarget`]: `OneEnemy` → a random living
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
    /// switch is ported in [`crate::monster_ai`] ([`crate::monster_ai::decide`])
    /// and consulted here as an override, followed by the post-switch
    /// recent-target ring ([`crate::monster_ai::apply_recent_target_ring`]). The
    /// companion target resolver `FUN_801E7320` is ported as
    /// [`Self::resolve_monster_target`] (the `monster_setup` hook).
    ///
    /// PORT: FUN_801E9FD4
    /// REF: FUN_801DABA4, FUN_801DA51C
    fn pick_monster_action(&mut self, slot: u8) -> MonsterAction {
        let pc = self.party_count.max(1);

        // --- generic decision core ---
        // The monster's own castable global magic ids (parser already drops the
        // empty `<= 1` slots, so every entry is "live").
        let magic: Vec<u8> = self
            .actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| self.monster_catalog.get(id))
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
        let roll = self.next_rng() % denom;
        // Provisional choice (category 3 = physical strike, 2 = magic).
        let (mut category, mut spell_id) = (3u8, 0u8);
        let mut target_class;
        if roll != 0 {
            let id = magic[(roll - 1) as usize];
            if let Some(def) = self.spell_catalog.get(id).cloned()
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
            let n = self.actors.len() as u8;
            let ctx = crate::monster_ai::MonsterAiCtx {
                monster_id: (monster_id & 0xFF) as u8,
                monster_index: slot.saturating_sub(pc),
                caster_slot: slot,
                hp,
                max_hp,
                mp,
                party_count: pc,
                monster_count: n.saturating_sub(pc).max(1),
                field_flags: self
                    .actors
                    .get(slot as usize)
                    .map(|a| a.battle.field_flags)
                    .unwrap_or(0),
                allies_with_mp,
            };
            let mut ai = std::mem::take(&mut self.monster_ai_state);
            if let Some(cast) = crate::monster_ai::decide(&ctx, &mut ai, &mut || self.next_rng()) {
                category = cast.category;
                spell_id = cast.spell_id;
                target_class = cast.target_class;
            }
            // Anti-repeat ring (applies to whichever single party target stands).
            target_class = crate::monster_ai::apply_recent_target_ring(
                target_class,
                spell_id,
                pc,
                &mut ai,
                &mut || self.next_rng(),
            );
            self.monster_ai_state = ai;
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
        self.monster_ai_state.mode_flags
    }

    /// Advance the battle-mode counter by one - the faithful port of the
    /// battle-action SM's `case 0xFF` (`_DAT_8007BD24[0x28A] += 1`), the
    /// boss-phase-transition pseudo-action. A boss script issues action `0xFF`
    /// when the fight crosses a scripted phase boundary; the next monster turn's
    /// [`Self::pick_monster_action`] then reads the bumped mode through
    /// [`crate::monster_ai::decide`], activating that phase's scripted casts.
    /// The retail counter is a byte, so it wraps at `0xFF`.
    ///
    /// PORT: FUN_801E295C
    pub fn advance_battle_mode(&mut self) {
        self.monster_ai_state.mode_flags = self.monster_ai_state.mode_flags.wrapping_add(1);
    }

    /// Target **class** the generic core picks for a monster casting `def`, by
    /// the spell's [`crate::spells::SpellTarget`] shape (monster's perspective:
    /// enemies = party band, allies = monster band). Single-enemy → a random
    /// living party slot; `AllEnemies` → class `8`; `AllAllies` → class `9`;
    /// `OneAlly` → the most-weakened living ally (or self); `SelfOnly` → self.
    fn monster_cast_target_class(&mut self, slot: u8, def: &crate::spells::SpellDef) -> u8 {
        use crate::spells::SpellTarget;
        let pc = self.party_count.max(1);
        let n = self.actors.len() as u8;
        match def.target {
            SpellTarget::OneEnemy => self.random_living_party_member(pc).unwrap_or(slot),
            SpellTarget::AllEnemies => 8,
            SpellTarget::AllAllies => 9,
            SpellTarget::SelfOnly => slot,
            SpellTarget::OneAlly => {
                let mut best: Option<(u8, u16)> = None;
                for i in pc..n {
                    if let Some(a) = self.actors.get(i as usize)
                        && a.battle.liveness != 0
                        && a.battle.hp < a.battle.max_hp / 2
                        && best.is_none_or(|(_, hp)| a.battle.hp < hp)
                    {
                        best = Some((i, a.battle.hp));
                    }
                }
                best.map(|(i, _)| i).unwrap_or(slot)
            }
        }
    }

    /// Resolve an absolute target list from a `+0x1DD` target class: `8` = all
    /// living party, `9` = all living monsters, `< party_count` = that single
    /// party slot, otherwise that single monster/self slot.
    fn resolve_class_to_slots(&self, slot: u8, class: u8) -> Vec<u8> {
        let pc = self.party_count.max(1);
        let n = self.actors.len() as u8;
        let alive = |i: u8| {
            self.actors
                .get(i as usize)
                .is_some_and(|a| a.battle.liveness != 0)
        };
        let _ = slot;
        match class {
            8 => (0..pc).filter(|&i| alive(i)).collect(),
            9 => (pc..n).filter(|&i| alive(i)).collect(),
            t if t < n => vec![t],
            // Out-of-range class: no targets (the caller falls back to physical).
            _ => Vec::new(),
        }
    }

    /// Pick a random living party member (`rand % party_count`, re-rolled until
    /// it lands on a living slot), mirroring the party-target roll shared by
    /// `FUN_801E9FD4` and `FUN_801E7320`. `None` only when the whole party is
    /// down. The deterministic LCG cycles every value, so the re-roll loop
    /// always terminates once one member is alive.
    fn random_living_party_member(&mut self, party_count: u8) -> Option<u8> {
        let pc = party_count.max(1);
        let any_alive = (0..pc).any(|i| {
            self.actors
                .get(i as usize)
                .is_some_and(|a| a.battle.liveness != 0)
        });
        if !any_alive {
            return None;
        }
        loop {
            let t = (self.next_rng() % pc as u32) as u8;
            if self
                .actors
                .get(t as usize)
                .is_some_and(|a| a.battle.liveness != 0)
            {
                return Some(t);
            }
        }
    }

    /// Clean-room port of `FUN_801E7320` - the monster-AI **target resolver**,
    /// invoked by the battle SM (`FUN_801E295C`) at `ActionSeed` as the
    /// `monster_setup` hook for monster actors whose `field_flags & 0x380` is
    /// set. It reads the targeting-class byte the action picker left in
    /// `actor.active_target` (`+0x1DD`) and expands it into a concrete target,
    /// re-rolling the deterministic RNG until it lands on a living actor on the
    /// matching side:
    ///
    /// - **class `0..2`** → a living **monster** slot (`rand % monster_count +
    ///   party_count`); if it lands on self, clears `action_category` and keeps
    ///   self as the target.
    /// - **class `3..6`** → a living **party** slot (`rand % party_count`).
    /// - **class `8`** → 1-in-3 keeps the all-target code `9`, else self.
    /// - **class `7` / other** → 1-in-3 sets the all-target code `8`, else self.
    ///
    /// Retail ctx fields: `ctx[+0]` = party count, `ctx[+1]` = monster count,
    /// `ctx[+0x13]` = active slot - here read from `party_count` / the actor
    /// table / `slot`. See `ghidra/scripts/funcs/overlay_battle_action_801e7320.txt`.
    ///
    /// Note: in the current live loop monsters carry `field_flags == 0`, so the
    /// SM does not invoke this and the picker's own target stands. Wiring the
    /// `0x380` flag (set by retail at an as-yet-untraced init site) is the open
    /// RE thread; this port keeps the routine faithful for when that lands.
    ///
    /// PORT: FUN_801E7320
    /// REF: FUN_801E295C
    pub(super) fn resolve_monster_target(&mut self, slot: u8) {
        let pc = self.party_count.max(1);
        let mc = (self.actors.len() as u8).saturating_sub(pc).max(1);
        let class = match self.actors.get(slot as usize) {
            Some(a) => a.battle.active_target,
            None => return,
        };
        let set_target = |w: &mut Self, t: u8| {
            if let Some(a) = w.actors.get_mut(slot as usize) {
                a.battle.active_target = t;
            }
        };
        let clear_category_self = |w: &mut Self| {
            if let Some(a) = w.actors.get_mut(slot as usize) {
                a.battle.action_category = 0;
                a.battle.active_target = slot;
            }
        };
        if class < 3 {
            // Target a living monster (the caster's own band).
            loop {
                let t = (self.next_rng() % mc as u32) as u8 + pc;
                set_target(self, t);
                if self
                    .actors
                    .get(t as usize)
                    .is_some_and(|a| a.battle.liveness != 0)
                {
                    if t == slot {
                        clear_category_self(self);
                    }
                    return;
                }
            }
        } else if class < 7 {
            // Target a living party member.
            loop {
                let t = (self.next_rng() % pc as u32) as u8;
                set_target(self, t);
                if self
                    .actors
                    .get(t as usize)
                    .is_some_and(|a| a.battle.liveness != 0)
                {
                    return;
                }
            }
        } else if class == 8 {
            if self.next_rng().is_multiple_of(3) {
                set_target(self, 9);
            } else {
                clear_category_self(self);
            }
        } else if self.next_rng().is_multiple_of(3) {
            set_target(self, 8);
        } else {
            clear_category_self(self);
        }
    }

    /// First living actor on the side opposing `attacker`. Party slots
    /// (`< party_count`) oppose the monster band (`party_count..`); monster
    /// slots oppose the party. `None` if that side is wiped.
    pub(super) fn first_living_opponent_of(&self, attacker: u8) -> Option<u8> {
        let pc = self.party_count.max(1);
        let n = self.actors.len() as u8;
        let (lo, hi) = if attacker < pc { (pc, n) } else { (0, pc) };
        (lo..hi).find(|&i| {
            self.actors
                .get(i as usize)
                .is_some_and(|a| a.battle.liveness != 0)
        })
    }

    /// Next living combatant after `after` in round-robin slot order across
    /// the whole actor table (party then monsters, wrapping). Drives the live
    /// loop's turn cycling so monsters take turns interleaved with the party.
    /// `None` only when no actor is alive.
    pub(super) fn next_living_combatant(&self, after: u8) -> Option<u8> {
        let n = self.actors.len();
        if n == 0 {
            return None;
        }
        (1..=n).find_map(|step| {
            let idx = (after as usize + step) % n;
            (self.actors[idx].battle.liveness != 0).then_some(idx as u8)
        })
    }

    /// True when at least one living battle slot carries a non-zero SPD. Gates
    /// the SPD-seeded initiative turn order on real speed data; otherwise the
    /// battle stays on the round-robin [`Self::next_living_combatant`].
    pub(super) fn any_battle_speed(&self) -> bool {
        (0..BATTLE_SLOTS).any(|i| {
            self.battle_speed[i] != 0 && self.actors.get(i).is_some_and(|a| a.battle.liveness != 0)
        })
    }

    /// Seed every living battle slot's initiative key from its SPD; dead slots
    /// get `0`. Per-actor formula `init_key = speed + rand()%(speed/2 + 1) + 1`
    /// (`overlay_0897_801e23ec`), so every living actor's key is `>= 1`.
    fn reseed_initiative(&mut self) {
        for i in 0..BATTLE_SLOTS {
            let alive = self.actors.get(i).is_some_and(|a| a.battle.liveness != 0);
            if !alive {
                if let Some(a) = self.actors.get_mut(i) {
                    a.battle.init_key = 0;
                }
                continue;
            }
            let speed = self.battle_speed[i];
            let span = (speed / 2 + 1) as u32; // never 0
            let key = speed as u32 + (self.next_rng() % span) + 1;
            if let Some(a) = self.actors.get_mut(i) {
                a.battle.init_key = key.min(u16::MAX as u32) as u16;
            }
        }
    }

    /// Seed the battle's initiative keys at setup: every living actor gets a
    /// key, then slot 0's key is consumed so it leads round 1 and the selector
    /// orders the rest by initiative. No-op (keys left at `0`) when no SPD is
    /// present, leaving the battle on the round-robin fallback.
    pub(super) fn seed_battle_initiative(&mut self) {
        if !self.any_battle_speed() {
            return;
        }
        self.reseed_initiative();
        if let Some(a) = self.actors.get_mut(0) {
            a.battle.init_key = 0;
        }
    }

    /// Next combatant by SPD-seeded initiative - the port of
    /// `recompute_battle_order` (`FUN_801daba4`). Returns the living actor with
    /// the highest current initiative key (random tiebreak via `rand %
    /// tie_count`), consuming that actor's key so the next turn picks another.
    /// When every living actor's key is spent a new round is seeded. Dead
    /// actors' keys are zeroed (the function's first loop) so they can't be
    /// picked. Falls back to round-robin when no actor carries SPD.
    ///
    /// PORT: FUN_801DABA4
    pub(super) fn next_combatant_by_initiative(&mut self) -> Option<u8> {
        if !self.any_battle_speed() {
            return self.next_living_combatant(self.battle_ctx.active_actor);
        }
        // First loop: zero dead actors' keys so the max-pick skips them.
        for i in 0..BATTLE_SLOTS {
            if self.actors.get(i).is_some_and(|a| a.battle.liveness == 0)
                && let Some(a) = self.actors.get_mut(i)
            {
                a.battle.init_key = 0;
            }
        }
        // Round boundary: when no living actor still holds a key, reseed.
        let any_key = (0..BATTLE_SLOTS).any(|i| {
            self.actors
                .get(i)
                .is_some_and(|a| a.battle.liveness != 0 && a.battle.init_key != 0)
        });
        if !any_key {
            self.reseed_initiative();
        }
        // Highest key among living actors; ties collected in slot order.
        let mut best: u16 = 0;
        let mut ties: Vec<u8> = Vec::new();
        for i in 0..BATTLE_SLOTS {
            let Some(a) = self.actors.get(i) else {
                continue;
            };
            if a.battle.liveness == 0 {
                continue;
            }
            let key = a.battle.init_key;
            if key == 0 {
                continue;
            }
            if key > best {
                best = key;
                ties.clear();
                ties.push(i as u8);
            } else if key == best {
                ties.push(i as u8);
            }
        }
        if ties.is_empty() {
            return self.next_living_combatant(self.battle_ctx.active_actor);
        }
        let pick = ties[(self.next_rng() as usize) % ties.len()];
        if let Some(a) = self.actors.get_mut(pick as usize) {
            a.battle.init_key = 0; // consume this turn
        }
        Some(pick)
    }

    /// Resolve a finished battle and return to the field.
    ///
    /// On [`BattleEndCause::MonsterWipe`] applies loot (XP / gold / drops /
    /// level-ups) via [`Self::apply_battle_loot`] against the captured
    /// formation; on [`BattleEndCause::PartyWipe`] raises [`Self::game_over`]
    /// (v0.1 has no defeat screen). Either way the field actor snapshot is
    /// restored, the encounter session drops into its grace window, and the
    /// scene mode flips back to [`SceneMode::Field`].
    pub(super) fn finish_battle(&mut self) {
        if self.battle_end == Some(BattleEndCause::MonsterWipe)
            && let Some(formation) = self.active_formation.clone()
        {
            // `apply_battle_loot` borrows the catalog while mutating self, so
            // swap it out and back around the call.
            let catalog = std::mem::take(&mut self.monster_catalog);
            let rewards = self.apply_battle_loot(&formation, &catalog);
            self.monster_catalog = catalog;
            self.last_battle_rewards = Some(rewards);
        }
        if self.battle_end == Some(BattleEndCause::PartyWipe) {
            self.game_over = true;
        }
        self.active_formation = None;
        self.battle_end = None;
        self.battle_escaped = false;
        // Restore the field track stashed at encounter start (cross-fades
        // back from the battle music). No-op if no swap was active.
        self.restore_field_bgm();
        // Revert any lingering buff deltas so the per-slot scalars return to
        // base, then drop the trackers + captured-id log (a new battle re-inits
        // these).
        let buffs = std::mem::take(&mut self.battle_buffs);
        for b in buffs {
            self.add_to_buff_scalar(b.slot, b.stat, -b.applied_delta);
        }
        // Bank any captured Seru into learning progress (drains battle_captures).
        self.resolve_captures();
        // Drop any open command / item / spell session - they belong to the
        // finished battle.
        self.battle_command = None;
        self.battle_item_menu = None;
        self.battle_spell_menu = None;
        self.battle_arts_menu = None;
        // Stale damage popups must not bleed into the next encounter / field.
        self.battle_hit_fx.clear();
        // Post-battle grace + suppression on the session.
        self.end_encounter_battle();
        // Restore the field actor table captured at the transition.
        if let Some(ret) = self.field_return.take() {
            self.actors = ret.actors;
            self.player_actor_slot = ret.player_actor_slot;
            self.party_count = ret.party_count;
        }
        // Return to the mode the battle was entered from (the field for a
        // field encounter, the overworld for a world-map encounter), then
        // reset the latch so a subsequent direct `enter_battle` defaults back
        // to the field.
        self.mode = self.battle_return_mode;
        self.battle_return_mode = SceneMode::Field;
        // Reset step tracking so the post-battle position doesn't count as a
        // step on the next field tick.
        self.field_last_tile = None;
    }

    /// Active enemy actors in the current battle as `(actor_index,
    /// monster_id, battle_slot)`, where `battle_slot` is the 0-based monster
    /// index the battle texture loader keys VRAM placement on (feed it to
    /// `legaia_asset::monster_archive::MonsterMesh::battle_render_mesh`).
    /// Empty unless the world is in [`SceneMode::Battle`].
    ///
    /// A renderer uses this to bridge each decoded monster mesh into its draw
    /// list: the engine itself never loads the archive, so the actor only
    /// carries the id - the host resolves it to a mesh.
    pub fn battle_monster_slots(&self) -> Vec<(usize, u16, u8)> {
        if !matches!(self.mode, SceneMode::Battle) {
            return Vec::new();
        }
        let first_monster = self.party_count as usize;
        self.actors
            .iter()
            .enumerate()
            .filter_map(|(idx, a)| {
                let id = a.battle_monster_id?;
                let slot = idx.checked_sub(first_monster)? as u8;
                Some((idx, id, slot))
            })
            .collect()
    }
}
