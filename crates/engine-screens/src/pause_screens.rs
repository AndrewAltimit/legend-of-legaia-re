//! The pause menu's screens projected once: the live `engine-core` /
//! `engine-menus` sessions turned into the plain view structs the shared
//! composition ([`legaia_engine_ui::pause_menu`]) takes.
//!
//! The composition (which windows a screen frames, which painter draws its
//! title tab, the modal order, the stage scale) was already shared; the
//! projection in front of it lived twice - in the native window's
//! `menu_draws.rs` and the browser play page's `play_menu/` - line for line.
//! A host now builds its [`PauseMenuCtx`] and calls one entry per screen.

use legaia_engine_core::field_menu_dispatch::{self, ArtsEditorPhaseTag, FieldMenuSubsession};
use legaia_engine_core::world::World;
use legaia_engine_menus::equipment::DiscEquipInfo;
use legaia_engine_menus::field_menu::FieldMenuSession;
use legaia_engine_menus::pause_screens as ps;
use legaia_engine_menus::spell_menu::{SpellMenuPhase, SpellMenuSession};
use legaia_engine_ui as ui;
use legaia_engine_ui::pause_menu::{
    EquipComposeInput, GenericContent, ItemsScreenView, KeyRebindView, MagicScreenView,
    OptionsScreenView, PauseMenuCtx, PauseMenuDraws, PauseScreen, SpecialConfirmView,
    StatusScreenView, TopLevelView, equip_screen_compose, pause_screen_draws,
};

/// Heading the list-reorder page prints. Retail's page carries the record
/// screen's own chrome, which the port does not draw yet; the label names the
/// page so the screen is not an unheaded list.
pub const LIST_ORDER_TITLE: &str = "ORDER";

/// Windows 6 and 5 - the pair the pause menu draws when the op-`0x49` entry
/// context's kind byte is `0x0D`. Empty when neither is up, which is what
/// lets the caller fall through to the root list.
///
/// Retail opens the menu on sub-screen `4` for that kind and routes the root
/// picker's cancel to sub-screen `3` (`FUN_801DC6B4` `0x801dc8d0..0x801dc8e4`;
/// `FUN_801D6B20` `0x801d6cf8..0x801d6d18`), and each of those sub-screens
/// hands the widget VM a one-command open script - `[open window 6]` and
/// `[open window 5]`. Both are content-only draws off the disc-parsed rect,
/// and both take their text from the overlay's own string pool via
/// [`legaia_engine_menus::pause_screens::ContextLockedLabels`], so no label is
/// invented here.
pub fn context_locked_screen_draws(
    ctx: &PauseMenuCtx<'_>,
    root: &FieldMenuSession,
    world: &World,
) -> PauseMenuDraws {
    let labels = &world.menu.context_labels;
    if root.notice_is_up() {
        let lines: Vec<&str> = labels.notice_lines.iter().map(String::as_str).collect();
        return pause_screen_draws(ctx, PauseScreen::ContextNotice { lines: &lines });
    }
    if let Some(cursor_row) = root.ready_confirm_cursor() {
        return pause_screen_draws(
            ctx,
            PauseScreen::ContextReady {
                headings: [
                    labels.ready_headings[0].as_str(),
                    labels.ready_headings[1].as_str(),
                ],
                choices: [labels.choices[0].as_str(), labels.choices[1].as_str()],
                // The painter's flag word is the shared cursor word
                // `FUN_801D688C` maintains: the low 12 bits are the selected
                // row and the `0x1000` bit inverts the marker. Sub-screen 3
                // keeps the plain, non-editing form.
                cursor: u32::from(cursor_row),
            },
        );
    }
    PauseMenuDraws::default()
}

