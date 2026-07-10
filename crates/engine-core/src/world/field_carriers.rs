//! Field carrier install/engage/tick, field interaction probing, carrier menus, and carrier/world-map encounter entry.
//!
//! Split out of `world.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

impl World {
    /// First synthetic [`Self::field_walk_touch`] slot for gate-0
    /// tile-trigger binds ([`Self::install_trigger_walk_touch`]). Partition-1
    /// placement indices (the natural walk-touch keys) stay well below this
    /// in the retail corpus, so the two key spaces never collide.
    pub const TRIGGER_WALK_TOUCH_SLOT_BASE: u8 = 0xC0;

    /// Place the scene's field entity SMs (all Idle). One
    /// [`vm::world_map::WorldMapEntityCtx`] per [`FieldCarrierConfig`], so a
    /// scripted-encounter carrier can be advanced via
    /// [`Self::engage_field_carrier`] and ticked by
    /// `Self::tick_field_carriers`. Replaces any previously installed set.
    ///
    /// This is the field-mode counterpart to
    /// [`Self::install_world_map_entities_with_configs`]; retail builds the
    /// same per-entity records from the scene's MAN actor-placement partition.
    pub fn install_field_carriers(&mut self, configs: Vec<FieldCarrierConfig>) {
        self.field_carriers = (0..configs.len())
            .map(|_| vm::world_map::WorldMapEntityCtx::default())
            .collect();
        self.field_carrier_configs = configs;
        self.pending_field_carrier_battle = None;
        // The slot map is only meaningful for a MAN-derived install; a
        // hand-built set has no placement slots. Clear it (and any armed engage)
        // so a re-install never leaves a stale slot pointing at the old set.
        self.field_carrier_slots.clear();
        self.pending_carrier_engage = None;
        self.carrier_menu = None;
        // NPC motion + walk-touch state is placement-keyed too: never let a
        // previous scene's routes / in-flight legs / door events leak.
        self.field_npc_routes.clear();
        self.field_npc_glide_speeds.clear();
        self.field_npc_motions.clear();
        self.field_walk_touch.clear();
        self.active_walk_touch = None;
        self.stepping_inline_npc = None;
        self.active_inline_slot = None;
    }

