//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

impl PlayWindowApp {
    /// Title-tab label for a sub-screen's small banner window (descriptor
    /// ids 0..=4). Text-only: the retail tab draws a brown banner sprite
    /// with the label; until that art is ported, the label lands at the
    /// tab window's pinned content origin.
    fn menu_tab_title_draws(&self, tab_id: usize, label: &str) -> Vec<TextDraw> {
        let pen = self.menu_window_pen(tab_id);
        legaia_engine_render::text_draws_for(
            &self.font.layout_ascii(label),
            pen,
            [1.0, 0.95, 0.75, 1.0],
        )
    }

    /// Build [`TextDraw`]s for an active field-menu sub-session. Each
    /// variant maps to the matching `*_draws_for` helper in
    /// `legaia-engine-render`. Renderer-side state stays in this method
    /// so the sub-session enums in `legaia-engine-core` can stay
    /// renderer-agnostic.
    pub(super) fn field_menu_sub_draws(
        &self,
        sub: &legaia_engine_core::field_menu_dispatch::FieldMenuSubsession,
    ) -> Vec<TextDraw> {
        use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
        match sub {
            FieldMenuSubsession::Status(s) => {
                use legaia_asset::menu_windows::window_ids;
                let Some(snap) = s.current() else {
                    return Vec::new();
                };
                let stat_rows: Vec<legaia_engine_render::StatusStatRow<'_>> = snap
                    .stats
                    .iter()
                    .zip(snap.stat_labels.iter())
                    .map(|((live, growth), l)| legaia_engine_render::StatusStatRow {
                        label: l,
                        value: *live as u32,
                        growth: *growth as u32,
                    })
                    .collect();
                let equip_rows: Vec<(&str, &str)> = snap
                    .equip
                    .iter()
                    .map(|e| (e.label, e.item_name.as_str()))
                    .collect();
                let view = legaia_engine_render::StatusPanelView {
                    name: &snap.name,
                    level: snap.level,
                    xp: snap.xp,
                    xp_to_next: snap.xp_to_next,
                    hp: snap.hp,
                    hp_max: snap.hp_max,
                    mp: snap.mp,
                    mp_max: snap.mp_max,
                    ap: snap.ap,
                    ap_max: snap.ap_max,
                    stat_rows: &stat_rows,
                    equip_rows: &equip_rows,
                };
                // Main panel content at the pinned FUN_801D33D8 offsets,
                // hung off the id-28 window's content origin; satellites
                // (party list / Condition pager / summary) + the screen
                // tab fill their own pinned windows.
                let mut d = legaia_engine_render::status_screen_draws_for(
                    &self.font,
                    &view,
                    Some("L1/R1: Switch  Circle: Back"),
                    self.menu_window_pen(window_ids::STATUS_MAIN),
                    // LV / HP / MP drawn as sprites from the UI-icon atlas
                    // (see `field_menu_chrome_sprite_draws`); skip the text.
                    self.save_menu.is_some(),
                );
                let names: Vec<&str> = s.snapshots().iter().map(|m| m.name.as_str()).collect();
                let sat = legaia_engine_render::StatusSatelliteView {
                    party_names: &names,
                    cursor: s.cursor() as usize,
                    name: &snap.name,
                    level: snap.level,
                };
                d.extend(legaia_engine_render::status_satellite_draws_for(
                    &self.font,
                    &sat,
                    self.menu_window_pen(window_ids::STATUS_PARTY_LIST),
                    self.menu_window_pen(window_ids::STATUS_CONDITION),
                    self.menu_window_pen(window_ids::STATUS_SUMMARY),
                ));
                d.extend(self.menu_tab_title_draws(window_ids::TAB_STATUS, "Status"));
                d
            }
            FieldMenuSubsession::Config(s) => {
                use legaia_asset::menu_windows::window_ids;
                let rows = s.state().rows();
                let row_views: Vec<legaia_engine_render::OptionsRowView<'_>> = rows
                    .iter()
                    .map(|r| legaia_engine_render::OptionsRowView {
                        label: r.label,
                        value: &r.value,
                    })
                    .collect();
                let mut d = legaia_engine_render::options_draws_for(
                    &self.font,
                    &row_views,
                    s.cursor(),
                    self.menu_window_pen(window_ids::OPTIONS_MAIN),
                );
                d.extend(self.menu_tab_title_draws(window_ids::TAB_OPTIONS, "Options"));
                d
            }
            FieldMenuSubsession::Save(s) => {
                use legaia_engine_core::save_select::SelectPhase;
                let rows: Vec<legaia_engine_render::SaveSelectRow<'_>> = s
                    .slots()
                    .iter()
                    .map(|snap| legaia_engine_render::SaveSelectRow {
                        label: &snap.label,
                        present: snap.present,
                        party_lv: snap.party_lv,
                        play_time_seconds: snap.play_time_seconds,
                        money: snap.money,
                        location: &snap.location,
                    })
                    .collect();
                let (cursor, confirm) = match s.phase() {
                    SelectPhase::Browsing { cursor } => (cursor as usize, None),
                    // Load-mode NowChecking / SlotPreview phases render
                    // separately (see slot_preview_draws / now_checking
                    // overlay below); pass through to a plain cursor.
                    SelectPhase::NowChecking { slot, .. } | SelectPhase::SlotPreview { slot } => {
                        (slot as usize, None)
                    }
                    SelectPhase::ConfirmOverwrite { slot, cursor } => {
                        (slot as usize, Some(("Overwrite slot?", cursor)))
                    }
                    SelectPhase::ConfirmDelete { slot, cursor } => {
                        (slot as usize, Some(("Delete slot?", cursor)))
                    }
                    SelectPhase::Done(_) => return Vec::new(),
                };
                // Field-menu Save subsession reuses the load-screen
                // chrome stage so the panel/pill sprites match retail
                // positions even when entered mid-game.
                let (sw, sh) = self
                    .win
                    .renderer()
                    .map(|r| r.surface_size())
                    .unwrap_or((1, 1));
                let (stage_origin, stage_scale) = self.save_select_stage(sw, sh);
                let emit_text_cursor = self.save_menu.is_none();
                legaia_engine_render::save_select_draws_for(
                    &self.font,
                    "Save",
                    &rows,
                    cursor,
                    confirm,
                    stage_origin,
                    stage_scale,
                    emit_text_cursor,
                )
            }
            FieldMenuSubsession::Spells(s) => {
                use legaia_engine_core::spell_menu::SpellMenuPhase;
                let names: Vec<&str> = s.party().iter().map(|c| c.name.as_str()).collect();
                let hp: Vec<(u16, u16)> = s.party().iter().map(|c| (c.hp, c.hp)).collect();
                let mp: Vec<(u16, u16)> = s.party().iter().map(|c| (c.mp, c.mp)).collect();
                let spell_rows = s.current_spell_rows();
                let spell_views: Vec<legaia_engine_render::SpellRowView<'_>> = spell_rows
                    .iter()
                    .map(|sr| legaia_engine_render::SpellRowView {
                        name: sr.name.as_str(),
                        mp_cost: sr.mp_cost,
                        admissible: sr.admissible,
                    })
                    .collect();
                let target_views: Vec<legaia_engine_render::SpellTargetView<'_>> = s
                    .targets()
                    .iter()
                    .map(|t| legaia_engine_render::SpellTargetView {
                        name: t.name.as_str(),
                        hp: t.hp,
                        hp_max: t.hp_max,
                        alive: t.alive(),
                    })
                    .collect();
                let (selected_caster, selected_spell, phase, cursor) = match s.phase() {
                    SpellMenuPhase::CharSelect { cursor } => (None, None, 0u8, *cursor),
                    SpellMenuPhase::SpellSelect { caster, cursor } => {
                        (Some(*caster), None, 1u8, *cursor)
                    }
                    SpellMenuPhase::TargetSelect {
                        caster,
                        spell_id,
                        cursor,
                    } => (Some(*caster), Some(*spell_id), 2u8, *cursor),
                    SpellMenuPhase::Done(_) => return Vec::new(),
                };
                let names_arr: Vec<&str> = names.to_vec();
                let args = legaia_engine_render::SpellMenuDrawArgs {
                    party_names: &names_arr,
                    party_hp: &hp,
                    party_mp: &mp,
                    selected_caster,
                    spells: &spell_views,
                    selected_spell,
                    targets: &target_views,
                    selected_target: None,
                    cursor,
                    phase,
                };
                legaia_engine_render::spell_menu_draws_for(&self.font, args, (32, 32))
            }
            FieldMenuSubsession::Items(s) => self.items_session_draws(s),
            FieldMenuSubsession::Equip { session, char_slot } => {
                self.equip_session_draws(session, *char_slot)
            }
            FieldMenuSubsession::Arts(s) => self.arts_session_draws(s),
        }
    }

