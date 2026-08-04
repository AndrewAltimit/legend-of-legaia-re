//! The native `play-window`'s half of the pause menu: projecting the live
//! `engine-core` sessions into the plain view structs the shared composition
//! takes.
//!
//! The composition itself - which windows a screen frames, which painter
//! draws its title tab, where the modals sit in the sprite order and the
//! final stage scale - lives in
//! [`legaia_engine_ui::pause_menu`](legaia_engine_render::pause_menu), so the
//! browser play page runs the same one. This module used to own a private
//! copy of it, which is both a tier-7 drift risk and the reason no library
//! test could enter a single pause-menu draw builder.

use super::*;
use legaia_engine_render::pause_menu::{
    EquipComposeInput, GenericContent, ItemsScreenView, MagicScreenView, MenuRects,
    OptionsScreenView, PauseMenuCtx, PauseMenuDraws, PauseScreen, SpecialConfirmView,
    StatusScreenView, TopLevelView, equip_screen_compose, pause_screen_draws,
    spell_level_notice_draws,
};

impl PlayWindowApp {
    /// The shared pause-menu composition context for this frame: the font,
    /// the descriptor-rect resolver, the chrome atlas band rects (absent
    /// without a disc) and the boot-UI stage transform.
    pub(super) fn menu_ctx(&self, surface_w: u32, surface_h: u32) -> PauseMenuCtx<'_> {
        let (origin, scale) = self.save_select_stage(surface_w, surface_h);
        PauseMenuCtx {
            font: &self.font,
            rects: MenuRects::new(self.menu_window_table.as_ref()),
            chrome: self.save_menu.as_ref().map(|a| &a.rects),
            origin,
            scale,
        }
    }

    /// Windows 6 and 5 - the pair the pause menu draws when the op-`0x49`
    /// entry context's kind byte is `0x0D`.
    ///
    /// Retail opens the menu on sub-screen `4` for that kind and routes the
    /// root picker's cancel to sub-screen `3`
    /// (`FUN_801DC6B4` `0x801dc8d0..0x801dc8e4`; `FUN_801D6B20`
    /// `0x801d6cf8..0x801d6d18`), and each of those sub-screens hands the
    /// widget VM a one-command open script - `[open window 6]` and
    /// `[open window 5]`. Both are content-only draws off the disc-parsed
    /// rect, and both take their text from the overlay's own string pool
    /// via [`legaia_engine_core::pause_screens::ContextLockedLabels`], so
    /// no label is invented here.
    pub(super) fn context_locked_screen_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> PauseMenuDraws {
        let Some(menu) = self.session.field_menu.as_ref() else {
            return PauseMenuDraws::default();
        };
        let ctx = self.menu_ctx(surface_w, surface_h);
        let labels = &self.session.host.world.menu_context_labels;
        if menu.notice_is_up() {
            let lines: Vec<&str> = labels.notice_lines.iter().map(String::as_str).collect();
            return pause_screen_draws(&ctx, PauseScreen::ContextNotice { lines: &lines });
        }
        if let Some(cursor_row) = menu.ready_confirm_cursor() {
            return pause_screen_draws(
                &ctx,
                PauseScreen::ContextReady {
                    headings: [
                        labels.ready_headings[0].as_str(),
                        labels.ready_headings[1].as_str(),
                    ],
                    choices: [labels.choices[0].as_str(), labels.choices[1].as_str()],
                    // The painter's flag word is the shared cursor word
                    // `FUN_801D688C` maintains: the low 12 bits are the
                    // selected row and the `0x1000` bit inverts the marker.
                    // Sub-screen 3 keeps the plain, non-editing form.
                    cursor: u32::from(cursor_row),
                },
            );
        }
        PauseMenuDraws::default()
    }

    /// Window 7 - the spell level-up notice, drawn while
    /// [`legaia_engine_core::menu_runtime::MenuRuntime`] holds the beat a
    /// leveled menu cast armed (`apply_spell_outcome` ->
    /// `arm_spell_level_notice`). Retail's cast sub-screens hand the widget
    /// VM the one-command script `0x801E4D50` / `0x801E4D78` (`[open window
    /// 7]`) when the `FUN_80035C00` sentinel pair changed, then stall for a
    /// press - the input side of that stall lives in the field-menu arm of
    /// `tick_boot_ui`.
    pub(super) fn magic_level_notice_draws(&self, surface_w: u32, surface_h: u32) -> Vec<TextDraw> {
        let Some(notice) = self.menu_runtime.spell_level_notice() else {
            return Vec::new();
        };
        spell_level_notice_draws(&self.menu_ctx(surface_w, surface_h), &notice.line)
    }

    /// Top-level pause menu: the command list, the money / play-time box and
    /// the party overview panel.
    ///
    /// The menu session lives on the `BootSession` (the headless host of the
    /// CARD/menu mode); the window only renders it.
    pub(super) fn field_menu_root_draws(&self, surface_w: u32, surface_h: u32) -> PauseMenuDraws {
        let Some(menu) = self.session.field_menu.as_ref() else {
            return PauseMenuDraws::default();
        };
        let view = menu.view();
        let rows: Vec<legaia_engine_render::FieldMenuRowView<'_>> = view
            .rows
            .iter()
            .map(|r| legaia_engine_render::FieldMenuRowView {
                label: r.label,
                enabled: r.enabled,
            })
            .collect();
        let snaps =
            legaia_engine_core::field_menu_dispatch::status_snapshots(&self.session.host.world);
        let party: Vec<legaia_engine_render::FieldMenuPartyView<'_>> = snaps
            .iter()
            .map(|s| legaia_engine_render::FieldMenuPartyView {
                name: &s.name,
                level: s.level,
                hp: s.hp,
                hp_max: s.hp_max,
                mp: s.mp,
                mp_max: s.mp_max,
                ap: s.ap as u16,
            })
            .collect();
        let party_ap: Vec<u16> = snaps.iter().map(|s| s.ap as u16).collect();
        pause_screen_draws(
            &self.menu_ctx(surface_w, surface_h),
            PauseScreen::TopLevel(TopLevelView {
                rows: &rows,
                cursor: view.cursor,
                money: view.money,
                play_time_seconds: view.play_time_seconds,
                party: &party,
                party_ap: &party_ap,
            }),
        )
    }

    /// Build the draw lists for an active field-menu sub-session. Each
    /// variant projects the live session onto the plain view structs the
    /// shared composition takes; the Save sub-session is the exception - it
    /// renders through the save-select chrome stage
    /// (`save_select_chrome_sprite_draws` + `boot_ui_draws`) it shares with
    /// the boot Continue -> Load screen, and so is handled by its caller.
    pub(super) fn field_menu_sub_draws(
        &self,
        sub: &legaia_engine_core::field_menu_dispatch::FieldMenuSubsession,
        surface_w: u32,
        surface_h: u32,
    ) -> PauseMenuDraws {
        use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
        let ctx = self.menu_ctx(surface_w, surface_h);
        match sub {
            FieldMenuSubsession::Status(s) => {
                let Some(snap) = s.current() else {
                    return PauseMenuDraws::default();
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
                let panel = legaia_engine_render::StatusPanelView {
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
                let names: Vec<&str> = s.snapshots().iter().map(|m| m.name.as_str()).collect();
                let satellite = legaia_engine_render::StatusSatelliteView {
                    party_names: &names,
                    cursor: s.cursor() as usize,
                    name: &snap.name,
                    level: snap.level,
                };
                pause_screen_draws(
                    &ctx,
                    PauseScreen::Status(StatusScreenView {
                        panel: &panel,
                        satellite: &satellite,
                        ap: snap.ap as u16,
                        // ATR icon by roster character id (slot) of the
                        // highlighted member; the icon set is Vahn / Noa /
                        // Gala in character order.
                        atr_char: snap.slot as usize,
                    }),
                )
            }
            FieldMenuSubsession::Config(s) => {
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
                // Selected-row pointing hand at `x-10` on the cursor row
                // (retail's FUN_8002b994 kind-0 cursor, shared with the
                // status party list).
                let row_y_off: i32 = rows
                    .iter()
                    .take(s.cursor() as usize)
                    .map(|r| r.advance)
                    .sum();
                pause_screen_draws(
                    &ctx,
                    PauseScreen::Options(OptionsScreenView {
                        rows: &row_views,
                        cursor: s.cursor(),
                        popup,
                        row_y_off,
                    }),
                )
            }
            // The Save sub-session's screen belongs to the save-select
            // stage, not to this composition - see the doc comment.
            FieldMenuSubsession::Save(_) => PauseMenuDraws::default(),
            FieldMenuSubsession::Spells(s) => self.pause_magic_draws(s, &ctx),
            FieldMenuSubsession::Items(s) => self.pause_items_draws(s, &ctx),
            FieldMenuSubsession::Equip { session, char_slot } => {
                self.equip_session_draws(session, *char_slot, &ctx)
            }
            FieldMenuSubsession::Arts(s) => self.arts_session_draws(s, &ctx),
        }
    }

    /// Text half of the field menu's Load / Save sub-screen.
    ///
    /// Not part of [`Self::field_menu_sub_draws`]'s shared composition: this
    /// screen is the **save-select** surface, and the native window reaches
    /// the same one from the boot Continue -> Load path
    /// (`BootUiState::SaveSelect`). Both go through
    /// `save_select_phase_text_draws` + `save_select_chrome_sprite_draws` so
    /// the in-game and boot entries cannot drift from each other; hoisting
    /// only the pause half would have forked them. Pre-scaled to surface
    /// coords, so the caller must not scale it again.
    pub(super) fn field_save_sub_draws(
        &self,
        s: &legaia_engine_core::save_select::SaveSelectSession,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<TextDraw> {
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
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        // The title word comes from the session's MODE, not from which menu
        // row opened it: the field menu's Load row builds the same
        // sub-session shape as its Save row, and retail's header tab toggles
        // its string on the same direction flag (`_DAT_801f0200`).
        let mut out = legaia_engine_render::save_select_draws_for(
            &self.font,
            save_select_title_word(s),
            &rows,
            cursor,
            None,
            stage_origin,
            stage_scale,
            self.save_menu.is_none(),
        );
        out.extend(save_select_phase_text_draws(
            &self.font,
            s,
            &self.save_flow,
            stage_origin,
            stage_scale,
            self.save_menu.is_some(),
        ));
        out
    }

    /// Build draws for the retail **Magic** screen: caster window (id 19),
    /// spell-list page (id 18), spell info window (id 20) and the "Magic"
    /// title tab (id 1). Session data (mp/mp_max, learned levels,
    /// descriptions) comes from the engine-core view model; during
    /// target-select the generic overlay stands in (the retail target-pick
    /// window layout is unpinned).
    pub(super) fn pause_magic_draws(
        &self,
        s: &legaia_engine_core::spell_menu::SpellMenuSession,
        ctx: &PauseMenuCtx<'_>,
    ) -> PauseMenuDraws {
        use legaia_engine_core::spell_menu::SpellMenuPhase;
        let world = &self.session.host.world;
        let model =
            legaia_engine_core::pause_screens::magic_screen_model(s, world.menu_text.as_ref());
        if !model.target_select {
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
                // LV / MP tags + hand cursor come from the UI-icon atlas
                // when it is resident.
                label_icons: ctx.chrome_present(),
                text_cursor: !ctx.chrome_present(),
            };
            return pause_screen_draws(
                ctx,
                PauseScreen::Magic(MagicScreenView {
                    view: &view,
                    casters: model.casters.len(),
                }),
            );
        }
        // Generic spell-menu overlay - the target-select fallback while the
        // retail target-pick window layout stays unpinned.
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
            SpellMenuPhase::SpellSelect { caster, cursor } => (Some(*caster), None, 1u8, *cursor),
            SpellMenuPhase::TargetSelect {
                caster,
                spell_id,
                cursor,
            } => (Some(*caster), Some(*spell_id), 2u8, *cursor),
            SpellMenuPhase::Done(_) => return PauseMenuDraws::default(),
        };
        let args = legaia_engine_render::SpellMenuDrawArgs {
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
        pause_screen_draws(ctx, PauseScreen::Generic(GenericContent::SpellMenu(args)))
    }

    /// Build draws for the retail **Items** screen: command window (id
    /// 13), item-list page (id 15), item info window (id 17, plus its
    /// extra widget box) and the "Items" title tab (id 0). Rows carry the
    /// real bag counts + disc descriptions from the session; during
    /// target-select retail swaps the item list for window 14, the party
    /// target panel (`FUN_801D0520`), and the generic inventory overlay
    /// only stands in when that panel has no roster to draw.
    pub(super) fn pause_items_draws(
        &self,
        s: &legaia_engine_core::pause_screens::PauseItemsSession,
        ctx: &PauseMenuCtx<'_>,
    ) -> PauseMenuDraws {
        let model = legaia_engine_core::pause_screens::items_screen_model(s);
        if model.target_select {
            if let Some(panel) = self.pause_target_panel_view(s) {
                let members = Self::target_panel_members(&panel);
                let view = legaia_engine_render::TargetPanelView {
                    members: &members,
                    mode: legaia_engine_render::TargetPanelMode::from_preview_word(panel.mode),
                    cursor: Self::target_panel_cursor(&panel),
                    // The LV / HP / MP tags come from the UI atlas whenever
                    // the sprite pass runs; without it the ASCII stand-ins
                    // carry the layout.
                    label_icons: ctx.chrome_present(),
                    text_cursor: !ctx.chrome_present(),
                };
                return pause_screen_draws(ctx, PauseScreen::ItemsTarget(&view));
            }
            return pause_screen_draws(
                ctx,
                PauseScreen::Generic(GenericContent::Prebuilt(self.items_session_draws(&s.inner))),
            );
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
            text_cursor: !ctx.chrome_present(),
        };
        let throw =
            model
                .throw_confirm
                .as_ref()
                .map(|c| legaia_engine_render::PauseThrowConfirmView {
                    name: &c.name,
                    count: c.count,
                    cursor: c.cursor,
                    text_cursor: !ctx.chrome_present(),
                });
        // Retail's own Use-route prompt strings live in the menu overlay's
        // data segment (the `0x801CEA94` block the renderer's `lui`/`addiu`
        // pairs point at) and are not recovered, so the port stages the item
        // name and its own question in the retail line slots; the geometry -
        // which is what the renderer actually is - is exact.
        let special_one_line = model
            .special_confirm
            .as_ref()
            .map(|sc| format!("Use {}?", sc.item_name));
        let special_lines: Vec<&str> = match (model.special_confirm.as_ref(), &special_one_line) {
            (Some(sc), Some(one)) => {
                if matches!(
                    sc.route,
                    legaia_engine_core::pause_screens::UseRoute::Incense
                ) {
                    vec![sc.item_name.as_str(), "Use it?"]
                } else {
                    vec![one.as_str()]
                }
            }
            _ => Vec::new(),
        };
        pause_screen_draws(
            ctx,
            PauseScreen::Items(ItemsScreenView {
                view: &view,
                // The shared info panel's Point Card arm: retail branches on
                // the staged id being `0xFE` and prints the live bank instead
                // of the passive lines (`FUN_801D0F1C` at `0x801d0fd0`).
                point_card: model
                    .info
                    .as_ref()
                    .filter(|i| i.is_point_card)
                    .map(|_| self.session.host.world.point_card.max(0) as u32),
                throw_confirm: throw.as_ref(),
                special_confirm: model.special_confirm.as_ref().map(|sc| SpecialConfirmView {
                    lines: &special_lines,
                    cursor: sc.cursor,
                }),
            }),
        )
    }

    /// Map the engine-core target-panel model onto the engine-ui view.
    pub(super) fn target_panel_members(
        model: &legaia_engine_core::pause_screens::TargetPanelModel,
    ) -> Vec<legaia_engine_render::TargetPanelMember<'_>> {
        model
            .members
            .iter()
            .map(|m| legaia_engine_render::TargetPanelMember {
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

    /// Cursor decode shared by the panel's text + sprite builders.
    pub(super) fn target_panel_cursor(
        model: &legaia_engine_core::pause_screens::TargetPanelModel,
    ) -> legaia_engine_render::TargetPanelCursor {
        if model.all_targets {
            legaia_engine_render::TargetPanelCursor::All { pressed: false }
        } else {
            legaia_engine_render::TargetPanelCursor::Single {
                row: model.cursor_row,
                pressed: false,
            }
        }
    }

    /// The window-14 target-panel model for an Items session in target
    /// select, or `None` when the flow is not there or has no rows to draw.
    pub(super) fn pause_target_panel_view(
        &self,
        s: &legaia_engine_core::pause_screens::PauseItemsSession,
    ) -> Option<legaia_engine_core::pause_screens::TargetPanelModel> {
        legaia_engine_core::pause_screens::target_panel_view_model(s, &self.session.host.world)
            .filter(|m| !m.members.is_empty())
    }

    /// Build draws for the inventory item-use overlay. Resolves item
    /// names through `ItemCatalog`, party / monster targets through the
    /// session's `targets` field. Drives both browsing and target-select
    /// phases via `inventory_use_draws_for`.
    ///
    /// Returned unscaled: the shared composition frames it and applies the
    /// stage transform ([`GenericContent::Prebuilt`]).
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

    /// Build draws for the equip screen in the retail multi-window layout:
    /// party window (id 21), item-list window (id 23), main window (id 22)
    /// and the "Equip" tab (id 2). The projection - slot labels, the
    /// candidate list, the stat compare against `compute_battle_stats` - is
    /// the shared `engine-core` model.
    pub(super) fn equip_session_draws(
        &self,
        session: &legaia_engine_core::equip_session::EquipSession,
        char_slot: u8,
        ctx: &PauseMenuCtx<'_>,
    ) -> PauseMenuDraws {
        let names = legaia_engine_core::field_menu_dispatch::roster_names(&self.session.host.world);
        let model =
            legaia_engine_core::pause_screens::equip_screen_model(session, char_slot, &names);
        equip_screen_compose(ctx, &equip_compose_input(&model, !ctx.chrome_present()))
    }

    /// Build draws for the Tactical Arts chain editor overlay.
    ///
    /// The projection out of the live `ChainEditor` (character name, the
    /// pretty-printed sequences, the phase tag, the "+ New" room check)
    /// is `field_menu_dispatch::arts_editor_view`, shared with the browser
    /// play page - only the borrow into `ArtsEditorDrawArgs` is per host.
    pub(super) fn arts_session_draws(
        &self,
        s: &legaia_engine_core::tactical_arts_editor::ChainEditor,
        ctx: &PauseMenuCtx<'_>,
    ) -> PauseMenuDraws {
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
        pause_screen_draws(ctx, PauseScreen::Generic(GenericContent::Arts(args)))
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
                    super::hud::SHOP_OVERLAY_PEN,
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
                    super::hud::SHOP_OVERLAY_PEN,
                ));
            }
            _ => {}
        }
    }
}