    /// Install the scene's field carriers **derived from its MAN actor-placement
    /// partition** ([`crate::man_field_scripts::derive_field_carriers`]) rather
    /// than from a hand-built list, replacing any previously installed set.
    ///
    /// Returns the carrier-Vec index of the Rim Elm sparring partner (the
    /// [`FieldCarrierConfig::ScriptedEncounter`] carrier) when the scene MAN
    /// contains it (town01), so the caller can [`Self::engage_field_carrier`] it
    /// on the dialogue-accept; `None` for scenes without that placement.
    ///
    /// This is the faithful counterpart to the hand-built
    /// [`Self::install_field_carriers`]: the carrier set, and the sparring
    /// carrier's identity within it, come from the real scene data.
    pub fn install_field_carriers_from_man(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
    ) -> Option<usize> {
        let derived = crate::man_field_scripts::derive_field_carriers(man_file, man);
        let sparring_idx = derived
            .iter()
            .position(|d| matches!(d.config, FieldCarrierConfig::ScriptedEncounter { .. }));

        // Map each scripted-encounter carrier's placement slot -> its carrier-Vec
        // index, so a field-interact on that placement auto-arms the fight. The
        // carrier index is the position in `derived` (install_field_carriers
        // preserves order). Plain talk NPCs are intentionally excluded: talking
        // to them must never launch a battle.
        let carrier_slots: std::collections::HashMap<u8, usize> = derived
            .iter()
            .enumerate()
            .filter(|(_, d)| matches!(d.config, FieldCarrierConfig::ScriptedEncounter { .. }))
            .filter_map(|(idx, d)| u8::try_from(d.placement_index).ok().map(|slot| (slot, idx)))
            .collect();

        self.install_field_carriers(derived.into_iter().map(|d| d.config).collect());
        // install_field_carriers cleared the slot map; repopulate for this set.
        self.field_carrier_slots = carrier_slots;

        // Capture each actor's inline interaction-script dialogue, keyed by its
        // partition-1 record index (= the `slot` a field-interact op carries),
        // so `field_interact` can open the interacted actor's real dialogue.
        // This is the actor's own inline MES text (retail `actor[+0x90]`), the
        // mechanism `0x3F` was wrongly standing in for.
        self.field_npc_dialog.clear();
        self.field_npc_dialog_prologue.clear();
        self.field_npc_positions.clear();
        self.field_npc_headings.clear();
        for (placement, kind) in crate::man_field_scripts::classify_placements(man_file, man) {
            let Ok(slot) = u8::try_from(placement.index) else {
                continue;
            };
            if let crate::man_field_scripts::PlacementKind::Npc {
                dialog_inline: Some(inline),
                ..
            } = kind
            {
                self.field_npc_dialog.insert(slot, inline);
                // Stash the untruncated record so the opt-in field-VM runner can
                // execute the interaction prologue (segment selection) - purely
                // additive; the default path keeps using `field_npc_dialog`.
                if let Some(prologue) =
                    crate::man_field_scripts::placement_inline_prologue(man_file, man, &placement)
                {
                    self.field_npc_dialog_prologue.insert(slot, prologue);
                }
                // The interaction probe box-tests the player against this spawn
                // position (= runtime actor frame; see `field_npc_positions`).
                self.field_npc_positions
                    .insert(slot, (placement.world_x, placement.world_z));
                // The placement's autonomous walk route (its own pre-text
                // `0x4C 0x51` move-to-tile ops), driven through the motion VM
                // when `animate_field_npcs` is set.
                let route =
                    crate::man_field_scripts::placement_motion_route(man_file, man, &placement);
                if !route.is_empty() {
                    self.field_npc_routes.insert(slot, route);
                    // Faithful per-leg glide speed from the placement's real
                    // `0x4C 0x51` motion-op base-step operand (retail
                    // `FUN_8003774C` `4 << bits`), replacing the flat stand-in;
                    // absent = the leg falls back to `FIELD_NPC_MOTION_SPEED`.
                    if let Some(speed) =
                        crate::man_field_scripts::placement_glide_speed(man_file, man, &placement)
                    {
                        self.field_npc_glide_speeds.insert(slot, speed);
                    }
                }
            }
            // Walk-touch events ride any non-parked placement (door warps are
            // Portal-classified, throw-back teleports are usually on guard
            // NPCs) - the locomotion's touch dispatch posts them on contact.
            if let Some(event) =
                crate::man_field_scripts::placement_walk_touch_event(man_file, man, &placement)
            {
                self.field_walk_touch
                    .insert(slot, ((placement.world_x, placement.world_z), event));
            }
        }

        // Seed the per-actor field-VM channels from the same placement partition
        // (retail spawns one script context per placement at scene load, cutscene
        // or not - the free-roam half of `FUN_8003AEB0`). Each channel's own init
        // opcodes then run through `step_field_channels`: scripted facings,
        // idle/`WAIT`-loop cadence, local-flag setup. A cutscene scene re-seeds
        // this set through `install_cutscene_timeline_record` afterwards.
        self.seed_field_channels(man_file, man);

        sparring_idx
    }

    /// Install walk-touch entries for the scene's **gate-0 tile-trigger
    /// object binds** (house doors): each bind sits at its trigger tile's
    /// world centre and fires the partition-0 record's decoded effect
    /// (see [`crate::man_field_scripts::p0_record_walk_touch_event`]) on
    /// player contact, through the same [`Self::check_field_walk_touch`]
    /// dispatch as placement contacts.
    ///
    /// Keyed from [`Self::TRIGGER_WALK_TOUCH_SLOT_BASE`] so the synthetic
    /// slots never collide with partition-1 placement indices. Call after
    /// [`Self::install_field_carriers_from_man`] (whose inner install clears
    /// `field_walk_touch`); idempotent per scene - re-installing replaces the
    /// previous bind set.
    // REF: FUN_8003A55C, FUN_801d5b5c
    pub fn install_trigger_walk_touch(
        &mut self,
        binds: &[((i16, i16), crate::man_field_scripts::WalkTouchEvent)],
    ) {
        self.field_walk_touch
            .retain(|slot, _| *slot < Self::TRIGGER_WALK_TOUCH_SLOT_BASE);
        for (i, (pos, event)) in binds.iter().enumerate() {
            let Some(slot) = Self::TRIGGER_WALK_TOUCH_SLOT_BASE.checked_add(i as u8) else {
                break;
            };
            self.field_walk_touch.insert(slot, (*pos, *event));
        }
    }