/// Top-level pause menu: the command list, the money / play-time box and the
/// party overview panel, plus the save-refusal notice when a refused commit
/// landed the player back here (`refusal` = the notice's message).
///
/// Money, play time, cursor and per-row ink all come off the shared session's
/// view.
pub fn field_menu_root_draws(
    ctx: &PauseMenuCtx<'_>,
    root: &FieldMenuSession,
    world: &World,
    refusal: Option<&str>,
) -> PauseMenuDraws {
    let view = root.view();
    let rows: Vec<ui::FieldMenuRowView<'_>> = view
        .rows
        .iter()
        .map(|r| ui::FieldMenuRowView {
            label: r.label,
            enabled: r.enabled,
        })
        .collect();
    let snaps = field_menu_dispatch::status_snapshots(world);
    let party: Vec<ui::FieldMenuPartyView<'_>> = snaps
        .iter()
        .map(|s| ui::FieldMenuPartyView {
            name: &s.name,
            level: s.level,
            hp: s.hp,
            hp_max: s.hp_max,
            mp: s.mp,
            mp_max: s.mp_max,
            ap: s.ap as u16,
            // Record `+0x12E`: non-zero re-inks the HP number gold
            // (`FUN_800349EC`'s ailment arm).
            status: s.status_flags,
        })
        .collect();
    let party_ap: Vec<u16> = snaps.iter().map(|s| s.ap as u16).collect();
    let mut out = pause_screen_draws(
        ctx,
        PauseScreen::TopLevel(TopLevelView {
            rows: &rows,
            cursor: view.cursor,
            money: view.money,
            play_time_seconds: view.play_time_seconds,
            party: &party,
            party_ap: &party_ap,
        }),
    );
    if let Some(message) = refusal {
        out.extend(save_refusal_draws(ctx, message));
    }
    out
}

/// The **save-refusal notice**: the messagebox that says a commit did not
/// happen (`SaveScreenFlow::refuse`). The sub-screen has closed by the time it
/// draws, which is why it rides over the menu root.
pub fn save_refusal_draws(ctx: &PauseMenuCtx<'_>, message: &str) -> PauseMenuDraws {
    PauseMenuDraws {
        sprites: ctx
            .chrome
            .map(|rects| ui::save_refusal_panel_draws_for(rects, ctx.origin, ctx.scale))
            .unwrap_or_default(),
        texts: ui::save_refusal_text_draws_for(ctx.font, message, ctx.origin, ctx.scale),
    }
}

/// The draw lists for an active field-menu sub-session.
///
/// `world` is the scene behind the menu (`None` only on a host with no scene
/// up, where the screens that need the roster draw their stand-ins or
/// nothing); `equip_info` is the menu runtime's disc equipment table.
///
/// The Save sub-session draws nothing here: its screen is the **save-select**
/// surface, which a host also reaches from the boot Continue -> Load path and
/// composes through `save_select_overlay_draws` there.
pub fn field_menu_sub_draws(
    ctx: &PauseMenuCtx<'_>,
    sub: &FieldMenuSubsession,
    world: Option<&World>,
    equip_info: Option<&DiscEquipInfo>,
) -> PauseMenuDraws {
    match sub {
        FieldMenuSubsession::ListOrder(s) => list_order_draws(ctx, s),
        FieldMenuSubsession::Status(s) => status_draws(ctx, s),
        FieldMenuSubsession::Config(s) => options_draws(ctx, s),
        FieldMenuSubsession::Save(_) => PauseMenuDraws::default(),
        FieldMenuSubsession::Spells(s) => magic_draws(ctx, s, world),
        FieldMenuSubsession::Items(s) => items_draws(ctx, s, world),
        FieldMenuSubsession::Equip {
            session, char_slot, ..
        } => equip_draws(ctx, session, *char_slot, world, equip_info),
        FieldMenuSubsession::Arts(s) => match world {
            Some(w) => arts_editor_draws(ctx, s, w),
            None => PauseMenuDraws::default(),
        },
    }
}

/// The record screen's list page, opened over the Magic screen's spell rows
/// (Square).
pub fn list_order_draws(
    ctx: &PauseMenuCtx<'_>,
    s: &legaia_engine_menus::list_order::ListOrderSession,
) -> PauseMenuDraws {
    let rows: Vec<ui::ListOrderRowView<'_>> = s
        .rows()
        .iter()
        .enumerate()
        .map(|(i, r)| ui::ListOrderRowView {
            label: r.label.as_str(),
            latched: s.latched() == Some(i),
        })
        .collect();
    pause_screen_draws(
        ctx,
        PauseScreen::Generic(GenericContent::ListOrder(ui::ListOrderDrawArgs {
            title: LIST_ORDER_TITLE,
            rows: &rows,
            cursor: s.cursor(),
            scroll_top: s.scroll_top(),
            page_rows: legaia_engine_menus::list_order::LIST_ORDER_PAGE_ROWS,
            reorderable: s.reorderable(),
        })),
    )
}