    /// Build draws for the inventory item-use overlay. Resolves item
    /// names through `ItemCatalog`, party / monster targets through the
    /// session's `targets` field. Drives both browsing and target-select
    /// phases via `inventory_use_draws_for`.
    pub(super) fn items_session_draws(
        &self,
        s: &legaia_engine_core::inventory_use::InventoryUseSession,
    ) -> Vec<TextDraw> {
        use legaia_engine_core::inventory_use::InventoryUseState;
        // Each visible item row needs its name + count + admissibility.
        // The session's `filtered_items` already lists indices into
        // `items` that pass the context filter; we render every owned
        // item but dim the ones outside the filter.
        let filter_set: std::collections::HashSet<usize> =
            s.filtered_items.iter().copied().collect();
        // Count duplicate item-ids so the overlay shows one row per
        // unique id rather than one row per stack slot.
        let mut counts: std::collections::HashMap<u8, u8> = std::collections::HashMap::new();
        for id in &s.items {
            *counts.entry(*id).or_insert(0) =
                counts.get(id).copied().unwrap_or(0).saturating_add(1);
        }
        // Stable order from first-seen.
        let mut seen: std::collections::HashSet<u8> = std::collections::HashSet::new();
        let mut row_data: Vec<(String, u8, bool)> = Vec::new();
        for (i, id) in s.items.iter().enumerate() {
            if !seen.insert(*id) {
                continue;
            }
            let entry = s.catalog.get(*id);
            let name = entry
                .map(|e| e.name.to_string())
                .unwrap_or_else(|| format!("Item {id:02X}"));
            let count = counts.get(id).copied().unwrap_or(1);
            let admissible = filter_set.contains(&i);
            row_data.push((name, count, admissible));
        }
        let item_rows: Vec<legaia_engine_render::InventoryItemRow<'_>> = row_data
            .iter()
            .map(|(n, c, a)| legaia_engine_render::InventoryItemRow {
                name: n,
                count: *c,
                admissible: *a,
            })
            .collect();
        let target_rows: Vec<legaia_engine_render::InventoryTargetRow<'_>> = s
            .targets
            .iter()
            .map(|t| legaia_engine_render::InventoryTargetRow {
                name: &t.name,
                hp: t.hp,
                hp_max: t.hp_max,
                mp: t.mp,
                mp_max: t.mp_max,
                alive: t.alive,
            })
            .collect();
        let (phase, cursor) = match s.state {
            InventoryUseState::Browsing { cursor } => (0u8, cursor as u8),
            InventoryUseState::TargetSelect { cursor, .. } => (1u8, cursor as u8),
            _ => (0u8, 0),
        };
        let selected_item_name = s.current_item().map(|e| e.name);
        let in_battle = matches!(
            s.context,
            legaia_engine_core::inventory_use::InventoryContext::Battle
        );
        let args = legaia_engine_render::InventoryUseDrawArgs {
            items: &item_rows,
            targets: &target_rows,
            in_battle,
            cursor,
            phase,
            selected_item_name,
        };
        legaia_engine_render::inventory_use_draws_for(&self.font, args, (16, 32))
    }

