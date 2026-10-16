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

    /// One battle-HUD frame from the shared builder: the party strip, the
    /// top-left plaque and the popups
    /// (`legaia_engine_screens::battle_screens::battle_hud_frame_draws`).
    ///
    /// Both halves come from one call so the two host draw slots cannot
    /// drift: the text half goes into the glyph layer, the sprite half into
    /// the system-UI atlas layer through `battle_chrome_sprite_draws`.
    pub(in crate::window) fn battle_hud_frame_draws(
        &self,
        w: u32,
        h: u32,
    ) -> legaia_engine_ui::BattleHudDraws {
        legaia_engine_screens::battle_screens::battle_hud_frame_draws(
            &self.battle_screen_inputs(),
            w,
            h,
        )
    }

    /// The engine-core battle-item-window projection (shared with the
    /// browser play page - `World::battle_item_menu_model` owns the gating
    /// and text resolution; this window only borrows it into the builder's
    /// frame via [`legaia_engine_screens::with_battle_item_frame`]).
    pub(in crate::window) fn battle_item_menu_model(
        &self,
    ) -> Option<legaia_engine_menus::inventory_use::BattleItemMenuModel> {
        self.session.host.world.battle_item_menu_model()
    }

    /// Whether a Muscle Dome leg's battle chrome is on screen (the shared
    /// gate, `battle_screens::dome_battle_chrome_up`).
    pub(in crate::window) fn dome_battle_chrome_up(&self) -> bool {
        legaia_engine_screens::battle_screens::dome_battle_chrome_up(&self.session.host.world)
    }

    /// The command menu's chips this frame (the shared projection,
    /// `legaia_engine_screens::battle_command`).
    pub(in crate::window) fn battle_command_menu_chips(&self) -> Option<CommandChips> {
        legaia_engine_screens::battle_command::battle_command_menu_chips(&self.session.host.world)
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