    /// Trigger a field interaction on placement `slot` (retail's field-interact
    /// op `0x3E` with `op0 < 100`, and the interaction-probe dispatch). Opens
    /// the actor's inline dialogue if it has any, arms / engages a scripted-
    /// encounter carrier on that slot (the dialogue-accept auto-arm), and
    /// surfaces a [`FieldEvent::FieldInteract`]. Shared by the field VM host and
    /// `Self::tick_field_interaction_probe`.
    pub fn trigger_field_interact(&mut self, interact_id: u8, slot: u8) {
        self.last_field_interact = Some((interact_id, slot));
        // Stash this slot's untruncated record (if any) so the opt-in VM-dialogue
        // runner can execute its interaction prologue. Always reassigned (to
        // `None` when absent) so a prior interaction's prologue can't leak.
        self.active_inline_prologue = self.field_npc_dialog_prologue.get(&slot).cloned();
        // Remember which NPC this interaction belongs to: the inline runner
        // routes the prologue's `0x4C 0x51` NPC-run ops to this slot.
        self.active_inline_slot = Some(slot);
        let inline = self.field_npc_dialog.get(&slot).cloned();
        let opened_dialog = if let Some(ref text) = inline {
            self.open_field_dialog(text.clone());
            true
        } else {
            false
        };
        // A scripted-encounter carrier on this slot (the sparring partner):
        // - no inline text -> engages immediately on interaction;
        // - dialogue with the 4-option spar picker -> a `CarrierMenu` gates the
        //   engage on the fight option (faithful);
        // - dialogue without a picker -> the any-accept `pending_carrier_engage`.
        if let Some(&carrier_idx) = self.field_carrier_slots.get(&slot) {
            if !opened_dialog {
                self.engage_field_carrier(carrier_idx);
            } else if let Some((n, fight_option)) = inline.as_deref().and_then(spar_menu_of) {
                self.carrier_menu = Some(CarrierMenu {
                    carrier_idx,
                    n,
                    fight_option,
                    cursor: 0,
                });
            } else {
                self.pending_carrier_engage = Some(carrier_idx);
            }
        }
        self.pending_field_events
            .push(crate::field_events::FieldEvent::FieldInteract { interact_id, slot });
    }

    /// Open a field dialogue box from an inline interaction-script buffer (the
    /// text is the buffer itself; the retail box geometry isn't pinned, so the
    /// box coords are zero). Sets [`Self::current_dialog`] and surfaces a
    /// [`FieldEvent::OpenDialog`].
    fn open_field_dialog(&mut self, inline: Vec<u8>) {
        self.current_dialog = Some(DialogRequest {
            text_id: 0,
            inline: inline.clone(),
            world_x: 0,
            world_z: 0,
            depth_id: 0,
        });
        self.pending_field_events
            .push(crate::field_events::FieldEvent::OpenDialog {
                text_id: 0,
                inline,
                world_x: 0,
                world_z: 0,
                depth_id: 0,
            });
    }

