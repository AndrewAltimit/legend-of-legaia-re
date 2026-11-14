//! The in-battle Magic and Item menus: session build, menu tick, the item menu
//! model, and item application and consumption.

use super::*;

impl World {
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
    ) -> Option<legaia_engine_battle::battle_magic::BattleSpellSession> {
        if self.battle.actor_blocked_from_magic(caster) {
            return None;
        }
        // `caster` is the battle ordinal; the spell list belongs to the
        // CHARACTER occupying it (roster slot per the present-party
        // composition). Live mirrors (MP, ability bits) stay ordinal-keyed.
        let char_slot = self.party_roster_slot(caster as usize) as u8;
        let member = self.party.roster.members.get(char_slot as usize)?;
        let list = member.spell_list();
        let n = (list.count as usize).min(list.ids.len());
        // Union the roster's saved spell list with anything learned via Seru
        // capture this session, so a freshly-learned spell is immediately
        // castable without waiting for a save/load round-trip.
        let mut learned: Vec<u8> = list.ids[..n].to_vec();
        for &sid in self.seru.log.learned_spells(char_slot) {
            if !learned.contains(&sid) {
                learned.push(sid);
            }
        }
        // An expansion pack's spells the character knows, under their menu
        // ids (catalog entries installed at the battle's entry).
        learned.extend(self.expansion_pack_spell_ids(char_slot));
        let caster_mp = self
            .actors
            .get(caster as usize)
            .map(|a| a.battle.mp)
            .unwrap_or(0);
        // Pass the caster's MP-saver ability bits so the menu greys rows by the
        // effective (reduced) cost the cast charges, not the raw spell cost.
        let ability_bits = self
            .party
            .character_ability_bits
            .get(caster as usize)
            .copied()
            .unwrap_or(0);
        Some(legaia_engine_battle::battle_magic::BattleSpellSession::new(
            caster,
            caster,
            &learned,
            &self.tables.spell_catalog,
            caster_mp,
            ability_bits,
        ))
    }

    /// Drive the open battle Magic submenu one frame from [`World::input`].
    ///
    /// Edge-triggered pad → one [`legaia_engine_battle::battle_magic::BattleSpellInput`] per
    /// frame. On a confirmed cast the action SM's Magic band is armed with the
    /// committed spell and its resolved targets ([`Self::arm_player_cast`]);
    /// the band charges the MP, plays the cast, and the outcome folds at
    /// retail's seam - a cast is the caster's whole turn, no strike fires.
    /// Backing out reopens the command menu for the same actor.
    pub(in crate::world) fn tick_battle_spell_menu(&mut self) {
        use legaia_engine_battle::battle_magic::{BattleSpellInput, SpellResolution};
        use legaia_engine_system::input::PadButton;

        let Some(mut menu) = self.battle.spell_menu.take() else {
            return;
        };

        // Same validator-backed target rows as the command menu.
        let (party, monsters) = self.battle_target_rows();

        // The window kernel's cursor auto-repeat, on the D-pad alone in battle.
        let repeat = self.input.take_menu_repeat();
        let ev = BattleSpellInput {
            up: self.input.just_pressed(PadButton::Up) || repeat & PadButton::Up.mask() != 0,
            down: self.input.just_pressed(PadButton::Down) || repeat & PadButton::Down.mask() != 0,
            left: self.input.just_pressed(PadButton::Left),
            right: self.input.just_pressed(PadButton::Right),
            cross: self.input.just_pressed(PadButton::Cross),
            circle: self.input.just_pressed(PadButton::Circle),
        };
        let phase_before = menu.phase.clone();
        menu.input(ev, &self.tables.spell_catalog, party, monsters);
        if let Some(cue) = legaia_engine_battle_loop::ui_cues::spell_phase_cue(
            &phase_before,
            &menu.phase,
            ev.cross,
        ) {
            self.push_battle_ui_cue(cue);
        }

        match menu.resolved() {
            Some(SpellResolution::Confirmed {
                spell_id,
                target_row,
                target_slot,
            }) => {
                // The magic window's commit (`0x46` -> its sub-cursor ->
                // the ring walk): the cast itself is the caster's dispatch.
                let caster = menu.actor;
                if let Some(a) = self.actors.get_mut(caster as usize) {
                    a.battle.action_category = 2;
                }
                self.commit_party_command(
                    caster,
                    legaia_engine_battle_loop::round_flow::PendingPartyAction::Spell {
                        spell_id,
                        target_row,
                        target_slot,
                    },
                );
            }
            Some(SpellResolution::Aborted) => {
                let actor = self.battle_ctx.active_actor;
                self.open_battle_command(actor);
            }
            None => {
                // Still browsing or targeting: the flow byte stands on the
                // window or on the target sub-cursor its commit raised
                // (`0x5C` / `0x5E` / `0x65` / `0x67`, the ladder at
                // `0x801D1C50..0x801D1D7C`).
                use legaia_engine_battle::battle_magic::SpellPhase;
                use legaia_engine_battle::target_picker::CursorRow;
                use legaia_engine_battle_menus::battle_flow::BattleFlowState as Flow;
                let flow = match &menu.phase {
                    SpellPhase::Targeting { picker, .. } => {
                        Flow::window_target(true, picker.kind().picks_enemy(), false)
                    }
                    SpellPhase::SweepConfirm { row, .. } => {
                        Flow::window_target(true, *row == CursorRow::Enemy, true)
                    }
                    _ => Flow::MagicWindow,
                };
                self.seat_sweep_target(flow);
                self.battle.spell_menu = Some(menu);
                if self.battle.set_battle_flow(flow) {
                    self.battle.spell_menu = None;
                    let actor = self.battle_ctx.active_actor;
                    self.open_battle_command(actor);
                }
            }
        }
    }

    /// The target byte `+0x1DD` on the window's sweep surfaces. Raising an
    /// all-target sub-cursor writes the side's group code on the acting
    /// member - `9` for `0x5D` / `0x5E` (`li v0,0x9` / `sb v0,0x1dd(v1)` at
    /// `0x801D1CF8..0x801D1D00`, the item arm joining at `0x801D1CF0`), `8`
    /// for `0x66` / `0x67` (`0x801D1D68..0x801D1D6C`, the item arm joining at
    /// `0x801D1D50` after its own caster-seat store) - and a cancel off any of
    /// the four re-picks the first living monster through `FUN_801DB8B4`
    /// (`0x801D27A8..0x801D27D0`, `0x801D28DC`, `0x801D2DDC`, `0x801D2F1C`).
    /// A confirm leaves the code standing for the commit.
    ///
    /// PORT: FUN_801D0748 (the sweep arms' `+0x1DD` stores)
    /// REF: FUN_801DB8B4 (the cancel re-pick)
    pub(in crate::world) fn seat_sweep_target(
        &mut self,
        next: legaia_engine_battle_menus::battle_flow::BattleFlowState,
    ) {
        use legaia_engine_battle_menus::battle_flow::BattleFlowState as Flow;
        use legaia_engine_battle_vm::battle_cue_group::{TARGET_ALL_ENEMIES, TARGET_PARTY_WIDE};
        let code = match next {
            Flow::ItemAllEnemies | Flow::SpellAllEnemies => Some(TARGET_ALL_ENEMIES),
            Flow::ItemAllAllies | Flow::SpellAllAllies => Some(TARGET_PARTY_WIDE),
            _ => None,
        };
        let was_sweep = matches!(
            self.battle.flow,
            Flow::ItemAllEnemies
                | Flow::SpellAllEnemies
                | Flow::ItemAllAllies
                | Flow::SpellAllAllies
        );
        let target = match code {
            Some(c) if self.battle.flow != next => c,
            None if was_sweep && matches!(next, Flow::ItemWindow | Flow::MagicWindow) => {
                crate::battle_round::BattleRound::first_living_monster(self)
            }
            _ => return,
        };
        let actor = self.battle_ctx.active_actor;
        if let Some(a) = self.actors.get_mut(usize::from(actor)) {
            a.battle.active_target = target;
        }
    }

    /// Build the battle-context inventory submenu from live world state:
    /// every item the player holds (`count > 0`), one party-member target row
    /// per configured party slot, then one enemy row per live monster slot
    /// (tagged `is_enemy`). Healing / cure / revive items validate against the
    /// party rows; offensive items (Bomb / capture / escape) validate against
    /// the enemy rows - the session routes the cursor to the correct side.
    pub(in crate::world) fn build_battle_item_session(
        &self,
    ) -> legaia_engine_menus::inventory_use::InventoryUseSession {
        use legaia_engine_menus::inventory_use::{
            InventoryContext, InventoryUseSession, TargetRow,
        };
        let names = self.party.roster_names();
        // The window's rows are the SCUS content builder's (`FUN_80030628`,
        // content id 3, battle context `gp+0x85C == 1`): one row per occupied
        // bag slot of the active window, each printing its own slot's count.
        // An id in two slots - Vahn's and Noa's halves before a normalize
        // merges them - is two rows; a used copy comes off the first slot
        // holding the id (`FUN_80042310` at the commit), so an emptied stack's
        // row simply leaves and the other stays where it was.
        let (mut items, mut counts, mut disabled) = self.battle_item_rows();
        // Row names off the disc's item-name table: the catalog names only
        // the usable items, and the window lists every held id. Bag ids only -
        // a pack item's alias id names the pack item through the catalog.
        let item_names: std::collections::BTreeMap<u8, String> = items
            .iter()
            .filter_map(|&id| Some((id, self.item_display_name(id)?)))
            .collect();
        // An expansion pack's battle consumables, under this fight's
        // aliases (none without a pack).
        items.extend(self.pack_battle_item_ids());
        counts.resize(items.len(), 1);
        disabled.resize(items.len(), false);
        let pc = self.party.party_count.clamp(1, 3) as usize;
        let mut targets: Vec<TargetRow> = (0..pc)
            .filter_map(|i| {
                let a = self.actors.get(i)?;
                // Skip unconfigured party slots (no battle stats).
                if a.battle.max_hp == 0 {
                    return None;
                }
                let mp_max = a.battle.mp_max;
                // Row label = the occupying character's name (roster_names is
                // roster-slot keyed; `i` is the battle ordinal).
                let name = names
                    .get(self.party_roster_slot(i))
                    .cloned()
                    .unwrap_or_else(|| format!("P{}", i + 1));
                let mut row = TargetRow::new(i as u8, name)
                    .with_stats(a.battle.hp, a.battle.max_hp, a.battle.mp, mp_max)
                    .with_statuses(
                        self.battle
                            .status_effects
                            .statuses(i as u8)
                            .iter()
                            .map(|s| s.kind),
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
                .and_then(|id| self.tables.monster_catalog.get(id))
                .map(|d| d.name.clone())
                .unwrap_or_else(|| format!("Enemy {}", slot - pc + 1));
            let mut row = TargetRow::new(slot as u8, name)
                .with_stats(a.battle.hp, a.battle.max_hp, 0, 0)
                .with_enemy(true);
            row.alive = a.battle.liveness != 0;
            targets.push(row);
        }
        InventoryUseSession::new(
            self.tables.item_catalog.clone(),
            items,
            targets,
            InventoryContext::Battle,
        )
        .with_bag_counts(counts)
        .with_item_names(item_names)
        .with_list_kernel_rows(
            disabled,
            legaia_engine_menus::inventory_use::BATTLE_ITEM_LIST_ROWS,
        )
    }

    /// The battle Item window's bag rows as `(ids, counts, disabled)`, in the
    /// SCUS content builder's order for the battle context: battle-usable
    /// rows in slot order, then the rest greyed at the tail (equipment and
    /// battle-unusable rows, then the effect-flag-`0x8` group).
    ///
    /// With no disc item-effect table the order is the same rule over the
    /// catalog's battle flag: usable rows in place, the rest at the tail.
    ///
    /// REF: FUN_80030628 (content id 3; the builder is
    /// `legaia_engine_menus::menu_list_rows::build_use_list_rows`)
    pub(in crate::world) fn battle_item_rows(&self) -> (Vec<u8>, Vec<u8>, Vec<bool>) {
        use legaia_engine_menus::menu_list_rows::{
            EffectRowTables, ROW_DISABLED, ROW_NAME_PAYLOAD_MASK, UseListCtx, build_use_list_rows,
        };
        let bag = &self.party.inventory;
        let (start, end) = bag.window_bounds();
        let slots = bag.slots();
        let window = &slots[start.min(slots.len())..end.min(slots.len())];
        let words: Vec<u16> = match self.tables.item_effects.as_ref() {
            Some(effects) => {
                let ids: Vec<u8> = window.iter().map(|&(id, _)| id).collect();
                let applicable = |_: u8| true;
                let ctx = UseListCtx {
                    battle: true,
                    door_light_blocked: false,
                    door_wind_blocked: false,
                    applicable: &applicable,
                };
                build_use_list_rows(&ids, start as u16, &EffectRowTables(effects), &ctx)
            }
            None => {
                let usable = |id: u8| {
                    self.tables
                        .item_catalog
                        .get(id)
                        .is_some_and(|e| e.usable_in_battle)
                };
                let occupied = || {
                    window
                        .iter()
                        .enumerate()
                        .filter(|(_, (id, _))| *id != 0)
                        .map(|(i, &(id, _))| ((start + i) as u16, id))
                };
                occupied()
                    .filter(|&(_, id)| usable(id))
                    .map(|(slot, _)| slot | 0x1000)
                    .chain(
                        occupied()
                            .filter(|&(_, id)| !usable(id))
                            .map(|(slot, _)| slot | 0x1000 | ROW_DISABLED),
                    )
                    .collect()
            }
        };
        let mut ids = Vec::with_capacity(words.len());
        let mut counts = Vec::with_capacity(words.len());
        let mut disabled = Vec::with_capacity(words.len());
        for w in words {
            let (id, count) = bag.slot((w & ROW_NAME_PAYLOAD_MASK) as u8);
            ids.push(id);
            counts.push(count);
            disabled.push(w & ROW_DISABLED != 0);
        }
        (ids, counts, disabled)
    }

    /// Drive the open battle inventory submenu one frame from [`World::input`].
    ///
    /// Edge-triggered pad → one [`legaia_engine_menus::inventory_use::InventoryUseInput`] per
    /// frame. On a completed use the chosen item is applied authoritatively via
    /// [`Self::use_item`], one copy is consumed from the inventory, a heal /
    /// cure popup is surfaced for the HUD, and the action SM is parked at
    /// `EndOfAction` so the live loop cycles to the next combatant (no strike
    /// fires - using an item is the actor's whole turn). Backing out reopens
    /// the command menu for the same actor.
    pub(in crate::world) fn tick_battle_item_menu(&mut self) {
        use legaia_engine_menus::inventory_use::{InventoryUseInput, InventoryUseState};
        use legaia_engine_system::input::PadButton;

        let Some(mut menu) = self.battle.item_menu.take() else {
            return;
        };

        // Retail's state 0x64 steps the target on the HORIZONTAL pad masks
        // (`0x2000`/`0x8000` at `0x801D2A50..0x801D2A5C` - the same pair the
        // ring uses), so during target select Left/Right drive the strip
        // cursor alongside Up/Down.
        let target_select = matches!(menu.state, InventoryUseState::TargetSelect { .. });
        let repeat = self.input.take_menu_repeat();
        let ev = if self.input.just_pressed(PadButton::Up)
            || repeat & PadButton::Up.mask() != 0
            || (target_select && self.input.just_pressed(PadButton::Left))
        {
            Some(InventoryUseInput::Up)
        } else if self.input.just_pressed(PadButton::Down)
            || repeat & PadButton::Down.mask() != 0
            || (target_select && self.input.just_pressed(PadButton::Right))
        {
            Some(InventoryUseInput::Down)
        } else if !target_select
            && (self.input.just_pressed(PadButton::Left) || repeat & PadButton::Left.mask() != 0)
        {
            // The list kernel pages on the horizontal pad, off the same
            // repeat-folded cursor word as Up / Down (`gp+0x86C`,
            // `0x80032AE8`).
            Some(InventoryUseInput::Left)
        } else if !target_select
            && (self.input.just_pressed(PadButton::Right) || repeat & PadButton::Right.mask() != 0)
        {
            Some(InventoryUseInput::Right)
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
        let state_before = menu.state;
        if let Some(ev) = ev {
            menu.input(ev);
        }
        // The event log drives the window's blip (`super::ui_cues`) and is
        // otherwise discarded; `used_slots` is the authoritative list of
        // targets the completed use applied to (one for a single-target item,
        // every healed ally for an all-party item).
        let events = menu.drain_events();
        if let Some(cue) =
            legaia_engine_battle_loop::ui_cues::item_events_cue(&state_before, &events)
        {
            self.push_battle_ui_cue(cue);
        }
        let used_slots = menu.used_slots.clone();

        if !used_slots.is_empty() {
            // The item window's commit. Retail consumes the copy here - the
            // `0x6E` step-back and the dead-actor sweep both *refund* it
            // through `FUN_800421D4` - and the effect lands when the member
            // dispatches: the committed category-1 action seeds through
            // `FUN_801E295C`'s item arm (`item_seed_band`) into the
            // `0x3C..0x40` cast states (`dispatch_pending_party_action`).
            let actor = self.battle_ctx.active_actor;
            // Sparring tutorial: the item commit is category 1, and it meets
            // the same `0x6E` validator an Attack or Spirit commit does. The
            // hook reads the committed byte off the active actor
            // (`lw v0,-0x6c90(0x801d)` / `lbu v1,0x1de(v0)`, `0x801F70D4..`
            // in overlay 967), so an item commit must reach it with `1` in
            // `+0x1DE` - which is what lets the Items lesson ever advance.
            // A rejection (the wrong lesson) discards the pick before the
            // copy is consumed and reopens the ring, as the attack path does.
            //
            // REF: FUN_801F6B70 (state 110, `0x801F7088..0x801F7190`)
            if item_before.is_some()
                && self.battle.tutorial.is_some()
                && self.battle.battle_tutorial_commit(
                    legaia_engine_battle_vm::battle_action::ActionCategory::Item.as_byte(),
                )
            {
                self.open_battle_command(actor);
                return;
            }
            match item_before {
                Some(item_id) => {
                    self.consume_item(item_id);
                    if let Some(a) = self.actors.get_mut(actor as usize) {
                        a.battle.action_category =
                            legaia_engine_battle_vm::battle_action::ActionCategory::Item.as_byte();
                    }
                    // The ring's Item arm wrote this already; restating it at
                    // the window's commit keeps an item window reached by any
                    // other door on the item clip rather than a stale one.
                    self.stamp_commit_clip(actor, CAST_COMMIT_CLIP);
                    self.commit_party_command(
                        actor,
                        legaia_engine_battle_loop::round_flow::PendingPartyAction::Item {
                            item_id,
                            used_slots,
                        },
                    );
                }
                // No item id resolved (defensive) - the member stands by.
                None => self.commit_party_command(
                    actor,
                    legaia_engine_battle_loop::round_flow::PendingPartyAction::StandBy,
                ),
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
                // Still browsing / target-selecting - keep the menu open. The
                // target step is the sub-cursor the window's commit raised
                // off the row's side and scope (`0x5B` one enemy, `0x5D` all
                // enemies, `0x64` one ally, `0x66` all allies;
                // `0x801D18E0..0x801D19F4`). The
                // session's cursor for an offensive item walks only the
                // enemy rows, a heal's only the party's, so the row under it
                // names the side.
                use legaia_engine_battle_menus::battle_flow::BattleFlowState as Flow;
                let flow = match menu.state {
                    InventoryUseState::TargetSelect { cursor, .. } => Flow::window_target(
                        false,
                        menu.targets.get(cursor).is_some_and(|t| t.is_enemy),
                        false,
                    ),
                    InventoryUseState::AllPartyConfirm { .. } => Flow::ItemAllAllies,
                    InventoryUseState::AllEnemiesConfirm { .. } => Flow::ItemAllEnemies,
                    _ => Flow::ItemWindow,
                };
                self.seat_sweep_target(flow);
                self.battle.item_menu = Some(menu);
                if self.battle.set_battle_flow(flow) {
                    self.battle.item_menu = None;
                    let actor = self.battle_ctx.active_actor;
                    self.open_battle_command(actor);
                }
            }
        }
    }

    /// Use `item_id` on `target_slot` **inside a battle**: the shared
    /// [`World::use_item`] resolution, plus the HP-readout seed the battle
    /// context needs and the field menu does not.
    ///
    /// `use_item` writes live HP directly. Out of battle that is complete -
    /// there is no readout. In battle it is half of retail's applier: the
    /// restore primitive `FUN_800402F4` folds the delta into the stat halfword
    /// (`0x800408A8`) **and** assigns the readout's pending accumulator
    /// `-delta` (`0x800408FC` / `0x80040D28` / `0x800410BC`). Writing only the
    /// stat leaves `hp != hp_display` with a **zero** accumulator, and that
    /// pair is absorbing: the ramp's one guard is `+0x10 != 0`
    /// (`0x800474E8`), so nothing ever moves the bar back. The action SM's
    /// `0x51` exit waits on that bar for any party-targeted action, so the
    /// next monster swing at the healed member parks the fight - with no
    /// in-battle exit, since the turn pump that would notice a KO is the thing
    /// that stopped.
    ///
    /// The seed is an **assignment**, not an accumulation, because that is
    /// what the three seed sites are: `sll/sra` the signed halfword delta,
    /// `subu v0,zero,v0`, `sw v0,0x10(actor)`. The remainder of a drain still
    /// in flight is discarded, which is retail's behaviour and not an
    /// approximation of it.
    ///
    /// REF: FUN_800402F4 (the assigning seed; kernel in
    /// `legaia_engine_battle_vm::battle_hp_bar::assign_pending`)
    pub fn apply_battle_item(
        &mut self,
        item_id: u8,
        target_slot: u8,
    ) -> legaia_engine_menus::items::ItemOutcome {
        // An expansion pack consumable runs its retail item's arm.
        let retail_id = self.pack_alias_as_item(item_id).unwrap_or(item_id);
        if let Some(outcome) = self.battle_applier_arm(retail_id, target_slot) {
            return outcome;
        }
        let before = self
            .actors
            .get(target_slot as usize)
            .map(|a| a.battle.hp)
            .unwrap_or(0);
        let outcome = self.use_item(item_id, target_slot);
        if let Some(a) = self.actors.get_mut(target_slot as usize) {
            let delta = i32::from(a.battle.hp) - i32::from(before);
            if delta != 0 {
                a.battle
                    .assign_hp_bar(delta.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16);
            }
        }
        outcome
    }

    /// Remove one copy of `item_id` from the inventory, dropping the entry
    /// when the count reaches zero. No-op when the player holds none.
    pub fn consume_item(&mut self, item_id: u8) {
        if self.consume_pack_alias(item_id) {
            return;
        }
        if let Some(qty) = self.party.inventory.get_mut(&item_id) {
            *qty = qty.saturating_sub(1);
            if *qty == 0 {
                self.party.inventory.remove(&item_id);
            }
        }
    }

    /// Surface the numeral a resolved item use throws, if any: the Point
    /// Card's spent amount and the port's offensive catalog items push a
    /// damage number; a restore, revive or cure pushes none, as retail's
    /// applier does not. The HP / status side is already applied; this is
    /// presentation-only (drained via [`Self::drain_battle_hit_fx`]).
    ///
    /// REF: FUN_800402F4 (no ring store outside the selector-`0x0E` arm)
    /// Throw the heal-coloured numeral a restore retail lands silently, when
    /// the heal-numeral enhancement ([`crate::world::WorldToggles::heal_numerals`])
    /// is on. Off, and under `pin_retail`, nothing is pushed: no applier
    /// restore, Grail tick, Final Heal or module heal reaches the ring in
    /// retail. A zero restore throws nothing either way.
    pub(in crate::world) fn push_heal_numeral(&mut self, target_slot: u8, amount: u16) {
        if self.toggles.heal_numerals && amount != 0 {
            self.battle.hit_fx.push(BattleHitFx {
                target_slot,
                amount,
                is_heal: true,
                is_crit: false,
            });
        }
    }

    pub(in crate::world) fn push_item_use_fx(
        &mut self,
        target_slot: u8,
        outcome: legaia_engine_menus::items::ItemOutcome,
    ) {
        use legaia_engine_menus::items::ItemOutcome;
        let (amount, is_heal) = match outcome {
            // Retail throws no numeral for a restore. The applier's HP / MP
            // / revive arms write the pool and the readout word `+0x10` and
            // push nothing onto the ring (`FUN_800402F4` has no `ctx[+0x83C]`
            // store; its one popup call is the Point Card's), and state
            // `0x3F` around the call pushes nothing either, so the HUD's HP
            // figure rolls up and no number rises over the patient. A
            // `Healing Berry` filmed off the Gaza item target reads the ring
            // empty and its push count `ctx[+0x273]` at 0 through the roll.
            // The heal-numeral enhancement throws one
            // ([`Self::push_heal_numeral`]).
            ItemOutcome::HealedHp { amount } | ItemOutcome::HealedMp { amount } => {
                self.push_heal_numeral(target_slot, amount);
                return;
            }
            ItemOutcome::Revived { hp_after } => {
                self.push_heal_numeral(target_slot, hp_after);
                return;
            }
            ItemOutcome::DamageDealt { amount } => (amount, false),
            // The Point Card strike pops its spent amount as a damage number,
            // which is what retail's `FUN_801F44A0(amount, slot)` call in the
            // selector-`0x0E` arm does.
            ItemOutcome::PointCardSpent { spent, .. } => {
                (spent.min(u32::from(u16::MAX)) as u16, false)
            }
            // Cures / capture / escape / stat boosts / art learns / no-effect:
            // no number.
            _ => return,
        };
        if amount == 0 {
            return;
        }
        self.battle.hit_fx.push(BattleHitFx {
            target_slot,
            amount,
            is_heal,
            is_crit: false,
        });
    }
}
