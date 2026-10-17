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
        legaia_engine_screens::field_screens::name_entry_view(entry, self.session.host.world.frame)
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

// The battle HUD's slot / popup projections and its pen are shared with the
// browser play page (`legaia_engine_screens::battle_screens`).
#[cfg(test)]
use legaia_engine_screens::battle_screens::{
    BATTLE_HUD_PEN, battle_hud_popup_views, battle_hud_slot_views,
};

#[cfg(test)]
#[path = "hud/battle_hud_wiring_tests.rs"]
mod battle_hud_wiring_tests;
