//! The list sub-screens' builders: save select, items, spells, equip, the
//! items-session draws and the options popup rect, plus the engine-core to
//! engine-ui view adapters they feed.

use super::*;

/// Map the engine-core window-14 target-panel model onto the engine-ui view
/// rows. Mirrors the native shell's `target_panel_members`.
fn target_panel_members(
    model: &legaia_engine_menus::pause_screens::TargetPanelModel,
) -> Vec<ui::TargetPanelMember<'_>> {
    model
        .members
        .iter()
        .map(|m| ui::TargetPanelMember {
            name: m.name.as_str(),
            level: m.level,
            hp: m.hp,
            mp: m.mp,
            hp_max: m.hp_max,
            mp_max: m.mp_max,
            base_hp_max: m.base_hp_max,
            base_mp_max: m.base_mp_max,
            stat_eff: m.stat_eff,
            stat_base: m.stat_base,
        })
        .collect()
}

/// Hand-cursor decode of the target panel: all-party picks put the hand on
/// every row (retail cursor bit `0x2000`), otherwise it sits on one row.
fn target_panel_cursor(
    model: &legaia_engine_menus::pause_screens::TargetPanelModel,
) -> ui::TargetPanelCursor {
    if model.all_targets {
        ui::TargetPanelCursor::All { pressed: false }
    } else {
        ui::TargetPanelCursor::Single {
            row: model.cursor_row,
            pressed: false,
        }
    }
}

/// Borrow the shared `engine-core` Equip screen model into the shared
/// `engine-ui` compose input.
///
/// The only per-host line left on this screen: the phase tag has to cross
/// from `engine-core`'s enum to `engine-ui`'s, and `engine-ui` deliberately
/// does not depend on `engine-core`. Everything either side of it - the
/// projection and the composition - is shared. The native window's twin is
/// `window/menu_draws.rs::equip_compose_input`.
fn equip_compose_input(
    m: &legaia_engine_menus::pause_screens::EquipScreenModel,
    text_cursor: bool,
) -> ui::pause_menu::EquipComposeInput<'_> {
    use legaia_engine_menus::pause_screens::EquipScreenPhase as Tag;
    ui::pause_menu::EquipComposeInput {
        party_names: &m.party_names,
        slot_labels: &m.slot_labels,
        slot_items: &m.slot_items,
        candidate_names: &m.candidate_names,
        candidate_counts: &m.candidate_counts,
        stat_compare: &m.stat_compare,
        best_changes: &m.best_changes,
        phase: match m.phase {
            Tag::SlotPicker => ui::EquipDrawPhase::SlotPicker,
            Tag::ItemPicker => ui::EquipDrawPhase::ItemPicker,
            Tag::Confirm => ui::EquipDrawPhase::Confirm,
        },
        cursor: m.cursor,
        active_slot: m.active_slot,
        confirm_label: m.confirm_label.as_deref(),
        char_slot: m.party_row as usize,
        slot_cursor: m.slot_cursor,
        pictogram_rows: m.pictogram_rows,
        text_cursor,
        info: m
            .info
            .as_ref()
            .map(|i| legaia_engine_ui::pause_menu::EquipInfoInput {
                name: i.name.as_str(),
                count: i.count,
                desc: i.desc.as_str(),
                passive: i.passive.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
            }),
        compare: m.compare.as_ref().map(equip_compare_input),
    }
}

/// Borrow the shared Equip **compare** model into `engine-ui`'s input. The
/// native window's twin is `window/menu_draws.rs::equip_compare_input`.
fn equip_compare_input(
    c: &legaia_engine_menus::pause_screens::EquipCompareModel,
) -> ui::pause_menu::EquipCompareInput<'_> {
    ui::pause_menu::EquipCompareInput {
        name: c.name.as_str(),
        current: ui::EquipStatBlock::from_words(&c.current),
        candidate: ui::EquipStatBlock::from_words(&c.candidate),
        hp_max: c.hp_max,
        mp_max: c.mp_max,
        slot_row: c.slot_row,
        staged_id: c.staged_id,
        staged_category: c.staged_category,
        equipped_id: c.equipped_id,
        equipped_category: c.equipped_category,
    }
}