/// Status sub-screen: the main panel + the three satellite windows + the
/// Status tab, with the LV/HP/MP + AP-gauge + element icon sprites.
pub fn status_draws(
    ctx: &PauseMenuCtx<'_>,
    s: &legaia_engine_menus::status_screen::StatusScreenSession,
) -> PauseMenuDraws {
    let Some(snap) = s.current() else {
        return PauseMenuDraws::default();
    };
    let stat_rows: Vec<ui::StatusStatRow<'_>> = snap
        .stats
        .iter()
        .zip(snap.stat_labels.iter())
        .map(|((live, growth), l)| ui::StatusStatRow {
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
    let panel = ui::StatusPanelView {
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
        status: snap.status_flags,
    };
    let names: Vec<&str> = s.snapshots().iter().map(|m| m.name.as_str()).collect();
    let satellite = ui::StatusSatelliteView {
        party_names: &names,
        cursor: s.cursor() as usize,
        name: &snap.name,
        level: snap.level,
    };
    pause_screen_draws(
        ctx,
        PauseScreen::Status(StatusScreenView {
            panel: &panel,
            satellite: &satellite,
            ap: snap.ap as u16,
            // ATR icon by roster character id (slot) of the highlighted
            // member; the icon set is Vahn / Noa / Gala in character order.
            atr_char: snap.slot as usize,
        }),
    )
}

/// The options value popup's per-open content rect: x / w off window 0x0B's
/// descriptor, y / h stamped from the hovered row of the main window.
pub fn options_popup_rect(
    ctx: &PauseMenuCtx<'_>,
    popup: &legaia_engine_menus::options::OptionsPopup,
) -> (i32, i32, i32, i32) {
    use legaia_game_tables::menu_windows::window_ids;
    let (px, _, pw, _) = ctx.rects.rect(window_ids::OPTIONS_POPUP);
    let (_, sy, _, _) = ctx.rects.rect(window_ids::OPTIONS_MAIN);
    legaia_engine_menus::options::options_popup_content_rect(
        sy,
        px,
        pw,
        popup.row,
        popup.choices.len(),
    )
}

/// Options sub-screen: the settings rows + value popup + the hand cursor, with
/// the options window frame + tab. The screen's model is the engine's
/// (`OptionsSession::screen_model`); this only borrows it into the renderer's
/// views and places the popup.
pub fn options_draws(
    ctx: &PauseMenuCtx<'_>,
    s: &legaia_engine_menus::options::OptionsSession,
) -> PauseMenuDraws {
    let m = s.screen_model();
    let row_views: Vec<ui::OptionsRowView<'_>> = m
        .rows
        .iter()
        .map(|r| ui::OptionsRowView {
            label: r.label,
            value: r.value,
            teal: r.teal,
            advance: r.advance,
        })
        .collect();
    let popup = m.popup.map(|p| ui::OptionsPopupDraw {
        rect: options_popup_rect(ctx, &p),
        choices: p.choices,
        cursor: p.cursor,
    });
    let rebind_pairs: Vec<(&str, &str)> = m
        .rebind
        .iter()
        .flat_map(|k| k.rows.iter())
        .map(|(b, k)| (b.as_str(), k.as_str()))
        .collect();
    let rebind = m.rebind.as_ref().map(|k| KeyRebindView {
        rows: &rebind_pairs,
        cursor: k.cursor,
        awaiting: k.awaiting,
    });
    pause_screen_draws(
        ctx,
        PauseScreen::Options(OptionsScreenView {
            rows: &row_views,
            cursor: m.cursor,
            popup,
            row_y_off: m.row_y_off,
            rebind,
        }),
    )
}

/// The retail **Magic** screen: caster window (id 19), spell-list page (id
/// 18), spell info window (id 20) and the "Magic" title tab (id 1), fed from
/// the engine-core view model. During target-select the generic overlay
/// stands in (the retail target-pick window layout is unpinned).
pub fn magic_draws(
    ctx: &PauseMenuCtx<'_>,
    s: &SpellMenuSession,
    world: Option<&World>,
) -> PauseMenuDraws {
    let model = ps::magic_screen_model(s, world.and_then(|w| w.menu.text.as_ref()));
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
            // LV / MP tags + hand cursor come from the UI-icon atlas when it
            // is resident.
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
        SpellMenuPhase::Done(_) => return PauseMenuDraws::default(),
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
    pause_screen_draws(ctx, PauseScreen::Generic(GenericContent::SpellMenu(args)))
}

/// Map the engine-core window-14 target-panel model onto the engine-ui rows.
pub fn target_panel_members(model: &ps::TargetPanelModel) -> Vec<ui::TargetPanelMember<'_>> {
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

/// Hand-cursor decode of the target panel, shared by its text + sprite
/// builders: all-party picks put the hand on every row (retail cursor bit
/// `0x2000`), otherwise it sits on one row.
pub fn target_panel_cursor(model: &ps::TargetPanelModel) -> ui::TargetPanelCursor {
    if model.all_targets {
        ui::TargetPanelCursor::All { pressed: false }
    } else {
        ui::TargetPanelCursor::Single {
            row: model.cursor_row,
            pressed: false,
        }
    }
}

/// The retail **Items** screen: command window (id 13), item-list page (id
/// 15), item info window (id 17, plus its extra widget box) and the "Items"
/// title tab (id 0). During target-select retail swaps the item list for
/// window 14, the party target panel (`FUN_801D0520`); the generic inventory
/// overlay only stands in when that panel has no roster to draw.
pub fn items_draws(
    ctx: &PauseMenuCtx<'_>,
    s: &ps::PauseItemsSession,
    world: Option<&World>,
) -> PauseMenuDraws {
    let model = ps::items_screen_model(s);
    if model.target_select {
        if let Some(panel) = world
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
                // The LV / HP / MP tags come from the UI atlas whenever the
                // sprite pass runs; without it the ASCII stand-ins carry the
                // layout.
                label_icons: ctx.chrome_present(),
                text_cursor: !ctx.chrome_present(),
            };
            return pause_screen_draws(ctx, PauseScreen::ItemsTarget(&view));
        }
        return pause_screen_draws(
            ctx,
            PauseScreen::Generic(GenericContent::Prebuilt(
                crate::inventory_use_session_draws(ctx.font, &s.inner),
            )),
        );
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
        text_cursor: !ctx.chrome_present(),
    };
    let throw = model
        .throw_confirm
        .as_ref()
        .map(|c| ui::PauseThrowConfirmView {
            name: &c.name,
            count: c.count,
            cursor: c.cursor,
            text_cursor: !ctx.chrome_present(),
        });
    // Retail's own Use-route prompt strings live in the menu overlay's data
    // segment (the `0x801CEA94` block the renderer's `lui`/`addiu` pairs point
    // at) and are not recovered, so the port stages the item name and its own
    // question in the retail line slots; the geometry - which is what the
    // renderer actually is - is exact.
    let special_one_line = model
        .special_confirm
        .as_ref()
        .map(|sc| format!("Use {}?", sc.item_name));
    let special_lines: Vec<&str> = match (model.special_confirm.as_ref(), &special_one_line) {
        (Some(sc), Some(one)) => {
            if matches!(sc.route, ps::UseRoute::Incense) {
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
            // The shared info panel's Point Card arm: retail branches on the
            // staged id being `0xFE` and prints the live bank instead of the
            // passive lines (`FUN_801D0F1C` at `0x801d0fd0`).
            point_card: model
                .info
                .as_ref()
                .filter(|i| i.is_point_card)
                .map(|_| world.map_or(0, |w| w.minigames.point_card.max(0) as u32)),
            throw_confirm: throw.as_ref(),
            special_confirm: model.special_confirm.as_ref().map(|sc| SpecialConfirmView {
                lines: &special_lines,
                cursor: sc.cursor,
            }),
        }),
    )
}

