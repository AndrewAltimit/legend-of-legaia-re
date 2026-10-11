//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;
#[path = "hud/battle_chrome.rs"]
mod battle_chrome;

#[path = "hud/battle_input_chrome.rs"]
mod battle_input_chrome;
#[path = "hud/dialog.rs"]
mod dialog;
#[path = "hud/field_party.rs"]
mod field_party;
#[path = "hud/text_list.rs"]
mod text_list;

/// One on-screen sparring-tutorial box with its stage rect `(x, y, w, h)`.
pub(super) type TutorialStageBox<'a> = (
    (i32, i32, i32, i32),
    &'a legaia_engine_core::battle_flow::ActiveTutorialBox,
);

/// Project the simulation's arts-input phase onto the presentation
/// crate's. The two enums are deliberately separate types -
/// `legaia-engine-ui` is a leaf that does not link `engine-core` - so
/// every host that draws the input screen carries this three-line map.
fn arts_input_screen(
    p: legaia_engine_battle::arts_command_input::ArtsInputScreen,
) -> legaia_engine_ui::arts_input::ArtsInputScreen {
    use legaia_engine_battle::arts_command_input::ArtsInputScreen as Sim;
    use legaia_engine_ui::arts_input::ArtsInputScreen as Ui;
    match p {
        Sim::Entering => Ui::Entering,
        Sim::Review => Ui::Review,
        Sim::Targeting => Ui::Targeting,
    }
}

/// The chip cluster a host projects for one frame: the owned `(label,
/// enabled)` chips in seat order, the cursor index, and the cluster's phase
/// (`engine-menus::battle_input::BattleCommandChips`, with the phase mapped
/// onto the leaf crate's own enum).
pub(super) type CommandChips = (
    Vec<(String, bool)>,
    usize,
    legaia_engine_ui::battle_command_ui::ChipPhase,
);

impl PlayWindowApp {
    /// The field overlay's own stage windows that ride the shop-family
    /// overlay group: the end-of-game records screen, the floor window and
    /// the code lock. Stage pixels; `build_hud` scales them with the shared
    /// screens' texts.
    pub(super) fn field_window_stage_draws(&self) -> Vec<TextDraw> {
        let mut stage: Vec<TextDraw> = Vec::new();
        // The end-of-game soft reset (op-0x49 sub-op 0xC, handler slot 0x33,
        // `FUN_801EDF00`): the records screen sliding in at the engine's pen.
        if let Some(pen) = self.session.host.world.soft_reset_records_pen() {
            stage.extend(self.records_draws_at(pen));
        }
        // The field floor window (op-0x49 sub-op 4, handler slot 0x23 - the Uru Mais
        // warp pads): laid out by the engine off the live picker state, the
        // legend read off the field overlay, drawn through the shared line
        // composition the browser play page uses.
        let mut floor = self.session.host.flag_window_lines();
        // The code lock (op-0x49 sub-op 2, handler slot 0x21 - doman's
        // password door): header off the field overlay, one letter per
        // entered symbol, through the same line composition.
        floor.extend(self.session.host.code_lock_lines());
        stage.extend(legaia_engine_ui::text_lines::text_line_draws_for(
            &self.font,
            floor
                .iter()
                .map(|l| (&l.text[..], i32::from(l.x), i32::from(l.y), l.marked)),
            [1.0, 1.0, 1.0, 1.0],
            legaia_engine_ui::text_lines::FLOOR_WINDOW_MARKED_INK,
        ));
        stage
    }

    /// Snapshot the live dialog source (simplified panel, cutscene
    /// timeline, or inline field-VM runner) into plain strings the
    /// text and chrome layers both consume. `None` when no box is
    /// open this frame.
    /// The tile board's quit-prompt geometry while its walk SM sits in the
    /// prompt state, `None` otherwise.
    fn tile_board_prompt_layout(&self) -> Option<legaia_engine_ui::TileBoardPromptLayout> {
        self.session.host.world.tile_board_prompt_cursor()?;
        let (frame, rows, cursor_x) = legaia_engine_minigames::tile_board::prompt_layout();
        Some(legaia_engine_ui::TileBoardPromptLayout {
            frame,
            rows,
            cursor_x,
        })
    }
}

