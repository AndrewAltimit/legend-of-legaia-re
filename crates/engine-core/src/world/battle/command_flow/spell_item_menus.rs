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
        if self.actor_blocked_from_magic(caster) {
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

        let ev = BattleSpellInput {
            up: self.input.just_pressed(PadButton::Up),
            down: self.input.just_pressed(PadButton::Down),
            left: self.input.just_pressed(PadButton::Left),
            right: self.input.just_pressed(PadButton::Right),
            cross: self.input.just_pressed(PadButton::Cross),
            circle: self.input.just_pressed(PadButton::Circle),
        };
        let phase_before = menu.phase.clone();
        menu.input(ev, &self.tables.spell_catalog, party, monsters);
        if let Some(cue) = super::ui_cues::spell_phase_cue(&phase_before, &menu.phase, ev.cross) {
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
                    crate::battle_round::PendingPartyAction::Spell {
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
                self.battle.spell_menu = Some(menu);
            }
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
        let items: Vec<u8> = self
            .party
            .inventory
            .iter()
            .filter_map(|(id, qty)| (*qty > 0).then_some(*id))
            .collect();
        let pc = self.party.party_count.clamp(1, 3) as usize;
        let mut targets: Vec<TargetRow> = (0..pc)
            .filter_map(|i| {
                let a = self.actors.get(i)?;
                // Skip unconfigured party slots (no battle stats).
                if a.battle.max_hp == 0 {
                    return None;
                }
                let mp_max = self.tables.character_max_mp.get(i).copied().unwrap_or(0);
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
    }

    /// Owned projection of the open battle Magic window (retail state
    /// `0x46`) while its list is browsed: the rows (name, affordability,
    /// Ra-Seru icon), the hand row, the highlighted spell's info-window lines
    /// off the disc's spell table and its level (the caster's record
    /// `+0x161` list) and effective MP cost - the `Lv` / `MP` tabs.
    ///
    /// `None` while no Magic window is browsing (target select draws the
    /// HUD's target surfaces, not this window) or a dialogue box owns the
    /// frame.
    pub fn battle_spell_menu_model(
        &self,
    ) -> Option<legaia_engine_battle::battle_magic::BattleSpellMenuModel> {
        use legaia_engine_battle::battle_magic::{BattleSpellMenuModel, SpellPhase};
        if self.mode != legaia_engine_mode::mode::SceneMode::Battle {
            return None;
        }
        if self.dialog.current.is_some() || self.dialog.inline.is_some() {
            return None;
        }
        let menu = self.battle.spell_menu.as_ref()?;
        let SpellPhase::Select { cursor } = menu.phase else {
            return None;
        };
        let ra_seru = |id: u8| legaia_engine_menus::pause_screens::RA_SERU_SPELL_IDS.contains(&id);
        let rows = menu
            .spells
            .iter()
            .map(|s| (s.name.clone(), s.affordable, ra_seru(s.id)))
            .collect();
        let icons = menu
            .spells
            .iter()
            .map(|s| legaia_engine_menus::pause_screens::spell_icon(self.menu.text.as_ref(), s.id))
            .collect();
        let selected = menu.spells.get(cursor as usize);
        let description = selected
            .and_then(|s| self.menu.text.as_ref().and_then(|t| t.spell_desc(s.id)))
            .map(|d| d.lines().map(str::to_string).collect())
            .unwrap_or_default();
        let level_mp = selected.map(|s| {
            let char_slot = self.party_roster_slot(menu.actor as usize);
            let level = self
                .party
                .roster
                .members
                .get(char_slot)
                .and_then(|m| {
                    let list = m.spell_list();
                    let n = (list.count as usize).min(list.ids.len());
                    let at = list.ids[..n].iter().position(|&id| id == s.id)?;
                    Some(list.levels[at])
                })
                .filter(|&l| l > 0)
                .unwrap_or(1);
            (level, u16::from(s.mp_cost))
        });
        Some(BattleSpellMenuModel {
            rows,
            icons,
            cursor: (!menu.spells.is_empty()).then_some(cursor as usize),
            description,
            level_mp,
        })
    }

    /// Owned projection of the open battle item menu for the windowed item
    /// surface (`legaia_engine_ui::battle_item_ui` on both hosts): the
    /// dedup row list + mapped cursor
    /// ([`InventoryUseSession::menu_view`]), the highlighted item's disc
    /// info-window line, the acting member's name (the middle breadcrumb of
    /// retail's `Begin | <name> | Item` trail) and the target roster while
    /// the session is picking a target.
    ///
    /// `None` while no battle item menu is up or a dialogue box owns the
    /// frame - the same suppression the command chips follow. Living here
    /// rather than in each host keeps the two hosts on one projection
    /// (host-drift tier: paired simulation injection sites).
    pub fn battle_item_menu_model(
        &self,
    ) -> Option<legaia_engine_menus::inventory_use::BattleItemMenuModel> {
        if self.mode != legaia_engine_mode::mode::SceneMode::Battle {
            return None;
        }
        if self.dialog.current.is_some() || self.dialog.inline.is_some() {
            return None;
        }
        let menu = self.battle.item_menu.as_ref()?;
        let view = menu.menu_view();
        let description = view
            .selected_id
            .and_then(|id| self.menu.text.as_ref().and_then(|t| t.item_desc(id)))
            .map(str::to_string);
        let actor = self.battle_ctx.active_actor;
        let actor_name = menu
            .targets
            .iter()
            .find(|t| t.slot == actor)
            .map(|t| t.name.clone())
            .unwrap_or_else(|| format!("P{}", actor + 1));
        let targets = view.target_select.then(|| {
            // Only the rows on the selected item's own side are LISTED -
            // retail's state-0x64 panel is the party roster and its cursor
            // walk wraps inside the party band `[0, ctx[+0x00])`; the
            // enemy-side item states ring the monster seats instead
            // (`FUN_801D0748`; rule pinned at
            // `legaia_engine_menus::inventory_use::target_on_effect_side`). The session
            // cursor indexes the full roster, so it is remapped onto the
            // filtered list.
            let side_ok = |t: &legaia_engine_menus::inventory_use::TargetRow| {
                menu.current_item()
                    .map(|e| {
                        legaia_engine_menus::inventory_use::target_on_effect_side(&e.effect, t)
                    })
                    .unwrap_or(!t.is_enemy)
            };
            let cursor = menu
                .targets
                .iter()
                .take(view.target_cursor)
                .filter(|t| side_ok(t))
                .count();
            (
                menu.targets
                    .iter()
                    .filter(|t| side_ok(t))
                    .map(
                        |t| legaia_engine_menus::inventory_use::BattleItemTargetRow {
                            name: t.name.clone(),
                            hp: t.hp,
                            hp_max: t.hp_max,
                            mp: t.mp,
                            mp_max: t.mp_max,
                            alive: t.alive,
                        },
                    )
                    .collect(),
                cursor,
            )
        });
        Some(legaia_engine_menus::inventory_use::BattleItemMenuModel {
            view,
            description,
            actor_name,
            targets,
        })
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
        let ev = if self.input.just_pressed(PadButton::Up)
            || (target_select && self.input.just_pressed(PadButton::Left))
        {
            Some(InventoryUseInput::Up)
        } else if self.input.just_pressed(PadButton::Down)
            || (target_select && self.input.just_pressed(PadButton::Right))
        {
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
        let state_before = menu.state;
        if let Some(ev) = ev {
            menu.input(ev);
        }
        // The event log drives the window's blip (`super::ui_cues`) and is
        // otherwise discarded; `used_slots` is the authoritative list of
        // targets the completed use applied to (one for a single-target item,
        // every healed ally for an all-party item).
        let events = menu.drain_events();
        if let Some(cue) = super::ui_cues::item_events_cue(&state_before, &events) {
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
                && self.battle_tutorial_commit(
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
                        crate::battle_round::PendingPartyAction::Item {
                            item_id,
                            used_slots,
                        },
                    );
                }
                // No item id resolved (defensive) - the member stands by.
                None => self
                    .commit_party_command(actor, crate::battle_round::PendingPartyAction::StandBy),
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
                self.battle.item_menu = Some(menu);
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
    pub(in crate::world) fn apply_battle_item(
        &mut self,
        item_id: u8,
        target_slot: u8,
    ) -> legaia_engine_menus::items::ItemOutcome {
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
        if let Some(qty) = self.party.inventory.get_mut(&item_id) {
            *qty = qty.saturating_sub(1);
            if *qty == 0 {
                self.party.inventory.remove(&item_id);
            }
        }
    }

    /// Surface a cosmetic HUD popup for a resolved item use. Heals / MP
    /// restores / revives push a heal-coloured number; offensive items push a
    /// damage-coloured number; cures push the status letter. The HP / status
    /// side is already applied by [`Self::use_item`]; this is presentation-only
    /// (drained via [`Self::drain_battle_hit_fx`]).
    pub(super) fn push_item_use_fx(
        &mut self,
        target_slot: u8,
        outcome: legaia_engine_menus::items::ItemOutcome,
    ) {
        use legaia_engine_menus::items::ItemOutcome;
        let (amount, is_heal) = match outcome {
            ItemOutcome::HealedHp { amount } | ItemOutcome::HealedMp { amount } => (amount, true),
            ItemOutcome::Revived { hp_after } => (hp_after, true),
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