    /// While a carrier's spar [`CarrierMenu`] is up, handle Up/Down navigation
    /// and the confirm. Returns `true` when a menu is active (so the generic
    /// dialog-dismiss must skip - the menu owns the input). On **confirm**:
    /// engages the carrier iff the cursor is on the fight option, else just
    /// closes the box (the other options' talk branches, which the engine does
    /// not run); **Circle** cancels (closes, no engage); Up/Down move the cursor
    /// without closing. Mirrors the retail inline-picker cursor.
    pub(crate) fn handle_carrier_menu(&mut self) -> bool {
        use crate::input::PadButton;
        if self.current_dialog.is_none() {
            // Box closed elsewhere; drop a stale menu.
            self.carrier_menu = None;
            return false;
        }
        let Some(menu) = self.carrier_menu else {
            return false;
        };
        if self.dialog_input_consumed {
            return true; // already handled this tick
        }
        let confirm = self.input.just_pressed(PadButton::Cross);
        let cancel = self.input.just_pressed(PadButton::Circle);
        if confirm || cancel {
            self.dialog_input_consumed = true;
            self.carrier_menu = None;
            self.current_dialog = None;
            self.pending_field_events
                .push(crate::field_events::FieldEvent::DialogDismissed);
            if confirm && menu.cursor == menu.fight_option {
                self.engage_field_carrier(menu.carrier_idx);
            }
            return true;
        }
        let up = self.input.just_pressed(PadButton::Up);
        let down = self.input.just_pressed(PadButton::Down);
        if up || down {
            let mut m = menu;
            if up {
                m.cursor = m.cursor.saturating_sub(1);
            }
            if down && m.cursor + 1 < m.n {
                m.cursor += 1;
            }
            self.carrier_menu = Some(m);
            self.dialog_input_consumed = true;
        }
        true
    }

    /// Clean-room interaction probe - retail `FUN_801cf9f4`, the action-button
    /// adjacency test that talks to a nearby field NPC.
    ///
    /// Mirrors [`Self::tick_world_map_npc_dialog`] for field mode: a single
    /// handler for both opening and dismissing a field dialogue on player input,
    /// so a probe-opened box (talking to an NPC by walking up to it, with no
    /// script `0x4C` poll) still dismisses.
    ///
    /// - **Box up:** a just-pressed Cross / Circle dismisses it (and engages a
    ///   pending scripted-encounter carrier - the dialogue-accept).
    /// - **No box:** a just-pressed Cross runs the retail facing probe
    ///   ([`Self::field_interact_probe_slot`]: the `DAT_801f2254` compass
    ///   point 64 units ahead, ±72 box); a hit opens that NPC's dialogue via
    ///   [`Self::trigger_field_interact`] and turns the player toward it
    ///   ([`Self::face_field_npc`]).
    ///
    /// The [`Self::dialog_input_consumed`] per-tick guard keeps this and the
    /// field VM's `0x4C` dialog poll from both acting on the same button edge.
    /// No-op without a player actor or installed NPC positions.
    ///
    /// PORT: FUN_801cf9f4
    /// REF: FUN_8003A1E4, FUN_80024C88
    pub(crate) fn tick_field_interaction_probe(&mut self) {
        use crate::input::PadButton;
        let confirm = self.input.just_pressed(PadButton::Cross);
        let cancel = self.input.just_pressed(PadButton::Circle);

        if self.current_dialog.is_some() {
            // A carrier's spar menu owns the input while it is up (navigate +
            // confirm the fight option); only then does the generic dismiss run.
            if self.handle_carrier_menu() {
                return;
            }
            // The inline-script runner, when active, owns box dismissal.
            if self.inline_dialogue.is_none() && (confirm || cancel) && !self.dialog_input_consumed
            {
                self.dialog_input_consumed = true;
                self.current_dialog = None;
                self.pending_field_events
                    .push(crate::field_events::FieldEvent::DialogDismissed);
                if let Some(idx) = self.pending_carrier_engage.take() {
                    self.engage_field_carrier(idx);
                }
            }
            return;
        }

        if self.dialog_input_consumed || !confirm || self.field_npc_positions.is_empty() {
            return;
        }
        // Retail geometry: a single facing-indexed compass probe 64 units
        // ahead, box-tested at ±72 against each NPC
        // ([`Self::field_interact_probe_slot`]). A hit posts the touch event
        // on the matched actor and turns the player toward it - the
        // face-the-NPC step retail applies to moving-class partners
        // (`flags & 0x20010 == 0x20000`), which every talk NPC is
        // (capture-pinned by `rimelm_npc_press_tetsu`).
        if let Some(npc_slot) = self.field_interact_probe_slot() {
            self.dialog_input_consumed = true;
            self.trigger_field_interact(0, npc_slot);
            self.face_field_npc(npc_slot);
        }
    }