    /// Build draws for the equipment overlay. Resolves slot labels
    /// through `EquipSlot::label`, candidate names from the engine's
    /// equipment catalog, and per-candidate stat deltas by diffing the
    /// active modifier against the slot's current occupant.
    pub(super) fn equip_session_draws(
        &self,
        session: &legaia_engine_core::equip_session::EquipSession,
        char_slot: u8,
    ) -> Vec<TextDraw> {
        use legaia_engine_core::equip_session::EquipState;
        use legaia_engine_core::equipment::EquipSlot;

        // Display name comes from the world's roster snapshot; fall back
        // to "Slot N" if the world doesn't have a record for the slot.
        let names = legaia_engine_core::field_menu_dispatch::roster_names(&self.session.host.world);
        let character_name = names
            .get(char_slot as usize)
            .cloned()
            .unwrap_or_else(|| format!("Slot {}", char_slot + 1));

        let record = session.record();
        let mut slot_label_buf: Vec<String> = Vec::with_capacity(8);
        for i in 0..8u8 {
            let label = EquipSlot::from_index(i)
                .map(|s| s.label().to_string())
                .unwrap_or_else(|| format!("Slot {i}"));
            slot_label_buf.push(label);
        }
        let mut slot_item_buf: Vec<String> = Vec::with_capacity(8);
        for &id in record.equip.iter() {
            slot_item_buf.push(if id == 0 {
                "(empty)".to_string()
            } else {
                format!("Item {id:02X}")
            });
        }
        let slot_rows: Vec<legaia_engine_render::EquipSlotRow<'_>> = (0..8usize)
            .map(|i| legaia_engine_render::EquipSlotRow {
                label: &slot_label_buf[i],
                current_name: &slot_item_buf[i],
            })
            .collect();

        let (phase, cursor, active_slot, confirm_label_owned) = match session.state() {
            EquipState::SlotPicker { cursor } => (
                legaia_engine_render::EquipDrawPhase::SlotPicker,
                cursor as u16,
                cursor,
                None,
            ),
            EquipState::ItemPicker { slot, cursor } => (
                legaia_engine_render::EquipDrawPhase::ItemPicker,
                cursor,
                slot,
                None,
            ),
            EquipState::Confirm {
                slot,
                item_id,
                cursor,
            } => {
                let label = format!("Equip Item {item_id:02X}?");
                (
                    legaia_engine_render::EquipDrawPhase::Confirm,
                    cursor as u16,
                    slot,
                    Some(label),
                )
            }
            EquipState::Done(_) => (legaia_engine_render::EquipDrawPhase::SlotPicker, 0, 0, None),
        };

        // Candidates only matter when we're past the slot picker.
        let (candidate_names, candidate_meta): (Vec<String>, Vec<(u8, i16, i16)>) =
            if phase == legaia_engine_render::EquipDrawPhase::SlotPicker {
                (Vec::new(), Vec::new())
            } else {
                let items = session.items_for_slot(active_slot);
                let current_id = record.equip[active_slot as usize];
                let current_mod = session
                    .equipment()
                    .get(current_id)
                    .copied()
                    .unwrap_or_default();
                let names: Vec<String> = items
                    .iter()
                    .map(|it| format!("Item {:02X}", it.id))
                    .collect();
                let meta: Vec<(u8, i16, i16)> = items
                    .iter()
                    .map(|it| {
                        let cand_mod = session.equipment().get(it.id).copied().unwrap_or_default();
                        let count = session.inventory().get(&it.id).copied().unwrap_or(0);
                        (
                            count,
                            cand_mod.atk - current_mod.atk,
                            cand_mod.udf - current_mod.udf,
                        )
                    })
                    .collect();
                (names, meta)
            };
        let candidate_rows: Vec<legaia_engine_render::EquipCandidateRow<'_>> = candidate_meta
            .iter()
            .enumerate()
            .map(
                |(i, (count, da, du))| legaia_engine_render::EquipCandidateRow {
                    name: &candidate_names[i],
                    count: *count,
                    atk_delta: *da,
                    udf_delta: *du,
                },
            )
            .collect();

        let args = legaia_engine_render::EquipDrawArgs {
            character_name: &character_name,
            slots: &slot_rows,
            candidates: &candidate_rows,
            phase,
            cursor,
            active_slot,
            confirm_label: confirm_label_owned.as_deref(),
        };
        legaia_engine_render::equipment_session_draws_for(&self.font, args, (16, 32))
    }

