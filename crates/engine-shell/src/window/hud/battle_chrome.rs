//! The battle HUD chrome the play window draws: the frame and badge rects,
//! the banner message, the item / command menu chips, the Muscle Dome chrome
//! gate and the enemy target strip.
//!
//! The compositions are `legaia_engine_screens::battle_screens`, shared with
//! the browser play page; this file only assembles their inputs from the
//! window's holders.

use super::*;

impl PlayWindowApp {
    /// The solid-white font-atlas texel the battle HUD's filled rects sample
    /// (`font_solid_src`). Scanned once per process - the window's font never
    /// changes after startup.
    pub(in crate::window) fn battle_hud_solid_src(&self) -> Option<(u32, u32, u32, u32)> {
        use std::sync::OnceLock;
        static SOLID: OnceLock<Option<(u32, u32, u32, u32)>> = OnceLock::new();
        *SOLID.get_or_init(|| legaia_engine_ui::font_solid_src(&self.font))
    }

    /// The shared battle screens' inputs, borrowed from this window.
    pub(in crate::window) fn battle_screen_inputs(
        &self,
    ) -> legaia_engine_screens::battle_screens::BattleScreenInputs<'_> {
        legaia_engine_screens::battle_screens::BattleScreenInputs {
            world: &self.session.host.world,
            hud: &self.battle_hud,
            font: &self.font,
            chrome: self.save_menu.as_ref().map(|a| &a.rects),
            badges: self.save_menu.as_ref().map(|a| a.badges),
            solid_src: self.battle_hud_solid_src(),
        }
    }

    /// The battle HUD's chrome sprites, the item window's frames and the
    /// command chips, or outside battle the field-side message banner's frame
    /// (`battle_screens::battle_chrome_sprite_draws`). Empty before the atlas
    /// is resident and while the boot UI owns the frame.
    pub(in crate::window) fn battle_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_ui::SpriteDraw> {
        if self.save_menu.is_none() || self.boot_ui.is_active() {
            return Vec::new();
        }
        legaia_engine_screens::battle_screens::battle_chrome_sprite_draws(
            &self.battle_screen_inputs(),
            surface_w,
            surface_h,
        )
    }
}
