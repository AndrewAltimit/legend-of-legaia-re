//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

impl PlayWindowApp {
    /// Title-tab label for a sub-screen's small banner window (descriptor
    /// ids 0..=4). Text-only: the carved plaque behind it is chrome,
    /// drawn from the UI-icon atlas by `field_menu_chrome_sprite_draws`
    /// (`legaia_engine_render::tab_banner_draws`). The label lands at the
    /// tab window's pinned content origin in the retail CLUT-7 text
    /// white.
    ///
    /// REF: FUN_801DCAD8 - the Status tab's content renderer (label
    /// string at `(a0+0xa, a0+0xc)`, staged text CLUT 7). The five tab
    /// renderers share one shape, so the draw itself lives in the shared
    /// builder `tab_label_draws` and this method only resolves the pen.
    fn menu_tab_title_draws(&self, tab_id: usize, label: &str) -> Vec<TextDraw> {
        let pen = self.menu_window_pen(tab_id);
        legaia_engine_render::tab_label_draws(&self.font, label, pen)
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
                // Retail's status screen carries no footer hint line -
                // navigation is implicit (L1/R1 pages, Circle backs out).
                let mut d = legaia_engine_render::status_screen_draws_for(
                    &self.font,
                    &view,
                    None,
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
                    // Hand cursor / pager triangles / LV + ATR icons drawn
                    // as sprites (`status_satellite_icon_sprites_for`).
                    self.save_menu.is_some(),
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
                        value: r.value,
                        teal: r.teal,
                        advance: r.advance,
                    })
                    .collect();
                let popup = s.popup().map(|p| legaia_engine_render::OptionsPopupDraw {
                    rect: self.options_popup_rect(&p),
                    choices: p.choices,
                    cursor: p.cursor,
                });
                let mut d = legaia_engine_render::options_draws_for(
                    &self.font,
                    &row_views,
                    s.cursor(),
                    popup.as_ref(),
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
                let cursor = match s.phase() {
                    SelectPhase::Browsing { cursor } => cursor as usize,
                    SelectPhase::NowChecking { slot, .. }
                    | SelectPhase::SlotPreview { slot }
                    | SelectPhase::ConfirmOverwrite { slot, .. }
                    | SelectPhase::ConfirmDelete { slot, .. } => slot as usize,
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
                // The title word comes from the session's MODE, not
                // from which menu row opened it: the field menu's Load
                // row builds the same sub-session shape as its Save
                // row (`FieldMenuSubsession::Save` with
                // `SaveSelectMode::Load`), and retail's header tab
                // toggles its string on the same direction flag
                // (`_DAT_801f0200`). A hardcoded "Save" here made the
                // in-game Load screen carry the Save title.
                let mut out = legaia_engine_render::save_select_draws_for(
                    &self.font,
                    save_select_title_word(s),
                    &rows,
                    cursor,
                    None,
                    stage_origin,
                    stage_scale,
                    emit_text_cursor,
                );
                // Phase overlays (NowChecking dialog text, slot-info
                // panel text / captions, confirm messagebox) - shared
                // with the boot Continue → Load screen so the two
                // paths render identically.
                out.extend(save_select_phase_text_draws(
                    &self.font,
                    s,
                    stage_origin,
                    stage_scale,
                    self.save_menu.is_some(),
                ));
                out
            }
            FieldMenuSubsession::Spells(s) => self.pause_magic_draws(s),
            FieldMenuSubsession::Items(s) => self.pause_items_draws(s),
            FieldMenuSubsession::Equip { session, char_slot } => {
                self.equip_session_draws(session, *char_slot)
            }
            FieldMenuSubsession::Arts(s) => self.arts_session_draws(s),
        }
    }

    /// Generic spell-menu overlay - the target-select fallback while the
    /// retail target-pick window layout stays unpinned.
    fn spell_session_generic_draws(
        &self,
        s: &legaia_engine_core::spell_menu::SpellMenuSession,
    ) -> Vec<TextDraw> {
        use legaia_engine_core::spell_menu::SpellMenuPhase;
        {
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
    }

    /// Build draws for the retail **Magic** screen: caster window (id 19),
    /// spell-list page (id 18), spell info window (id 20) and the "Magic"
    /// title tab (id 1), each at its disc-parsed descriptor rect via the
    /// shared engine-ui builder. Session data (mp/mp_max, learned levels,
    /// descriptions) comes from the engine-core view model; during
    /// target-select the generic overlay stands in (the retail target-pick
    /// window layout is unpinned).
    pub(super) fn pause_magic_draws(
        &self,
        s: &legaia_engine_core::spell_menu::SpellMenuSession,
    ) -> Vec<TextDraw> {
        use legaia_asset::menu_windows::window_ids;
        let world = &self.session.host.world;
        let model =
            legaia_engine_core::pause_screens::magic_screen_model(s, world.menu_text.as_ref());
        if model.target_select {
            return self.spell_session_generic_draws(s);
        }
        let casters: Vec<legaia_engine_render::PauseMagicCaster<'_>> = model
            .casters
            .iter()
            .map(
                |(name, level, mp, mp_max)| legaia_engine_render::PauseMagicCaster {
                    name,
                    level: *level as u16,
                    mp: *mp,
                    mp_max: *mp_max,
                },
            )
            .collect();
        let rows: Vec<legaia_engine_render::PauseMagicRow<'_>> = model
            .page_rows
            .iter()
            .map(|(name, ra_seru)| legaia_engine_render::PauseMagicRow {
                name,
                ra_seru: *ra_seru,
            })
            .collect();
        let info = model
            .info
            .as_ref()
            .map(|i| legaia_engine_render::PauseMagicInfo {
                name: &i.name,
                level: i.level,
                desc: &i.desc,
                mp_cost: i.mp_cost,
            });
        let view = legaia_engine_render::PauseMagicView {
            casters: &casters,
            rows: &rows,
            page: model.page,
            pages: model.pages,
            phase: if model.focus_list {
                legaia_engine_render::PauseMagicPhase::List
            } else {
                legaia_engine_render::PauseMagicPhase::Caster
            },
            caster_cursor: model.caster_cursor,
            list_cursor: model.list_cursor_on_page,
            info,
            // LV / MP tags + hand cursor come from the UI-icon atlas when
            // it's resident (see `field_menu_chrome_sprite_draws`).
            label_icons: self.save_menu.is_some(),
            text_cursor: self.save_menu.is_none(),
        };
        let mut d = legaia_engine_render::magic_screen_draws_for(
            &self.font,
            &view,
            self.menu_window_pen(window_ids::MAGIC_CASTER),
            self.menu_window_pen(window_ids::MAGIC_LIST),
            self.menu_window_pen(window_ids::MAGIC_INFO),
        );
        d.extend(self.menu_tab_title_draws(window_ids::TAB_MAGIC, "Magic"));
        d
    }

    /// Build draws for the retail **Items** screen: command window (id
    /// 13), item-list page (id 15), item info window (id 17, plus its
    /// extra widget box) and the "Items" title tab (id 0). Rows carry the
    /// real bag counts + disc descriptions from the session; during
    /// target-select the generic overlay stands in.
    pub(super) fn pause_items_draws(
        &self,
        s: &legaia_engine_core::pause_screens::PauseItemsSession,
    ) -> Vec<TextDraw> {
        use legaia_asset::menu_windows::window_ids;
        let model = legaia_engine_core::pause_screens::items_screen_model(s);
        if model.target_select {
            return self.items_session_draws(&s.inner);
        }
        let rows: Vec<legaia_engine_render::PauseItemsRow<'_>> = model
            .page_rows
            .iter()
            .map(|(name, count)| legaia_engine_render::PauseItemsRow {
                name,
                count: *count,
            })
            .collect();
        let info = model
            .info
            .as_ref()
            .map(|i| legaia_engine_render::PauseItemInfo {
                name: &i.name,
                count: i.count,
                desc: &i.desc,
                passive: i.passive.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
            });
        let view = legaia_engine_render::PauseItemsView {
            rows: &rows,
            page: model.page,
            pages: model.pages,
            phase: if model.focus_list {
                legaia_engine_render::PauseItemsPhase::List
            } else {
                legaia_engine_render::PauseItemsPhase::Command
            },
            command_cursor: model.command_cursor,
            list_cursor: model.list_cursor_on_page,
            bag_empty: model.bag_empty,
            info,
            text_cursor: self.save_menu.is_none(),
        };
        let mut d = legaia_engine_render::items_screen_draws_for(
            &self.font,
            &view,
            self.menu_window_pen(window_ids::ITEMS_COMMAND),
            self.menu_window_pen(window_ids::ITEMS_LIST),
            self.menu_window_pen(window_ids::ITEMS_INFO),
        );
        d.extend(self.menu_tab_title_draws(window_ids::TAB_ITEMS, "Items"));
        // Throw Out confirm prompt (descriptor id 9, renderer FUN_801D1B20):
        // the Yes/No window that slides in over the command window. The
        // window frame is chrome (still caller-pending); the text overlay
        // sits at the descriptor pen, falling back to the pinned rect.
        if let Some(confirm) = model.throw_confirm.as_ref() {
            let pen = self.menu_window_pen(9);
            let pen = if pen == (0, 0) {
                let (x, y, _, _) = legaia_engine_render::ITEMS_THROW_CONFIRM_RECT;
                (x, y)
            } else {
                pen
            };
            let view = legaia_engine_render::PauseThrowConfirmView {
                name: &confirm.name,
                count: confirm.count,
                cursor: confirm.cursor,
                text_cursor: self.save_menu.is_none(),
            };
            d.extend(legaia_engine_render::items_throw_confirm_draws_for(
                &self.font, &view, pen,
            ));
        }
        // Special Use-route confirm (submenu 0xB Door of Light -> window
        // 10 / `FUN_801D1DAC`, submenu 0xD Incense -> window 12 /
        // `FUN_801D1F10`). A different window and renderer from the Throw
        // Out confirm above, and the cursor seeds to Yes rather than No.
        //
        // Retail's own prompt strings live in the menu overlay's data
        // segment (the `0x801CEA94` block the renderer's `lui`/`addiu`
        // pairs point at) and are not recovered, so the port stages the
        // item name and its own question in the retail line slots; the
        // geometry - which is what the renderer actually is - is exact.
        if let Some(sc) = model.special_confirm.as_ref() {
            let two_line = matches!(
                sc.route,
                legaia_engine_core::pause_screens::UseRoute::Incense
            );
            let prompt_lines = if two_line { 2 } else { 1 };
            let (win_id, fallback) = legaia_engine_render::use_confirm_window(prompt_lines);
            let pen = self.menu_window_pen(win_id);
            let pen = if pen == (0, 0) {
                (fallback.0, fallback.1)
            } else {
                pen
            };
            let one_line = format!("Use {}?", sc.item_name);
            let lines: Vec<&str> = if two_line {
                vec![sc.item_name.as_str(), "Use it?"]
            } else {
                vec![one_line.as_str()]
            };
            d.extend(legaia_engine_render::confirm_prompt_draws(
                &self.font,
                &lines,
                &["Yes", "No"],
                pen,
            ));
            if self.save_menu.is_none() {
                let (hx, hy) =
                    legaia_engine_render::confirm_prompt_hand_pos(pen, prompt_lines, sc.cursor);
                d.extend(legaia_engine_render::text_draws_for(
                    &self.font.layout_ascii(">"),
                    (hx, hy),
                    legaia_engine_render::MENU_TEXT_GOLD,
                ));
            }
        }
        d
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

    /// Build draws for the equip screen in the retail multi-window
    /// layout: party window (id 21), item-list window (id 23), main
    /// window (id 22) and the "Equip" tab (id 2), each at its
    /// disc-parsed descriptor rect. Slot labels resolve through
    /// `EquipSlot::label`; the stat-compare block diffs
    /// `compute_battle_stats` with the hovered candidate installed.
    pub(super) fn equip_session_draws(
        &self,
        session: &legaia_engine_core::equip_session::EquipSession,
        char_slot: u8,
    ) -> Vec<TextDraw> {
        use legaia_asset::menu_windows::window_ids;
        use legaia_engine_core::equip_session::EquipState;
        use legaia_engine_core::equipment::EquipSlot;

        // Party-window rows come from the world's roster snapshot.
        let names = legaia_engine_core::field_menu_dispatch::roster_names(&self.session.host.world);
        let party_names: Vec<&str> = names.iter().map(String::as_str).collect();

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
                String::new()
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

        // Candidates + stat compare only matter past the slot picker.
        let (candidate_names, candidate_counts, considered_id): (Vec<String>, Vec<u8>, Option<u8>) =
            if phase == legaia_engine_render::EquipDrawPhase::SlotPicker {
                (Vec::new(), Vec::new(), None)
            } else {
                let items = session.items_for_slot(active_slot);
                let names: Vec<String> = items
                    .iter()
                    .map(|it| format!("Item {:02X}", it.id))
                    .collect();
                let counts: Vec<u8> = items
                    .iter()
                    .map(|it| session.inventory().get(&it.id).copied().unwrap_or(0))
                    .collect();
                // The item the compare block previews: the hovered row in
                // the picker, the pending item in the confirm phase.
                let considered = match session.state() {
                    EquipState::Confirm { item_id, .. } => Some(item_id),
                    _ => items.get(cursor as usize).map(|it| it.id),
                };
                (names, counts, considered)
            };
        let candidate_rows: Vec<legaia_engine_render::EquipCandidateRow<'_>> = candidate_names
            .iter()
            .zip(candidate_counts.iter())
            .map(|(name, count)| legaia_engine_render::EquipCandidateRow {
                name,
                count: *count,
            })
            .collect();

        // Stat-compare block: current vs candidate-installed stats. The
        // session recomputes with live status modifiers on commit; the
        // menu preview uses the neutral status set (field-menu context).
        let stat_compare: Vec<legaia_engine_render::EquipStatRow<'_>> = match considered_id {
            Some(id) => {
                let neutral = legaia_engine_core::battle_stats::StatusModifiers::default();
                let cur = legaia_engine_core::battle_stats::compute_battle_stats(
                    record,
                    session.equipment(),
                    &[],
                    &neutral,
                );
                let mut copy = *record;
                copy.equip[active_slot as usize] = id;
                let new = legaia_engine_core::battle_stats::compute_battle_stats(
                    &copy,
                    session.equipment(),
                    &[],
                    &neutral,
                );
                // The three retail compare rows (FUN_801D21C0 stat block).
                vec![
                    legaia_engine_render::EquipStatRow {
                        label: "ATK",
                        current: cur.atk,
                        preview: new.atk,
                    },
                    legaia_engine_render::EquipStatRow {
                        label: "UDF",
                        current: cur.udf,
                        preview: new.udf,
                    },
                    legaia_engine_render::EquipStatRow {
                        label: "LDF",
                        current: cur.ldf,
                        preview: new.ldf,
                    },
                ]
            }
            None => Vec::new(),
        };

        let view = legaia_engine_render::EquipScreenView {
            party_names: &party_names,
            party_cursor: char_slot as usize,
            slots: &slot_rows,
            candidates: &candidate_rows,
            stat_compare: &stat_compare,
            phase,
            cursor,
            active_slot,
            confirm_label: confirm_label_owned.as_deref(),
            // Hand-cursor sprites come from the system-UI atlas when it's
            // resident (see `field_menu_chrome_sprite_draws`).
            text_cursor: self.save_menu.is_none(),
        };
        let mut d = legaia_engine_render::equip_screen_draws_for(
            &self.font,
            &view,
            self.menu_window_pen(window_ids::EQUIP_PARTY),
            self.menu_window_pen(window_ids::EQUIP_LIST),
            self.menu_window_pen(window_ids::EQUIP_MAIN),
        );
        d.extend(self.menu_tab_title_draws(window_ids::TAB_EQUIP, "Equip"));
        d
    }

    /// Build draws for the Tactical Arts editor overlay.
    ///
    /// The projection out of the live `ChainEditor` (character name, the
    /// pretty-printed sequences, the phase tag, the "+ New" room check)
    /// is `field_menu_dispatch::arts_editor_view`, shared with the browser
    /// play page - only the borrow into `ArtsEditorDrawArgs` is per host.
    pub(super) fn arts_session_draws(
        &self,
        s: &legaia_engine_core::tactical_arts_editor::ChainEditor,
    ) -> Vec<TextDraw> {
        let view =
            legaia_engine_core::field_menu_dispatch::arts_editor_view(s, &self.session.host.world);
        let saved_rows: Vec<legaia_engine_render::ArtsChainRow<'_>> = view
            .saved
            .iter()
            .map(|(name, pretty)| legaia_engine_render::ArtsChainRow {
                name,
                pretty_sequence: pretty,
            })
            .collect();
        let args = legaia_engine_render::ArtsEditorDrawArgs {
            character_name: &view.character_name,
            phase: arts_phase_tag(view.phase),
            saved: &saved_rows,
            browse_cursor: view.browse_cursor,
            editing_pretty: &view.editing_pretty,
            editing_len: view.editing_len,
            min_len: view.min_len,
            max_len: view.max_len,
            naming_name: &view.naming_name,
            can_add_new: view.can_add_new,
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
                    .map(|l| ShopRow::new(l.as_str(), None))
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
                let rows = vec![ShopRow::new("Yes", None), ShopRow::new("No", None)];
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

/// Map the shared `engine-core` arts-editor phase tag onto the `engine-ui`
/// one. Two enums exist because `engine-ui` is the wgpu-free leaf and does
/// not depend on `engine-core`; this is the only place they meet on the
/// native side.
fn arts_phase_tag(
    phase: legaia_engine_core::field_menu_dispatch::ArtsEditorPhaseTag,
) -> legaia_engine_render::ArtsEditorPhase {
    use legaia_engine_core::field_menu_dispatch::ArtsEditorPhaseTag as Tag;
    match phase {
        Tag::Browsing => legaia_engine_render::ArtsEditorPhase::Browsing,
        Tag::Editing => legaia_engine_render::ArtsEditorPhase::Editing,
        Tag::Naming => legaia_engine_render::ArtsEditorPhase::Naming,
    }
}