impl PlayWindowApp {
    /// Project the live name-entry session into the renderer-agnostic view
    /// the engine-ui builders consume (grid vs control cursor split via the
    /// session's own control mapping).
    pub(super) fn name_entry_view<'a>(
        &self,
        entry: &'a legaia_engine_menus::name_entry::NameEntry,
    ) -> legaia_engine_ui::NameEntryView<'a> {
        use legaia_engine_menus::name_entry::GRID;
        let (grid_cursor, control_cursor) = entry.cursor_cells();
        legaia_engine_ui::NameEntryView {
            grid_rows: &GRID,
            name: &entry.name,
            default_name: &entry.default_name,
            grid_cursor,
            control_cursor,
            confirming: entry.state == legaia_engine_menus::name_entry::NameEntryState::Confirm,
            confirm_yes: entry.confirm_yes,
            caret_on: legaia_engine_menus::name_entry::caret_on(self.session.host.world.frame),
        }
    }

    /// Build the name-entry window chrome + hand cursor sprites (the two
    /// filigree 9-slice windows at the retail-traced footprints). Sampled
    /// from the resident system-UI atlas; composited in the same sprite
    /// slot as the dialog chrome, under the text layer.
    pub(super) fn name_entry_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_ui::SpriteDraw> {
        let Some(assets) = self.save_menu.as_ref() else {
            return Vec::new();
        };
        let Some(entry) = self.session.host.world.party.name_entry.as_ref() else {
            return Vec::new();
        };
        let view = self.name_entry_view(entry);
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        legaia_engine_ui::name_entry_chrome_sprite_draws_for(
            &assets.rects,
            &view,
            stage_origin,
            stage_scale,
        )
    }
}

/// Top-left anchor of the battle HUD's slot-row block, in surface pixels.
///
/// Numerically equal to `legaia_engine_screens::LEVEL_UP_PEN` and deliberately a separate
/// constant: nothing ties the battle HUD's anchor to the post-battle banner's,
/// and collapsing them would invent a coupling neither host has.
pub(super) const BATTLE_HUD_PEN: (i32, i32) = (8, 60);

/// Borrow an engine-core battle-item-window model as the shared builder's
/// frame, handing it to `f` (the frame borrows row views built here, so
/// this is CPS-shaped).
pub(super) fn with_battle_item_frame<R>(
    model: &legaia_engine_menus::inventory_use::BattleItemMenuModel,
    f: impl FnOnce(&legaia_engine_ui::battle_item_ui::BattleItemMenuFrame<'_>) -> R,
) -> R {
    use legaia_engine_ui::battle_item_ui as bii;
    let rows: Vec<bii::BattleItemRowView<'_>> = model
        .view
        .rows
        .iter()
        .map(|r| bii::BattleItemRowView {
            name: &r.name,
            count: r.count,
            admissible: r.admissible,
        })
        .collect();
    let target_rows: Vec<bii::BattleItemTargetView<'_>> = model
        .targets
        .as_ref()
        .map(|(rows, _)| {
            rows.iter()
                .map(|t| bii::BattleItemTargetView {
                    name: &t.name,
                    hp: t.hp,
                    hp_max: t.hp_max,
                    mp: t.mp,
                    mp_max: t.mp_max,
                    alive: t.alive,
                })
                .collect()
        })
        .unwrap_or_default();
    let frame = bii::BattleItemMenuFrame {
        rows: &rows,
        cursor: model.view.cursor_row,
        description: model.description.as_deref(),
        actor_name: &model.actor_name,
        targets: model
            .targets
            .as_ref()
            .map(|(_, cursor)| (target_rows.as_slice(), *cursor)),
    };
    f(&frame)
}

/// Project the HUD model's slot array into the shared builder's view type.
///
/// Every slot is emitted, **including inactive ones** (as empty-name rows the
/// builder skips). That is deliberate: `battle_hud_draws_for` derives both a
/// row's Y and a popup's anchor from the slice index, so the index has to stay
/// the absolute actor-table slot. Compacting to active slots only would shift
/// every monster row up and anchor damage numbers to the wrong actor.
pub(super) fn battle_hud_slot_views(
    hud: &legaia_engine_battle::battle_hud_state::BattleHud,
) -> Vec<HudSlotView<'_>> {
    hud.slots
        .iter()
        .map(|s| {
            let (hp_fill, mp_fill) = s.gauge_fill_indices();
            let meta = HudSlotMeta {
                is_party: s.is_party,
                alive: s.alive,
                hp: s.hp,
                hp_max: s.hp_max,
                mp: s.mp,
                mp_max: s.mp_max,
                ap_filled: s.ap_filled,
                ap_max: s.ap_max,
                hp_fill,
                mp_fill,
                // The single retail-selected status element
                // (`FUN_8002C2E4`'s ladder over the packed `+0x16E` word)
                // plus the level its no-ailment arm draws.
                status_sprite: s.status_sprite(),
                level: s.level,
            };
            let name = if s.active { s.name.as_str() } else { "" };
            HudSlotView::from_plain(meta, name)
        })
        .collect()
}

/// Project the HUD model's popup queue into the shared builder's view type.
pub(super) fn battle_hud_popup_views(
    hud: &legaia_engine_battle::battle_hud_state::BattleHud,
) -> Vec<HudPopupView> {
    hud.popup_views()
        .into_iter()
        .map(|p| HudPopupView {
            slot: p.slot,
            amount: p.amount,
            is_heal: p.is_heal,
            is_crit: p.is_crit,
            status_letter: p.status_letter,
            alpha: p.alpha,
        })
        .collect()
}

#[cfg(test)]
#[path = "hud/battle_hud_wiring_tests.rs"]
mod battle_hud_wiring_tests;
