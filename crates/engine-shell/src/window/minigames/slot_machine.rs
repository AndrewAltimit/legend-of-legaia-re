//! The casino slot machine on the play window: the payline prims, the
//! marquee tick, the cabinet upload and prims, and the rules text.

use super::*;

impl PlayWindowApp {
    /// The slot machine's five paylines as screen primitives: the ported
    /// payline pass (`FUN_801D3380`) with its projection
    /// (`SlotMachine::payline_segments`), turned into one-pixel flat quads by
    /// the shared `slot_paylines` builder. Both browser pages stroke the
    /// same projected segments. Empty outside the slot machine or when the
    /// overlay's payline table did not decode.
    pub(in crate::window) fn slot_payline_screen_prims(
        &self,
    ) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        if self.session.host.world.mode != SceneMode::SlotMachine {
            return Vec::new();
        }
        self.session
            .host
            .world
            .minigames
            .slot_machine
            .as_ref()
            .map(legaia_engine_screens::minigame_hud::slot_payline_prims)
            .unwrap_or_default()
    }

    /// Advance the slot machine's marquee one frame and compose its dot
    /// buffer (`SlotMarqueeClock::frame`), off the live machine's own
    /// marquee state. Clears the clock whenever no machine is on screen, so a
    /// fresh visit starts its legend the way retail's init leaves it.
    pub(in crate::window) fn tick_slot_marquee(&mut self) {
        let world = &self.session.host.world;
        let live = world.mode == SceneMode::SlotMachine;
        let (Some(m), Some(Some(assets)), true) = (
            world.minigames.slot_machine.as_ref(),
            self.slot_cabinet_assets.as_ref(),
            live,
        ) else {
            self.slot_marquee_clock = Default::default();
            self.slot_dots.clear();
            return;
        };
        // The bonus-anticipation latch (`DAT_801d3ca4`, `FUN_801d1af4`)
        // picks the two "reach" legends.
        self.slot_dots =
            self.slot_marquee_clock
                .frame(&m.marquee(), m.anticipation(), &assets.scene.messages);
    }

    /// Put the slot machine's VRAM on the GPU while the machine is on
    /// screen, decoding its data on first sight; drop it otherwise. The
    /// machine draws against this VRAM in place of the walked-in scene's -
    /// the overlay replaces the field, it is not a layer over it.
    pub(in crate::window) fn refresh_slot_cabinet_gpu(&mut self) {
        if self.session.host.world.mode != SceneMode::SlotMachine {
            self.slot_gpu = None;
            return;
        }
        if self.slot_cabinet_assets.is_none() {
            let index = self.session.host.index.clone();
            let read = |i: usize| index.entry_bytes(i as u32).ok().map(|b| b.to_vec());
            let loaded = legaia_engine_ui::slot_cabinet::SlotCabinetAssets::load(read)
                .map_err(|e| log::warn!("slots: {e}"))
                .ok()
                .map(|a| {
                    // The resident system-UI sheet the submenu box's border
                    // samples (the browser pages upload the same TIM).
                    use legaia_ui_formats::title_pak as tp;
                    match index.prot_dat_raw_bytes(
                        tp::OVERLAY_SYSTEM_UI_TIM_OFFSET as u64,
                        tp::OVERLAY_SYSTEM_UI_TIM_SIZE,
                    ) {
                        Ok(head) => a.with_system_ui(&head),
                        Err(_) => a,
                    }
                });
            self.slot_cabinet_assets = Some(loaded.map(std::sync::Arc::new));
        }
        if self.slot_gpu.is_some() {
            return;
        }
        let (Some(r), Some(Some(assets))) = (
            self.win.renderer.as_ref(),
            self.slot_cabinet_assets.as_ref(),
        ) else {
            return;
        };
        match r.upload_vram(&assets.vram) {
            Ok(v) => self.slot_gpu = Some(v),
            Err(e) => log::warn!("slots: vram upload failed: {e:#}"),
        }
    }

    /// The whole machine as screen primitives - cabinet, reels, furniture,
    /// dot matrix and coin HUD - through the shared
    /// `slot_cabinet::slot_cabinet_prims` builder. Empty outside the slot
    /// machine or before its VRAM is resident.
    pub(in crate::window) fn slot_cabinet_screen_prims(
        &self,
    ) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        if self.slot_gpu.is_none() || self.session.host.world.mode != SceneMode::SlotMachine {
            return Vec::new();
        }
        let (Some(m), Some(Some(assets))) = (
            self.session.host.world.minigames.slot_machine.as_ref(),
            self.slot_cabinet_assets.as_ref(),
        ) else {
            return Vec::new();
        };
        // The cabinet, the reels and the cash-out flow over them; the
        // paylines are their own layer (`slot_payline_screen_prims`), as on
        // the browser play page.
        legaia_engine_screens::minigame_hud::slot_cabinet_prims(
            m,
            assets,
            &self.slot_dots,
            self.slot_marquee_clock.blink,
        )
    }

    /// The slot machine's rules-page text (`slot_cabinet::
    /// slot_rules_text_draws_for`) in surface pixels; empty off a rules page.
    /// The browser play page draws the same list (`slot_rules_text_draws`).
    pub(in crate::window) fn slot_rules_text_draws(&self, w: u32, h: u32) -> Vec<TextDraw> {
        if self.session.host.world.mode != SceneMode::SlotMachine {
            return Vec::new();
        }
        let (Some(m), Some(Some(assets))) = (
            self.session.host.world.minigames.slot_machine.as_ref(),
            self.slot_cabinet_assets.as_ref(),
        ) else {
            return Vec::new();
        };
        let Some(rules) = assets.rules.as_ref() else {
            return Vec::new();
        };
        let mut d = legaia_engine_ui::slot_cabinet::slot_rules_text_draws_for(
            &self.font,
            rules,
            m.screen(),
            m.fade_level(),
        );
        let (stage_origin, stage_scale) = self.save_select_stage(w, h);
        legaia_engine_ui::scale_stage_text_draws(&mut d, stage_origin, stage_scale);
        d
    }
}