/// Borrow the shared `engine-core` Equip screen model into the shared
/// `engine-ui` compose input.
///
/// The only per-host line left on this screen: the phase tag has to cross
/// from `engine-core`'s enum to `engine-ui`'s, and `engine-ui` deliberately
/// does not depend on `engine-core`. The browser twin is
/// `web-viewer/src/play_menu.rs::equip_compose_input`.
fn equip_compose_input(
    m: &legaia_engine_core::pause_screens::EquipScreenModel,
    text_cursor: bool,
) -> EquipComposeInput<'_> {
    use legaia_engine_core::pause_screens::EquipScreenPhase as Tag;
    EquipComposeInput {
        party_names: &m.party_names,
        slot_labels: &m.slot_labels,
        slot_items: &m.slot_items,
        candidate_names: &m.candidate_names,
        candidate_counts: &m.candidate_counts,
        stat_compare: &m.stat_compare,
        phase: match m.phase {
            Tag::SlotPicker => legaia_engine_render::EquipDrawPhase::SlotPicker,
            Tag::ItemPicker => legaia_engine_render::EquipDrawPhase::ItemPicker,
            Tag::Confirm => legaia_engine_render::EquipDrawPhase::Confirm,
        },
        cursor: m.cursor,
        active_slot: m.active_slot,
        confirm_label: m.confirm_label.as_deref(),
        char_slot: m.char_slot as usize,
        slot_cursor: m.slot_cursor,
        pictogram_rows: m.pictogram_rows,
        text_cursor,
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