    /// Host signal that the player engaged field carrier `idx` (accepted the
    /// Tetsu "Come at me!" dialogue / pressed confirm on the NPC). Advances the
    /// carrier's `FUN_801DA51C` SM from Idle to **Activating** and drains its
    /// countdown to zero, so the next `Self::tick_field_carriers` runs the
    /// state-1 body in full: `on_activating` (formation copy) immediately
    /// followed by the `case 2/3` fall-through scene-transition (battle
    /// handoff). No-op for an out-of-range index or a non-Idle carrier.
    ///
    /// Mirrors retail's scripted state-0 -> state-1 advance (towns are 0%
    /// random, so the Tetsu carrier never self-advances via the encounter
    /// roll; the dialogue script drives it).
    ///
    /// REF: FUN_801DA51C
    pub fn engage_field_carrier(&mut self, idx: usize) {
        if let Some(ctx) = self.field_carriers.get_mut(idx)
            && ctx.state == vm::world_map::EntityState::Idle as u16
        {
            ctx.state = vm::world_map::EntityState::Activating as u16;
        }
    }

    /// Step every installed field carrier SM one frame (the field-mode use of
    /// the ported `FUN_801DA51C`), then resolve a latched scripted-encounter
    /// transition into a battle. No-op when no carriers are installed.
    ///
    /// The entity list is taken out of the world so the SM's host bridge can
    /// borrow `&mut World` (same pattern as [`Self::tick_world_map`]).
    ///
    /// REF: FUN_801DA51C
    pub(crate) fn tick_field_carriers(&mut self) {
        if !self.field_carriers.is_empty() {
            let mut carriers = std::mem::take(&mut self.field_carriers);
            for (idx, ctx) in carriers.iter_mut().enumerate() {
                let mut host = FieldCarrierHostImpl { world: self };
                vm::world_map::step(idx, ctx, &mut host);
            }
            self.field_carriers = carriers;
        }

        // The latched battle entry is shared with the field-VM op-`3E FF`
        // scripted-battle arm ([`Self::trigger_scripted_battle`]), which can
        // fire in a scene with no installed carriers - drain it regardless.
        if let Some(formation_id) = self.pending_field_carrier_battle.take() {
            self.begin_field_carrier_battle(formation_id);
        }
    }

    /// Resolve a field carrier's latched `formation_id` against
    /// [`Self::formation_table`] and flip Field -> Battle, snapshotting the
    /// field context so [`Self::finish_battle`] returns to [`SceneMode::Field`].
    /// No-op when the id isn't registered.
    fn begin_field_carrier_battle(&mut self, formation_id: u16) {
        // Reuse the field-encounter battle entry: a carrier transition is a
        // forced encounter against a registered MAN formation.
        self.begin_encounter_battle(crate::encounter::EncounterRoll {
            formation_id,
            row_index: 0,
            roll_q8: 0,
        });
    }

    /// Resolve `formation_id` against [`Self::formation_table`] and flip from
    /// the world map into a battle, snapshotting the world-map context so
    /// [`Self::finish_battle`] returns to [`SceneMode::WorldMap`]. No-op when
    /// the id isn't registered (the encounter is simply dropped).
    pub(crate) fn begin_world_map_encounter(&mut self, formation_id: u16) {
        let Some(formation) = self.formation_table.formation(formation_id).cloned() else {
            return;
        };
        self.field_return = Some(FieldReturnState {
            actors: self.actors.clone(),
            player_actor_slot: self.player_actor_slot,
            party_count: self.party_count,
        });
        self.battle_return_mode = SceneMode::WorldMap;
        // `enter_battle_from_formation` swaps to the battle BGM itself.
        self.enter_battle_from_formation(&formation);
        self.active_formation = Some(formation);
    }
}
