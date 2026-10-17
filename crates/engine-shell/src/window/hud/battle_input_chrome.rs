//! Chrome sprites for the battle's input screens: the arts-input screen and
//! the sparring tutorial's boxes. The compositions are
//! `legaia_engine_screens::battle_screens`, shared with the browser play page.

use super::*;

impl PlayWindowApp {
    /// The **arts command-input** chrome sprites - the four direction chips +
    /// D-pad glyph, the input bar with its committed pennants, and the AP
    /// plate - while a party member owns the pad in the retail-model entry
    /// session (`battle_screens::arts_input_chrome_draws`). Empty otherwise,
    /// and before the system-UI atlas is resident.
    pub(in crate::window) fn arts_input_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_ui::SpriteDraw> {
        let Some(assets) = self.save_menu.as_ref() else {
            return Vec::new();
        };
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        legaia_engine_screens::battle_screens::arts_input_chrome_draws(
            &self.session.host.world,
            Some(&assets.rects),
            stage_origin,
            stage_scale,
        )
    }
}

impl PlayWindowApp {
    /// Sparring-tutorial prompt-box chrome and Koru's timed-fight strip
    /// (`battle_screens::battle_tutorial_chrome_draws`), composited in the
    /// shared chrome sprite slot, under the text layer.
    pub(in crate::window) fn battle_tutorial_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_ui::SpriteDraw> {
        let Some(assets) = self.save_menu.as_ref() else {
            return Vec::new();
        };
        if self.boot_ui.is_active() {
            return Vec::new();
        }
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        legaia_engine_screens::battle_screens::battle_tutorial_chrome_draws(
            &self.session.host.world,
            &self.font,
            &assets.rects,
            stage_origin,
            stage_scale,
        )
    }
}