    /// Build draws for the Tactical Arts editor overlay. Pulls the
    /// saved-chain library snapshot the editor took at construction; the
    /// editor's `library_view` is the authoritative source until the
    /// engine calls `apply_outcome`.
    pub(super) fn arts_session_draws(
        &self,
        s: &legaia_engine_core::tactical_arts_editor::ChainEditor,
    ) -> Vec<TextDraw> {
        use legaia_engine_core::tactical_arts_editor::{ChainLibrary, EditorPhase};
        let char_slot = s.char_slot();
        let names = legaia_engine_core::field_menu_dispatch::roster_names(&self.session.host.world);
        let character_name = names
            .get(char_slot as usize)
            .cloned()
            .unwrap_or_else(|| format!("Slot {}", char_slot + 1));

        let saved = s.library_view();
        let pretty_buf: Vec<String> = saved.iter().map(|c| c.pretty_sequence()).collect();
        let saved_rows: Vec<legaia_engine_render::ArtsChainRow<'_>> = saved
            .iter()
            .enumerate()
            .map(|(i, c)| legaia_engine_render::ArtsChainRow {
                name: &c.name,
                pretty_sequence: &pretty_buf[i],
            })
            .collect();

        let (phase_tag, browse_cursor, editing_pretty_owned, editing_len, naming_name_owned) =
            match s.phase() {
                EditorPhase::Browsing { cursor } => (
                    legaia_engine_render::ArtsEditorPhase::Browsing,
                    *cursor,
                    String::new(),
                    0usize,
                    String::new(),
                ),
                EditorPhase::Editing { working } => {
                    let pretty = working
                        .iter()
                        .map(|c| match c {
                            legaia_art::queue::Command::Left => "L",
                            legaia_art::queue::Command::Right => "R",
                            legaia_art::queue::Command::Up => "U",
                            legaia_art::queue::Command::Down => "D",
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    (
                        legaia_engine_render::ArtsEditorPhase::Editing,
                        0u8,
                        pretty,
                        working.len(),
                        String::new(),
                    )
                }
                EditorPhase::Naming { working, name } => {
                    let pretty = working
                        .iter()
                        .map(|c| match c {
                            legaia_art::queue::Command::Left => "L",
                            legaia_art::queue::Command::Right => "R",
                            legaia_art::queue::Command::Up => "U",
                            legaia_art::queue::Command::Down => "D",
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    (
                        legaia_engine_render::ArtsEditorPhase::Naming,
                        0u8,
                        pretty,
                        working.len(),
                        name.clone(),
                    )
                }
                EditorPhase::Done(_) => (
                    legaia_engine_render::ArtsEditorPhase::Browsing,
                    0u8,
                    String::new(),
                    0usize,
                    String::new(),
                ),
            };

        let can_add_new = saved.len() < ChainLibrary::MAX_SLOTS;
        let args = legaia_engine_render::ArtsEditorDrawArgs {
            character_name: &character_name,
            phase: phase_tag,
            saved: &saved_rows,
            browse_cursor,
            editing_pretty: &editing_pretty_owned,
            editing_len,
            min_len: ChainLibrary::MIN_LEN,
            max_len: ChainLibrary::MAX_LEN,
            naming_name: &naming_name_owned,
            can_add_new,
        };
        legaia_engine_render::tactical_arts_editor_draws_for(&self.font, args, (16, 32))
    }

    /// Render the seru-trade screens of the shop menu: the offer list
    /// (`ShopTrade`) or the yes/no confirm (`ShopTradeConfirm`). Each offer is
    /// labelled "give (owner) -> receive" with names from the boot SCUS.
    pub(super) fn draw_shop_trade(
        &self,
        out: &mut Vec<TextDraw>,
        state: Option<MenuState>,
        cursor: usize,
    ) {
        let name_of = |id: u8| -> String {
            self.seru_names
                .as_ref()
                .and_then(|t| t.name(id))
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("Seru {id:02X}"))
        };
        let owner_of = |slot: u8| -> String {
            self.session
                .host
                .world
                .roster
                .members
                .get(slot as usize)
                .map(|m| m.name())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("P{slot}"))
        };
        match state {
            Some(MenuState::ShopTrade) => {
                let mut labels: Vec<String> = Vec::new();
                match self.menu_runtime.trade_session.as_ref() {
                    Some(t) if !t.offers.is_empty() => {
                        for o in &t.offers {
                            labels.push(format!(
                                "{} ({}) -> {}",
                                name_of(o.give.seru_id),
                                owner_of(o.give.owner_slot),
                                name_of(o.receive_seru_id),
                            ));
                        }
                    }
                    _ => labels.push("(no trades offered)".to_string()),
                }
                let rows: Vec<ShopRow<'_>> = labels
                    .iter()
                    .map(|l| ShopRow {
                        label: l.as_str(),
                        price: None,
                    })
                    .collect();
                out.extend(shop_draws_for(
                    &self.font,
                    "SHOP - TRADE SERU",
                    &rows,
                    cursor,
                    None,
                    (8, 140),
                ));
            }
            Some(MenuState::ShopTradeConfirm) => {
                let title = match self.menu_runtime.pending_trade_offer() {
                    Some(o) => format!(
                        "Trade {} for {}?",
                        name_of(o.give.seru_id),
                        name_of(o.receive_seru_id),
                    ),
                    None => "Trade?".to_string(),
                };
                let rows = vec![
                    ShopRow {
                        label: "Yes",
                        price: None,
                    },
                    ShopRow {
                        label: "No",
                        price: None,
                    },
                ];
                out.extend(shop_draws_for(
                    &self.font,
                    &title,
                    &rows,
                    cursor,
                    None,
                    (8, 140),
                ));
            }
            _ => {}
        }
    }
}