/// The equip screen in the retail multi-window layout: party window (id 21),
/// item-list window (id 23), main window (id 22) and the "Equip" tab (id 2),
/// plus the slot pictogram column and hand cursors. The projection - slot
/// labels, the candidate list, the stat compare against
/// `compute_battle_stats` - is the shared `engine-menus` model.
pub fn equip_draws(
    ctx: &PauseMenuCtx<'_>,
    session: &legaia_engine_menus::equip_session::EquipSession,
    char_slot: u8,
    world: Option<&World>,
    equip_info: Option<&DiscEquipInfo>,
) -> PauseMenuDraws {
    let names = world
        .map(field_menu_dispatch::roster_names)
        .unwrap_or_default();
    // Item ids resolve through the world's own disc text tables, the same
    // resolver the Items screen's rows use - without it the screen spells
    // every candidate as a raw id.
    let text = |id: u8| {
        world
            .map(|w| field_menu_dispatch::item_display_text(w, id))
            .unwrap_or_default()
    };
    // Window 25's panel needs what the session does not carry: the record
    // (HP / MP maxima) and the two disc tables the compare category is looked
    // up in.
    let record = world.and_then(|w| w.party.roster.members.get(char_slot as usize));
    let compare = record.map(|r| ps::EquipCompareCtx {
        record: r,
        equip_info,
        item_effects: world.and_then(|w| w.tables.item_effects.as_ref()),
    });
    let model = ps::equip_screen_model(
        session,
        char_slot,
        &names,
        &world.map(|w| w.present_party_list()).unwrap_or_default(),
        Some(&text),
        compare,
    );
    equip_screen_compose(ctx, &equip_compose_input(&model, !ctx.chrome_present()))
}

