//! The shop / prize-exchange / inn / banner screens, through the shared
//! composition both play hosts call (`legaia-engine-screens`).
//!
//! This host only assembles the inputs from its own holders. The frame is
//! built once per redraw ([`PlayWindowApp::shop_overlay_frame`]) and read by
//! both passes: `build_hud` takes its stage texts (scaled with the rest of the
//! stage group), the chrome sprite pass takes its sprites.

use super::*;

use legaia_engine_screens::{ScreenInputs, ShopOverlayFrame};

impl PlayWindowApp {
    /// The shop-family overlay for this frame over a `surface_w x surface_h`
    /// surface: stage texts, surface sprites, banner rows.
    pub(super) fn shop_overlay_frame(&self, surface_w: u32, surface_h: u32) -> ShopOverlayFrame {
        let coin_counter = self.session.host.coin_counter_lines();
        let inputs = ScreenInputs {
            world: &self.session.host.world,
            menu: &self.menu_runtime,
            font: &self.font,
            table: self.menu_window_table.as_ref(),
            chrome: self.save_menu.as_ref().map(|m| &m.rects),
            seru_names: self.seru_names.as_ref(),
            coin_counter: &coin_counter,
        };
        legaia_engine_screens::shop_overlay_frame(&inputs, surface_w, surface_h)
    }
}
