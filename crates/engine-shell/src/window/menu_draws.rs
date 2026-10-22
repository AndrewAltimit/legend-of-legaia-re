//! The native `play-window`'s half of the pause menu: the composition
//! context and the four entry points the window's text and sprite passes
//! call.
//!
//! The composition itself - which windows a screen frames, which painter
//! draws its title tab, where the modals sit in the sprite order and the
//! final stage scale - lives in
//! [`legaia_engine_ui::pause_menu`](legaia_engine_ui::pause_menu), and the
//! projection of the live `engine-core` sessions into its view structs in
//! [`legaia_engine_screens::pause_screens`]; the browser play page runs the
//! same two. What stays here is borrowing this window's holders into them.

use super::*;
use legaia_engine_screens::pause_screens;
use legaia_engine_ui::pause_menu::{
    MenuRects, PauseMenuCtx, PauseMenuDraws, art_learned_notice_draws, spell_level_notice_draws,
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
    /// entry context's kind byte is `0x0D`
    /// (`pause_screens::context_locked_screen_draws`).
    pub(super) fn context_locked_screen_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> PauseMenuDraws {
        let Some(menu) = self.session.field_menu.as_ref() else {
            return PauseMenuDraws::default();
        };
        pause_screens::context_locked_screen_draws(
            &self.menu_ctx(surface_w, surface_h),
            menu,
            &self.session.host.world,
        )
    }

    /// Window 7 - the spell level-up notice, drawn while
    /// [`legaia_engine_menu_runtime::menu_runtime::MenuRuntime`] holds the beat a
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

    /// Window 8 - the art-learned notice, drawn while the menu runtime holds
    /// the beat a Hyper-Art book use armed (`apply_pause_items_outcome` ->
    /// `MenuState::pending_art_notice` -> `arm_art_learned_notice`). The Items
    /// use sub-screen opens it with script `0x801E4C60` (`[open window 8]`)
    /// and stalls for a press - the input side lives in the field-menu arm
    /// of `tick_boot_ui`. Twin of the browser page's arm in `play_menu`.
    pub(super) fn art_learned_notice_window_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<TextDraw> {
        let Some(notice) = self.menu_runtime.art_learned_notice() else {
            return Vec::new();
        };
        art_learned_notice_draws(&self.menu_ctx(surface_w, surface_h), &notice.lines)
    }

    /// Top-level pause menu: the command list, the money / play-time box, the
    /// party overview panel and the save-refusal notice
    /// (`pause_screens::field_menu_root_draws`).
    ///
    /// The menu session lives on the `NativeSession` (the headless host of the
    /// CARD/menu mode); the window only renders it.
    pub(super) fn field_menu_root_draws(&self, surface_w: u32, surface_h: u32) -> PauseMenuDraws {
        let Some(menu) = self.session.field_menu.as_ref() else {
            return PauseMenuDraws::default();
        };
        pause_screens::field_menu_root_draws(
            &self.menu_ctx(surface_w, surface_h),
            menu,
            &self.session.host.world,
            self.save_flow.refusal().map(|r| r.message()),
        )
    }

    /// Build the draw lists for an active field-menu sub-session
    /// (`pause_screens::field_menu_sub_draws`). The Save sub-session is the
    /// exception - it renders through the save-select chrome stage
    /// (`save_select_chrome_sprite_draws` + `boot_ui_draws`) it shares with
    /// the boot Continue -> Load screen, and so is handled by its caller.
    pub(super) fn field_menu_sub_draws(
        &self,
        sub: &legaia_engine_menu_runtime::field_menu_dispatch::FieldMenuSubsession,
        surface_w: u32,
        surface_h: u32,
    ) -> PauseMenuDraws {
        pause_screens::field_menu_sub_draws(
            &self.menu_ctx(surface_w, surface_h),
            sub,
            Some(&self.session.host.world),
            self.menu_runtime.equip_info.as_ref(),
        )
    }

    /// Text half of the field menu's Load / Save sub-screen.
    ///
    /// Not part of [`Self::field_menu_sub_draws`]'s shared composition: this
    /// screen is the **save-select** surface, and the native window reaches
    /// the same one from the boot Continue -> Load path
    /// (`BootUiState::Continue`). Both go through `save_select_overlay`
    /// (the shared overlay model + composition) so the in-game and boot
    /// entries cannot drift from each other; hoisting only the pause half
    /// would have forked them. Pre-scaled to surface coords, so the caller
    /// must not scale it again.
    pub(super) fn field_save_sub_draws(
        &self,
        s: &legaia_engine_menus::save_select::SaveSelectSession,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<TextDraw> {
        self.save_select_overlay(&self.save_flow, s, surface_w, surface_h)
            .texts
    }
}