/// Borrow the shared Equip screen model into the shared `engine-ui` compose
/// input. The phase tag has to cross from `engine-menus`' enum to
/// `engine-ui`'s, and `engine-ui` deliberately does not depend on the engine.
pub fn equip_compose_input(m: &ps::EquipScreenModel, text_cursor: bool) -> EquipComposeInput<'_> {
    use ps::EquipScreenPhase as Tag;
    EquipComposeInput {
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
        info: m.info.as_ref().map(|i| ui::pause_menu::EquipInfoInput {
            name: i.name.as_str(),
            count: i.count,
            desc: i.desc.as_str(),
            passive: i.passive.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
        }),
        compare: m.compare.as_ref().map(equip_compare_input),
    }
}

/// Borrow the shared Equip **compare** model into `engine-ui`'s input.
pub fn equip_compare_input(c: &ps::EquipCompareModel) -> ui::pause_menu::EquipCompareInput<'_> {
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

/// The Tactical Arts chain editor, inside the generic sub-window frame. The
/// projection out of the live `ChainEditor` (character name, the
/// pretty-printed sequences, the phase tag, the "+ New" room check) is
/// `field_menu_dispatch::arts_editor_view`.
pub fn arts_editor_draws(
    ctx: &PauseMenuCtx<'_>,
    editor: &legaia_engine_battle::tactical_arts_editor::ChainEditor,
    world: &World,
) -> PauseMenuDraws {
    let view = field_menu_dispatch::arts_editor_view(editor, world);
    let saved_rows: Vec<ui::ArtsChainRow<'_>> = view
        .saved
        .iter()
        .map(|(name, pretty)| ui::ArtsChainRow {
            name,
            pretty_sequence: pretty,
        })
        .collect();
    let args = ui::ArtsEditorDrawArgs {
        character_name: &view.character_name,
        phase: match view.phase {
            ArtsEditorPhaseTag::Browsing => ui::ArtsEditorPhase::Browsing,
            ArtsEditorPhaseTag::Editing => ui::ArtsEditorPhase::Editing,
            ArtsEditorPhaseTag::Naming => ui::ArtsEditorPhase::Naming,
        },
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
