//! Browser host surface for the retail developer menu - the play-page twin
//! of the native window's `window/dev_menu.rs`.
//!
//! Retail opens its dev tools from debug branches in the world-map and field
//! controllers - branches a retail player cannot reach. The native window's
//! equivalent is the `LEGAIA_DEV_MENU` environment variable: an explicit
//! opt-in taken by the person who launches the binary. This host mirrors
//! that opt-in in the only form a browser tab has: an explicit page control
//! ([`LegaiaRuntime::play_dev_menu_set_enabled`]) that the visitor - who
//! *is* the person running the program in a client-side page - must flip
//! each session. The page deliberately neither persists the flag nor reads
//! it from the URL, so a shared link cannot carry it and nothing but a
//! fresh, deliberate click enables it. Everything the menu edits is the
//! visitor's own single-player world, in their own tab.
//!
//! With the flag off (the default), nothing here runs and no draw is
//! produced - the shipped page is unchanged.
//!
//! The model is the same one the native window drives: `engine-core`'s
//! [`DevMenuSession`] ticked off the retail **packed** pad words
//! (`retail_packed` over the world's own pad pump - see that function in
//! `legaia_engine_core::dev_menu` for the two layouts), the row list drawn
//! through the ported list-body renderer (`dev_menu_list_draws_for`,
//! `FUN_801EAD98`'s geometry), and Square swapping the list for the
//! battle-records page (`records_screen_draws_for`, `FUN_801ED710`) while
//! the list has the pad. The equip row's confirm commits against the
//! engine's own bag through the same `WorldEquipHost`.
//!
//! `check-ui-host-drift.py` pins this host to the native one three ways: the
//! two pens and the records heading strings are `CONSTANT_PAIRS` rows, and
//! the tick + records-model injection sites are `SIM_PAIRS` rows, so neither
//! host can drift on the packed-pad conversion, the equip commit or the
//! record-counter reads without the gate failing.

use legaia_engine_menu_runtime::dev_menu_host::DevMenuWorldExt;
use legaia_engine_menus::dev_menu_host::{DevMenuSession, DevPage};
use legaia_engine_screens::dev_menu::{
    DEV_RECORDS_PEN, RECORDS_TOGGLE, dev_menu_list_draws, records_draws_for_world,
};
use legaia_engine_system::pad::retail_packed;
use legaia_engine_ui::{self as ui, TextDraw};
use wasm_bindgen::prelude::*;

use crate::runtime::LegaiaRuntime;

impl LegaiaRuntime {
    /// Advance the developer menu one frame off the world's own pad pump.
    ///
    /// Called from `tick_frame` so the session sees the same 60 Hz pad
    /// transitions the simulation does - no page-side key handling exists
    /// (host-drift tier 5): the words come out of `World::set_pad`, converted
    /// to the retail packed layout the dev kernels key on.
    pub(crate) fn tick_dev_menu(&mut self) {
        if !self.dev_menu_enabled {
            return;
        }
        let Some((scene, camera)) = self.scene_host.host_cam_mut() else {
            return;
        };
        // The EVENT FLAG page's list comes off the user's disc (the field
        // overlay's data segment), as on the native window.
        let session = self
            .dev_menu
            .get_or_insert_with(|| DevMenuSession::with_flag_tags(scene.dev_flag_list_tags()));
        let world = &mut scene.world;
        let (edge, held) = {
            let (now, prev) = (
                retail_packed(world.input.pad()),
                retail_packed(world.input.pad_prev()),
            );
            (now & !prev, now)
        };

        // The shared tick the native window makes: party records, the
        // CAMERA row's follow switch on this page's camera, and the EQUIP
        // row's confirm against the engine's own bag.
        let _ = session.tick_host(world, camera, edge, held);

        // The screen's cues ride the world's SFX ring (retail
        // `FUN_80035B50`), which `route_field_sfx` replays - the same call
        // the native window makes.
        session.route_sfx(world);

        // Square swaps the row list for the Records page while the list has
        // the pad; the sub-editors keep their own key map.
        if session.page == DevPage::List && edge & RECORDS_TOGGLE != 0 {
            self.dev_menu_records = !self.dev_menu_records;
        }
    }

    /// Build the records page for the live world - the browser twin of the
    /// native `build_dev_records_draws`.
    fn dev_records_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        self.records_draws_at(font, DEV_RECORDS_PEN)
    }

    /// The records screen (`FUN_801ED710`) at `pen`, retail stage pixels -
    /// the native `records_draws_at`'s builder (`records_draws_for_world`), shared by the dev
    /// Records page and the end-of-game soft reset
    /// (`World::soft_reset_records_pen`).
    pub(crate) fn records_draws_at(
        &self,
        font: &legaia_font::Font,
        pen: (i32, i32),
    ) -> Vec<TextDraw> {
        let Some(scene) = self.scene_host.host() else {
            return Vec::new();
        };
        records_draws_for_world(&scene.world, font, pen)
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// The visitor's explicit dev-menu opt-in - the browser twin of the
    /// native window's `LEGAIA_DEV_MENU` environment variable. Session-only
    /// by design: the page neither persists this nor reads it from the URL,
    /// so only a deliberate click by the person at the keyboard enables it.
    /// Turning it off drops the session and its staged edits.
    pub fn play_dev_menu_set_enabled(&mut self, on: bool) {
        self.dev_menu_enabled = on;
        if !on {
            self.dev_menu = None;
            self.dev_menu_records = false;
        }
    }

    /// Whether the dev menu is currently enabled for this session.
    pub fn play_dev_menu_enabled(&self) -> bool {
        self.dev_menu_enabled
    }

    /// Draw list for the developer-menu overlay: `{ "open", "texts" }` in
    /// surface pixels, font-atlas quads only (the dev overlay is text-only on
    /// the native window too). `open` is `false` while the opt-in is off,
    /// before a scene is staged, or while the pause menu owns the screen -
    /// the dev overlay rides the live field, exactly as the native window
    /// folds it into the field HUD's draw list.
    pub fn play_dev_menu_draws_json(&mut self, surface_w: u32, surface_h: u32) -> String {
        const CLOSED: &str = r#"{"open":false,"texts":[]}"#;
        if !self.dev_menu_enabled || self.play_menu_is_open() {
            return CLOSED.to_string();
        }
        if !self.ensure_menu_assets() {
            return CLOSED.to_string();
        }
        let Some(session) = self.dev_menu.as_ref() else {
            return CLOSED.to_string();
        };
        let Some(assets) = self.menu_assets.as_ref() else {
            return CLOSED.to_string();
        };
        let font = assets.font_ref();
        let (origin, scale) = crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
        let mut texts = if session.page == DevPage::List && self.dev_menu_records {
            self.dev_records_draws(font)
        } else {
            dev_menu_list_draws(session, font)
        };
        if texts.is_empty() {
            return CLOSED.to_string();
        }
        ui::scale_stage_text_draws(&mut texts, origin, scale);
        serde_json::json!({
            "open": true,
            "texts": texts.iter().map(crate::play_menu::quad_json).collect::<Vec<_>>(),
        })
        .to_string()
    }
}
