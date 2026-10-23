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
        legaia_engine_screens::field_screens::field_window_stage_draws(
            &self.session.host,
            &self.font,
        )
    }
}

impl PlayWindowApp {
    /// Project the live name-entry session into the renderer-agnostic view
    /// the engine-ui builders consume (grid vs control cursor split via the
    /// session's own control mapping).
    pub(super) fn name_entry_view<'a>(
        &'a self,
        entry: &'a legaia_engine_menus::name_entry::NameEntry,
    ) -> legaia_engine_ui::NameEntryView<'a> {
        let world = &self.session.host.world;
        legaia_engine_screens::field_screens::name_entry_view(
            entry,
            world.frame,
            world.tables.name_entry_prompts.as_ref(),
        )
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
