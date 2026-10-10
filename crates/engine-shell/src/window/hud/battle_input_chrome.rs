//! Chrome sprites for the battle's input screens: the arts-input screen and
//! the sparring tutorial's boxes.

use super::*;

impl PlayWindowApp {
    /// Build the **arts command-input** chrome sprites - the four
    /// direction chips + D-pad glyph, the input bar with its committed
    /// pennants, and the AP plate - while a party member owns the pad in
    /// the retail-model entry session. Empty otherwise.
    ///
    /// Everything is composed by the shared
    /// [`legaia_engine_render::arts_input`] builders off the same baked
    /// system-UI atlas the menu chrome samples, so this host and the
    /// browser play page draw one geometry.
    pub(in crate::window) fn arts_input_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        use legaia_engine_render::arts_input as ai;
        let Some(assets) = self.save_menu.as_ref() else {
            return Vec::new();
        };
        if self.session.host.world.mode == legaia_engine_core::world::SceneMode::MuscleDome
            && !self.dome_battle_chrome_up()
        {
            return Vec::new();
        }
        // A Spirit turn raises the same bar + plate pair (no chips, no
        // pennants) and grows them - `World::spirit_gauge_view`; the browser
        // play page takes the same fallback.
        let world = &self.session.host.world;
        let Some(view) = world
            .arts_input_view()
            .or_else(|| world.spirit_gauge_view())
        else {
            return Vec::new();
        };
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        let frame = ai::ArtsInputFrame {
            buffer: view.pennants,
            spent: view.pennant_spent,
            chip_costs: view.costs,
            chip_icons: view.chip_icons,
            pool: view.pool,
            pool_max: view.pool_max,
            plate_value: view.plate_value,
            list_page: view.list_page,
            phase: arts_input_screen(view.phase),
        };
        let mut out = ai::arts_input_chrome_draws(
            &ai::ArtsInputAtlasRects::BAKED,
            &frame,
            stage_origin,
            stage_scale,
        );
        // The Rot stamp over each direction the caster's rotted limbs refuse,
        // on top of the chips (retail draws it right after the D-pad glyph).
        // The browser page makes the same call.
        if let Some(rot) = assets.rects.battle.and_then(|b| b.rot_stamp) {
            out.extend(ai::arts_input_rot_stamp_draws(
                rot,
                &frame,
                view.status,
                stage_origin,
                stage_scale,
            ));
        }
        // The AP plate is the status screen's own AP-gauge widget, so it
        // reuses the pieces the atlas already carries.
        out.extend(ai::arts_input_ap_plate_draws(
            &ai::ApPlateRects {
                cap: assets.rects.gauge_cap,
                trough: assets.rects.gauge_trough,
                fill: assets.rects.gauge_fill,
                box_: assets.rects.gauge_box,
                digits: assets.rects.gauge_digits,
                tip: assets.rects.gauge_tip,
                full_mark: assets.rects.gauge_100,
            },
            &frame,
            stage_origin,
            stage_scale,
        ));
        out
    }
}

impl PlayWindowApp {
    /// The live sparring-tutorial prompt's box rect in 320x240 stage pixels,
    /// or `None` when no box is up.
    ///
    /// The width is measured in this host's font (retail measures it with
    /// `FUN_80035F04`) and the engine applies the emitter's placement +
    /// sizing arithmetic. Shared by the text layer and the chrome layer so
    /// the frame and the rows cannot disagree.
    pub(in crate::window) fn battle_tutorial_stage_rect(&self) -> Option<(i32, i32, i32, i32)> {
        let tbox = self.session.host.world.battle_tutorial_box()?;
        let width = legaia_engine_render::battle_tutorial_text_width(&self.font, &tbox.text);
        let (x, y, w, h) = tbox.rect(width)?;
        Some((x as i32, y as i32, w as i32, h as i32))
    }

    /// Every tutorial box on screen with its stage rect - the front group of
    /// the world's box queue (one retail hook dispatch registers all of its
    /// boxes together, so the group draws together).
    pub(in crate::window) fn battle_tutorial_stage_boxes(&self) -> Vec<TutorialStageBox<'_>> {
        self.session
            .host
            .world
            .battle_tutorial_boxes_on_screen()
            .filter_map(|tbox| {
                let width =
                    legaia_engine_render::battle_tutorial_text_width(&self.font, &tbox.text);
                let (x, y, w, h) = tbox.rect(width)?;
                Some(((x as i32, y as i32, w as i32, h as i32), tbox))
            })
            .collect()
    }

    /// Sparring-tutorial prompt-box chrome: the same gradient fill + gold
    /// 9-slice frame the dialog reading box wears, at the rect the retail
    /// emitter registers the prompt's text actor with. Sampled from the
    /// resident system-UI atlas; composited in the shared chrome sprite slot,
    /// under the text layer.
    pub(in crate::window) fn battle_tutorial_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        let Some(assets) = self.save_menu.as_ref() else {
            return Vec::new();
        };
        if self.boot_ui.is_active() {
            return Vec::new();
        }
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        let mut out = Vec::new();
        for (rect, tbox) in self.battle_tutorial_stage_boxes() {
            out.extend(legaia_engine_render::battle_tutorial_chrome_draws_for(
                &assets.rects,
                rect,
                tbox.waits_for_input,
                stage_origin,
                stage_scale,
            ));
        }
        // Koru's timed-fight strip wears the same skin (both are text actors
        // registered with an explicit rect and style word `0x44`).
        if legaia_engine_core::timed_fight::timed_fight_strip(&self.session.host.world).is_some() {
            out.extend(legaia_engine_render::timed_fight_strip_chrome_draws(
                &assets.rects,
                stage_origin,
                stage_scale,
            ));
        }
        out
    }
}
