//! Player-driven battle command menu and the Arts / Magic / Item submenu
//! drivers. Split out of `battle.rs` as additional `impl World` blocks; no
//! logic change from the original inline definitions.

use super::*;

impl World {
    /// Open the player-driven command menu for party member `actor` and park
    /// the action SM. The action context's `active_actor` is set now; the
    /// queued action / target is filled in by [`Self::tick_battle_command`]
    /// once the player confirms. No-op unless [`Self::battle_player_driven`].
    pub(in crate::world) fn open_battle_command(&mut self, actor: u8) {
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
    pub(in crate::world) fn tick_battle_command(&mut self) {
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
            Some(Resolution::SpiritGuard) => {
                // Player picked Spirit: charge the AP gauge (+5, idempotent
                // per turn - the retail Square-press kernel) and raise the
                // guard stance (retail pending-action byte +0x1DE = 4, the
                // damage finisher's guard-halve input). The stance holds
                // until this actor's next turn starts. Spirit is the whole
                // turn: park at EndOfAction so the loop cycles.
                let actor = session.actor;
                if let Some(gauge) = self.ap_gauges.get_mut(actor as usize) {
                    gauge.charge_spirit();
                }
                if let Some(guard) = self.battle_guarding.get_mut(actor as usize) {
                    *guard = true;
                }
                self.battle_ctx.active_actor = actor;
                self.battle_ctx.action_state =
                    vm::battle_action::ActionState::EndOfAction.as_byte();
            }
            Some(Resolution::RunAway) => {
                // Player picked Run: roll the escape and arm the action SM's
                // run band (category 5 -> RunBegin/RunWait/RunEscape, retail
                // 0x64..0x66). The SM carries the roll outcome on
                // `multi_cast_gate` (success floors downed party HP at 1 and
                // tears the battle down `Escaped`; failure consumes the turn
                // via the Done band). The roll is the retail `FUN_801E791C`
                // formula (the writer of `_DAT_8007726C`): party SPD*1.5 +
                // missing-HP/16 vs enemy SPD + missing-HP/32, two rand draws,
                // Chicken Heart/King accessory bits folded from the living
                // party members' second ability word.
                let actor = session.actor;
                let escaped = self.roll_battle_escape();
                if let Some(a) = self.actors.get_mut(actor as usize) {
                    a.battle.action_category = 5; // Run band
                }
                self.battle_ctx.active_actor = actor;
                self.battle_ctx.queued_action = 5;
                self.battle_ctx.multi_cast_gate = u8::from(escaped);
                self.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
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
    pub(in crate::world) fn tick_battle_arts_menu(&mut self) {
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
                let (power, enemy_effect, action) = menu
                    .arts
                    .get(art_index as usize)
                    .map(|a| (a.power.clone(), a.enemy_effect, a.action))
                    .unwrap_or_default();
                self.apply_battle_art(
                    caster,
                    &power,
                    enemy_effect,
                    action,
                    target_row,
                    target_slot,
                );
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
        action: Option<legaia_art::ActionConstant>,
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
        // Arts-voice shout: fired once, on the art's animation-start frame,
        // when the art carries a real action constant (a synthetic/demo art
        // has none and stays silent - the retail degradation for arts with no
        // cue-table entry). The host resolves the (character, action) pair
        // against the arts-voice tables + XA clip banks and plays the CD-XA
        // shout with the modeled CD-response delay, so the audio trails this
        // frame rather than leading it. REF: FUN_8004C140.
        if let Some(action) = action {
            let cslot = legaia_art::Character::all()
                .iter()
                .position(|c| *c == character)
                .unwrap_or(usize::MAX);
            if cslot < 3 {
                self.battle_shout_cues
                    .push(crate::battle_events::BattleShoutCue {
                        cslot: cslot as u8,
                        action: action.as_byte(),
                    });
            }
        }
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
            // only read `power` + `enemy_effect`. `art` carries the row's
            // matched action constant when one exists (the shout-cue key);
            // the placeholder only remains for synthetic rows, and the live
            // loop doesn't drive the per-art animation script either way.
            let info = ArtStrikeInfo {
                strike_index: i as u8,
                anim_byte: 0,
                actor_slot: caster,
                target_slot: target as u8,
                character,
                art: action.unwrap_or(legaia_art::ActionConstant::Art1B),
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
                    // A petrified target (Stone) absorbs the hit - no HP loss
                    // (Stone is invulnerable at every damage entry point). The
                    // strike still counts as landed (it connected, then was
                    // nullified), matching the basic-attack / spell paths.
                    let applied = if self.actor_is_petrified(target as u8) {
                        0
                    } else {
                        dmg
                    };
                    let a = &mut self.actors[target].battle;
                    a.hp = a.hp.saturating_sub(applied);
                    total = total.saturating_add(applied as u32);
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
            let applied = self
                .status_effects
                .apply_from_enemy_effect(target as u8, enemy_effect);
            // Rot's applier rolls the disabled limb (`rand % 3`, the retail
            // `1 << (rand%3 + 3)` bit pick).
            if applied == Some(legaia_engine_vm::status_effects::StatusKind::Rot) {
                let limb = (self.next_rng() % 3) as u8;
                self.status_effects.set_rot_limb(target as u8, limb);
            }
        }
        if total > 0 {
            self.battle_hit_fx.push(BattleHitFx {
                target_slot: target as u8,
                amount: total.min(u16::MAX as u32) as u16,
                is_heal: false,
                is_crit: landed > 1,
            });
            let survives = self.actors[target].battle.hp > 0;
            self.queue_battle_reaction(target, survives);
        }
    }

    /// Build the battle Magic submenu for `caster` (an actor-table / party-row
    /// index). Reads the caster's learned spells off their roster record and
    /// their live battle MP to grey out unaffordable rows. Returns `None` when
    /// there's no roster record for the slot, OR when the caster is **silenced
    /// / petrified** (a `blocks_magic` status) - in both cases the caller
    /// reopens the command menu so the player picks a non-magic action, which
    /// is the party-side mirror of the monster AI's cast→physical fallback.
    pub(in crate::world) fn build_battle_spell_session(
        &self,
        caster: u8,
    ) -> Option<crate::battle_magic::BattleSpellSession> {
        if self.actor_blocked_from_magic(caster) {
            return None;
        }
        // `caster` is the battle ordinal; the spell list belongs to the
        // CHARACTER occupying it (roster slot per the present-party
        // composition). Live mirrors (MP, ability bits) stay ordinal-keyed.
        let char_slot = self.party_roster_slot(caster as usize) as u8;
        let member = self.roster.members.get(char_slot as usize)?;
        let list = member.spell_list();
        let n = (list.count as usize).min(list.ids.len());
        // Union the roster's saved spell list with anything learned via Seru
        // capture this session, so a freshly-learned spell is immediately
        // castable without waiting for a save/load round-trip.
        let mut learned: Vec<u8> = list.ids[..n].to_vec();
        for &sid in self.seru_log.learned_spells(char_slot) {
            if !learned.contains(&sid) {
                learned.push(sid);
            }
        }
        let caster_mp = self
            .actors
            .get(caster as usize)
            .map(|a| a.battle.mp)
            .unwrap_or(0);
        // Pass the caster's MP-saver ability bits so the menu greys rows by the
        // effective (reduced) cost the cast charges, not the raw spell cost.
        let ability_bits = self
            .character_ability_bits
            .get(caster as usize)
            .copied()
            .unwrap_or(0);
        Some(crate::battle_magic::BattleSpellSession::new(
            caster,
            caster,
            &learned,
            &self.spell_catalog,
            caster_mp,
            ability_bits,
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
    pub(in crate::world) fn tick_battle_spell_menu(&mut self) {
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

    /// Build the battle-context inventory submenu from live world state:
    /// every item the player holds (`count > 0`), one party-member target row
    /// per configured party slot, then one enemy row per live monster slot
    /// (tagged `is_enemy`). Healing / cure / revive items validate against the
    /// party rows; offensive items (Bomb / capture / escape) validate against
    /// the enemy rows - the session routes the cursor to the correct side.
    pub(in crate::world) fn build_battle_item_session(
        &self,
    ) -> crate::inventory_use::InventoryUseSession {
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
                // Row label = the occupying character's name (roster_names is
                // roster-slot keyed; `i` is the battle ordinal).
                let name = names
                    .get(self.party_roster_slot(i))
                    .cloned()
                    .unwrap_or_else(|| format!("P{}", i + 1));
                let mut row = TargetRow::new(i as u8, name)
                    .with_stats(a.battle.hp, a.battle.max_hp, a.battle.mp, mp_max)
                    .with_statuses(self.status_effects.statuses(i as u8).iter().map(|s| s.kind));
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
    pub(in crate::world) fn tick_battle_item_menu(&mut self) {
        use crate::input::PadButton;
        use crate::inventory_use::{InventoryUseInput, InventoryUseState};

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
        // Discard the event log; `used_slots` is the authoritative list of
        // targets the completed use applied to (one for a single-target item,
        // every healed ally for an all-party item).
        let _ = menu.drain_events();
        let used_slots = menu.used_slots.clone();

        if !used_slots.is_empty() {
            if let Some(item_id) = item_before {
                // Apply to every affected slot, but consume only one copy.
                for &target_slot in &used_slots {
                    let outcome = self.use_item(item_id, target_slot);
                    self.push_item_use_fx(target_slot, outcome);
                }
                self.consume_item(item_id);
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
}