impl LegaiaRuntime {
    /// Load / Save sub-screen: the real retail save-select chrome, driven off
    /// the memory-card rack - the native window's `save_select_overlay`
    /// twin, with the pill row bound to the rack's card ports and the preview
    /// grid to the selected card's blocks.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_save_select(
        &self,
        assets: &PlayMenuAssets,
        s: &SaveSelectSession,
        menu: &PlayMenu,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) -> Option<(usize, u8)> {
        // The screen is the shared composition over the engine's overlay
        // sequence (`SaveScreenFlow::overlay_model` ->
        // `save_select_overlay_draws`), the native window's calls too. Its
        // text half draws with or without the chrome atlas: this page used
        // to return before every phase overlay when the atlas was absent,
        // leaving the title and nothing else.
        let m = menu.save_flow.overlay_model(s)?;
        let rows: Vec<ui::SaveSelectRow<'_>> = s
            .slots()
            .iter()
            .map(|slot| ui::SaveSelectRow {
                label: &slot.label,
                present: slot.present,
                party_lv: slot.party_lv,
                play_time_seconds: slot.play_time_seconds,
                money: slot.money,
                location: &slot.location,
            })
            .collect();
        let cells: Vec<SlotGridCell> = m
            .preview
            .iter()
            .flat_map(|p| p.cells.iter())
            .map(|c| SlotGridCell {
                present: c.present,
                portrait_char_id: c.portrait_char_id,
            })
            .collect();
        let preview = m.preview.as_ref().map(|p| ui::SaveSelectPreviewView {
            cells: &cells,
            cell: p.cell,
            info: p.info.map(|b| SlotInfoView {
                slot_no: b.slot.saturating_add(1),
                location: &b.location,
                play_time: &p.play_time,
                leader_name: &b.leader_name,
                leader_level: b.party_lv,
                leader_hp: b.leader_hp,
                leader_mp: b.leader_mp,
                leader_char_id: b.leader_char_id,
            }),
            caption: p.caption,
            panel_y_offset: p.panel_y_offset,
        });
        let view = ui::SaveSelectOverlayView {
            title: m.title,
            rows: &rows,
            cursor: m.cursor,
            single_pill: m.single_pill,
            pills: &m.pills,
            pill_cursor: m.pill_cursor,
            slide_t: m.slide_t,
            info_t: m.info_t,
            now_checking: m.now_checking,
            banner: m.banner.map(|b| ui::CardBannerView {
                lines: b.lines,
                note: b.note,
                work: b.work,
                slide_t: b.slide_t,
                progress_t: b.progress_t,
            }),
            preview,
            confirm: m.confirm,
        };
        let out = ui::save_select_overlay_draws(
            &assets.font,
            assets.chrome.as_ref().map(|(_, rects)| rects),
            &view,
            origin,
            scale,
        );
        let darken = out.darken.map(|d| (sprites.len() + d.sprite, d.level));
        sprites.extend(out.sprites);
        texts.extend(out.texts);
        darken
    }

    /// Items sub-screen: the retail four-window layout (command 13 / list
    /// 15 / info 17 + the "Items" tab) fed from the engine-core session
    /// model. During target-select retail replaces the item list with
    /// window 14 - the party target panel (`FUN_801D0520`); the generic
    /// overlay only stands in when there is no world (and so no roster)
    /// behind the session.
    pub(super) fn build_items(
        &self,
        assets: &PlayMenuAssets,
        s: &legaia_engine_menus::pause_screens::PauseItemsSession,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        let ctx = assets.menu_ctx(origin, scale);
        let model = legaia_engine_menus::pause_screens::items_screen_model(s);
        if model.target_select {
            if let Some(panel) = self
                .menu_world()
                .and_then(|w| {
                    legaia_engine_core::world_views::pause_screens::target_panel_view_model(s, w)
                })
                .filter(|m| !m.members.is_empty())
            {
                let members = target_panel_members(&panel);
                let view = ui::TargetPanelView {
                    members: &members,
                    mode: ui::TargetPanelMode::from_preview_word(panel.mode),
                    cursor: target_panel_cursor(&panel),
                    label_icons: assets.chrome.is_some(),
                    text_cursor: assets.chrome.is_none(),
                };
                let out = pause_screen_draws(&ctx, PauseScreen::ItemsTarget(&view));
                sprites.extend(out.sprites);
                texts.extend(out.texts);
                return;
            }
            let out = pause_screen_draws(
                &ctx,
                PauseScreen::Generic(GenericContent::Prebuilt(
                    self.items_session_draws(assets, &s.inner),
                )),
            );
            sprites.extend(out.sprites);
            texts.extend(out.texts);
            return;
        }
        let rows: Vec<ui::PauseItemsRow<'_>> = model
            .page_rows
            .iter()
            .map(|(name, count)| ui::PauseItemsRow {
                name,
                count: *count,
            })
            .collect();
        let info = model.info.as_ref().map(|i| ui::PauseItemInfo {
            name: &i.name,
            count: i.count,
            desc: &i.desc,
            passive: i.passive.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
        });
        let view = ui::PauseItemsView {
            rows: &rows,
            page: model.page,
            pages: model.pages,
            phase: if model.focus_list {
                ui::PauseItemsPhase::List
            } else {
                ui::PauseItemsPhase::Command
            },
            command_cursor: model.command_cursor,
            list_cursor: model.list_cursor_on_page,
            bag_empty: model.bag_empty,
            info,
            text_cursor: assets.chrome.is_none(),
        };
        let throw = model
            .throw_confirm
            .as_ref()
            .map(|c| ui::PauseThrowConfirmView {
                name: &c.name,
                count: c.count,
                cursor: c.cursor,
                text_cursor: assets.chrome.is_none(),
            });
        // Retail's own Use-route prompt strings live in the menu overlay's
        // unrecovered data segment, so the port stages the item name and its
        // own question in the retail line slots - the geometry, which is what
        // the renderer is, is exact.
        let special_one_line = model
            .special_confirm
            .as_ref()
            .map(|sc| format!("Use {}?", sc.item_name));
        let special_lines: Vec<&str> = match (model.special_confirm.as_ref(), &special_one_line) {
            (Some(sc), Some(one)) => {
                if matches!(
                    sc.route,
                    legaia_engine_menus::pause_screens::UseRoute::Incense
                ) {
                    vec![sc.item_name.as_str(), "Use it?"]
                } else {
                    vec![one.as_str()]
                }
            }
            _ => Vec::new(),
        };
        let point_card = model.info.as_ref().filter(|i| i.is_point_card).map(|_| {
            self.menu_world()
                .map(|w| w.minigames.point_card.max(0) as u32)
                .unwrap_or(0)
        });
        let out = pause_screen_draws(
            &ctx,
            PauseScreen::Items(ItemsScreenView {
                view: &view,
                point_card,
                throw_confirm: throw.as_ref(),
                special_confirm: model.special_confirm.as_ref().map(|sc| SpecialConfirmView {
                    lines: &special_lines,
                    cursor: sc.cursor,
                }),
            }),
        );
        sprites.extend(out.sprites);
        texts.extend(out.texts);
    }

    /// Magic sub-screen: the retail four-window layout (list 18 / caster
    /// 19 / info 20 + the "Magic" tab) fed from the engine-core session
    /// model. During target-select the generic overlay stands in (its
    /// retail window layout is unpinned).
    pub(super) fn build_spells(
        &self,
        assets: &PlayMenuAssets,
        s: &SpellMenuSession,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        let ctx = assets.menu_ctx(origin, scale);
        let model = legaia_engine_menus::pause_screens::magic_screen_model(
            s,
            self.menu_world().and_then(|w| w.menu.text.as_ref()),
        );
        if !model.target_select {
            let casters: Vec<ui::PauseMagicCaster<'_>> = model
                .casters
                .iter()
                .map(|(name, level, mp, mp_max)| ui::PauseMagicCaster {
                    name,
                    level: *level as u16,
                    mp: *mp,
                    mp_max: *mp_max,
                })
                .collect();
            let rows: Vec<ui::PauseMagicRow<'_>> = model
                .page_rows
                .iter()
                .map(|(name, ra_seru)| ui::PauseMagicRow {
                    name,
                    ra_seru: *ra_seru,
                })
                .collect();
            let info = model.info.as_ref().map(|i| ui::PauseMagicInfo {
                name: &i.name,
                level: i.level,
                desc: &i.desc,
                mp_cost: i.mp_cost,
            });
            let view = ui::PauseMagicView {
                casters: &casters,
                rows: &rows,
                page: model.page,
                pages: model.pages,
                phase: if model.focus_list {
                    ui::PauseMagicPhase::List
                } else {
                    ui::PauseMagicPhase::Caster
                },
                caster_cursor: model.caster_cursor,
                list_cursor: model.list_cursor_on_page,
                info,
                label_icons: assets.chrome.is_some(),
                text_cursor: assets.chrome.is_none(),
            };
            let out = pause_screen_draws(
                &ctx,
                PauseScreen::Magic(MagicScreenView {
                    view: &view,
                    casters: model.casters.len(),
                }),
            );
            sprites.extend(out.sprites);
            texts.extend(out.texts);
            return;
        }
        let names: Vec<&str> = s.party().iter().map(|c| c.name.as_str()).collect();
        let hp: Vec<(u16, u16)> = s.party().iter().map(|c| (c.hp, c.hp)).collect();
        let mp: Vec<(u16, u16)> = s.party().iter().map(|c| (c.mp, c.mp)).collect();
        let spell_rows = s.current_spell_rows();
        let spell_views: Vec<ui::SpellRowView<'_>> = spell_rows
            .iter()
            .map(|sr| ui::SpellRowView {
                name: sr.name.as_str(),
                mp_cost: sr.mp_cost,
                admissible: sr.admissible,
            })
            .collect();
        let target_views: Vec<ui::SpellTargetView<'_>> = s
            .targets()
            .iter()
            .map(|t| ui::SpellTargetView {
                name: t.name.as_str(),
                hp: t.hp,
                hp_max: t.hp_max,
                alive: t.alive(),
            })
            .collect();
        let (selected_caster, selected_spell, phase, cursor) = match s.phase() {
            SpellMenuPhase::CharSelect { cursor } => (None, None, 0u8, *cursor),
            SpellMenuPhase::SpellSelect { caster, cursor } => (Some(*caster), None, 1u8, *cursor),
            SpellMenuPhase::TargetSelect {
                caster,
                spell_id,
                cursor,
            } => (Some(*caster), Some(*spell_id), 2u8, *cursor),
            // Retail sub-screen `0x10`, the no-pick group flow: it draws no
            // target rows at all, so the screen is the spell list with the
            // confirmed spell staged in the info window - phase `1`, not `2`.
            SpellMenuPhase::GroupConfirm {
                caster,
                spell_id,
                cursor,
            } => (Some(*caster), Some(*spell_id), 1u8, *cursor),
            SpellMenuPhase::Done(_) => return,
        };
        let args = ui::SpellMenuDrawArgs {
            party_names: &names,
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
        let out = pause_screen_draws(&ctx, PauseScreen::Generic(GenericContent::SpellMenu(args)));
        sprites.extend(out.sprites);
        texts.extend(out.texts);
    }

    /// Equip sub-screen: the retail multi-window layout (party / item-list /
    /// main window + the Equip tab) + the slot pictogram column and hand
    /// cursors.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_equip(
        &self,
        assets: &PlayMenuAssets,
        session: &EquipSession,
        char_slot: u8,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        let ctx = assets.menu_ctx(origin, scale);
        let names = self
            .menu_world()
            .map(field_menu_dispatch::roster_names)
            .unwrap_or_default();
        // Same resolver the native window passes, so both hosts spell the
        // candidates and the info panel out of the disc text tables rather
        // than as raw ids.
        let world = self.menu_world();
        let text = |id: u8| {
            world
                .map(|w| field_menu_dispatch::item_display_text(w, id))
                .unwrap_or_default()
        };
        // Window 25's panel needs the record (HP / MP maxima) and the two
        // disc tables the compare category is looked up in - the same
        // sources the native window passes.
        let record = world.and_then(|w| w.party.roster.members.get(char_slot as usize));
        let compare = record.map(|r| legaia_engine_menus::pause_screens::EquipCompareCtx {
            record: r,
            equip_info: self.menu.equip_info.as_ref(),
            item_effects: world.and_then(|w| w.tables.item_effects.as_ref()),
        });
        let m = legaia_engine_menus::pause_screens::equip_screen_model(
            session,
            char_slot,
            &names,
            &world.map(|w| w.present_party_list()).unwrap_or_default(),
            Some(&text),
            compare,
        );
        let out = equip_screen_compose(&ctx, &equip_compose_input(&m, assets.chrome.is_none()));
        sprites.extend(out.sprites);
        texts.extend(out.texts);
    }

    /// Build the inventory item-use overlay text draws. Ported verbatim from
    /// the native shell's `items_session_draws` so the site emits the identical
    /// draw list. Crate-visible: the battle overlay draws the in-battle Item
    /// submenu through the same projection ([`crate::play_battle`]).
    pub(crate) fn items_session_draws(
        &self,
        assets: &PlayMenuAssets,
        s: &InventoryUseSession,
    ) -> Vec<TextDraw> {
        let font = &assets.font;
        let filter_set: std::collections::HashSet<usize> =
            s.filtered_items.iter().copied().collect();
        let mut counts: std::collections::HashMap<u8, u8> = std::collections::HashMap::new();
        for id in &s.items {
            *counts.entry(*id).or_insert(0) =
                counts.get(id).copied().unwrap_or(0).saturating_add(1);
        }
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
        let item_rows: Vec<ui::InventoryItemRow<'_>> = row_data
            .iter()
            .map(|(n, c, a)| ui::InventoryItemRow {
                name: n,
                count: *c,
                admissible: *a,
            })
            .collect();
        let target_rows: Vec<ui::InventoryTargetRow<'_>> = s
            .targets
            .iter()
            .map(|t| ui::InventoryTargetRow {
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
            legaia_engine_menus::inventory_use::InventoryContext::Battle
        );
        let args = ui::InventoryUseDrawArgs {
            items: &item_rows,
            targets: &target_rows,
            in_battle,
            cursor,
            phase,
            selected_item_name,
        };
        ui::inventory_use_draws_for(font, args, (16, 32))
    }

    /// The options value popup's per-open content rect (its y/h are stamped
    /// from the hovered row) - same helper the native window uses.
    pub(super) fn options_popup_rect(
        &self,
        assets: &PlayMenuAssets,
        popup: &legaia_engine_core::options::OptionsPopup,
    ) -> (i32, i32, i32, i32) {
        use legaia_game_tables::menu_windows::window_ids;
        let (px, _, pw, _) = assets.window_rect(window_ids::OPTIONS_POPUP);
        let (_, sy, _, _) = assets.window_rect(window_ids::OPTIONS_MAIN);
        legaia_engine_core::options::options_popup_content_rect(
            sy,
            px,
            pw,
            popup.row,
            popup.choices.len(),
        )
    }
}
