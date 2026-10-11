//! The battle's text and sprite overlays outside the HUD proper: the spoils
//! screen, the passive-ability readout and the encounter hint.

use super::*;

impl PlayWindowApp {
    /// Post-battle spoils panel draws, or empty when the panel is not up.
    ///
    /// Reads [`legaia_engine_core::world::World::battle_spoils_banner`] (the
    /// timer + the name resolution both live in engine-core) and feeds the
    /// shared `engine-ui` builder, so the browser play page draws the exact
    /// same panel from the exact same model.
    pub(in crate::window) fn battle_spoils_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<TextDraw> {
        if let Some(defeat) = self.session.host.world.battle_defeat_banner() {
            let windows =
                legaia_engine_ui::battle_defeat_windows(defeat.line.as_deref(), defeat.slide_y);
            let (origin, scale) = self.save_select_stage(surface_w, surface_h);
            let mut draws = legaia_engine_ui::battle_result_line_draws_for(&self.font, &windows);
            legaia_engine_ui::scale_stage_text_draws(&mut draws, origin, scale);
            return draws;
        }
        let Some(banner) = self.session.host.world.battle_spoils_banner() else {
            return Vec::new();
        };
        let leader = self.session.host.world.battle_spoils_leader();
        let view = legaia_engine_ui::BattleSpoilsView {
            xp: banner.xp,
            gold: banner.gold,
            level_ups: &banner.level_ups,
            drops: &banner.drops,
            leader: &leader,
            subject: banner.subject,
            slide: banner.slide,
        };
        let windows = legaia_engine_ui::battle_spoils_windows(&view);
        let (origin, scale) = self.save_select_stage(surface_w, surface_h);
        let mut draws = legaia_engine_ui::battle_spoils_draws_for(&self.font, &view, &windows);
        legaia_engine_ui::scale_stage_text_draws(&mut draws, origin, scale);
        draws
    }

    /// The post-battle report's window chrome - the gold nine-slice over the
    /// blue fill, one frame per window the shared builder describes. Rides
    /// the system-UI atlas slot with the rest of the menu chrome.
    pub(in crate::window) fn battle_spoils_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_ui::SpriteDraw> {
        let Some(rects) = self.save_menu.as_ref().map(|a| &a.rects) else {
            return Vec::new();
        };
        // The loss window (screen element `0x42`) shares the report frame.
        if let Some(defeat) = self.session.host.world.battle_defeat_banner() {
            let (origin, scale) = self.save_select_stage(surface_w, surface_h);
            return legaia_engine_ui::battle_defeat_windows(defeat.line.as_deref(), defeat.slide_y)
                .iter()
                .flat_map(|w| {
                    legaia_engine_ui::menu_window_chrome_draws_for(rects, w.rect, origin, scale)
                })
                .collect();
        }
        let Some(banner) = self.session.host.world.battle_spoils_banner() else {
            return Vec::new();
        };
        let leader = self.session.host.world.battle_spoils_leader();
        let view = legaia_engine_ui::BattleSpoilsView {
            xp: banner.xp,
            gold: banner.gold,
            level_ups: &banner.level_ups,
            drops: &banner.drops,
            leader: &leader,
            subject: banner.subject,
            slide: banner.slide,
        };
        let (origin, scale) = self.save_select_stage(surface_w, surface_h);
        legaia_engine_ui::battle_spoils_windows(&view)
            .iter()
            .flat_map(|w| {
                legaia_engine_ui::menu_window_chrome_draws_for(rects, w.rect, origin, scale)
            })
            .collect()
    }

    /// One dim HUD line while the live loop is armed on a scene that cannot
    /// roll a random encounter.
    ///
    /// Several retail scenes - `town01`, the scene the binary boots into,
    /// among them - have every rollable encounter region shadowed by an
    /// earlier rate-0 row, so walking them forever produces nothing. That is
    /// scene data the port keeps faithfully; without a hint it reads as the
    /// engine being broken, which is exactly how it was reported.
    /// The field overlay's **passive-ability badge column** - the icons
    /// retail floats over the player's head while an accessory passive is
    /// active (`FUN_801d095c`).
    ///
    /// The engine's half is `World::passive_hud_points` (three head-relative
    /// world points, lifted by fractions of the player's `+0x72`) and
    /// `World::passive_hud_icons` (the icon list for a resolved anchor); the
    /// projection between them is this host's, because the camera is. Retail
    /// takes the **X of the first** projected point and the **Y of the
    /// third** - a mixed pair, not one point's.
    ///
    /// The icon ids `0x47..=0x4D` name cells of the field pictogram bank
    /// (`FUN_8002C488`), which this host has no sprite atlas for, so each
    /// draws as the same ASCII stand-in the menu painters' pictograms use.
    /// The browser play page draws the same list off the same World seat.
    pub(in crate::window) fn passive_hud_draws(
        &self,
        cam: glam::Mat4,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<TextDraw> {
        // The badge column is not a screen of its own: retail reaches
        // `FUN_801d095c` only through `FUN_801D0D38`'s `jal` at `0x801D130C`,
        // below the `_DAT_8007B868` gate whose suppress arm jumps past it. So
        // the badges answer the party readout's suppression, and this host
        // asks the same shared kernel the readout does. Gating only on "no
        // boot-UI panel is up" left the badges painted over every dialog box,
        // every cutscene beat and every fight; the browser play page has
        // asked the full question since its own column landed.
        if self.field_party_hud_suppressed() {
            return Vec::new();
        }
        let world = &self.session.host.world;
        if !world.passive_hud_active() {
            return Vec::new();
        }
        let Some(points) = world.passive_hud_points() else {
            return Vec::new();
        };
        let project = |p: [f32; 3]| -> Option<(i32, i32)> {
            let clip = cam * glam::Vec4::new(p[0], p[1], p[2], 1.0);
            if clip.w <= 0.01 {
                return None;
            }
            let ndc = clip.truncate() / clip.w;
            Some((
                ((ndc.x * 0.5 + 0.5) * 320.0) as i32,
                ((0.5 - ndc.y * 0.5) * 240.0) as i32,
            ))
        };
        let (Some(first), Some(third)) = (project(points[0]), project(points[2])) else {
            return Vec::new();
        };
        let (origin, scale) = self.save_select_stage(surface_w, surface_h);
        let mut out = Vec::new();
        for i in world.passive_hud_icons((first.0, third.1)) {
            let mut draws = text_draws_for(
                &self.font.layout_ascii("*"),
                (i.x, i.y),
                legaia_engine_ui::MENU_TEXT_GOLD,
            );
            legaia_engine_ui::scale_stage_text_draws(&mut draws, origin, scale);
            out.extend(draws);
        }
        out
    }

    pub(in crate::window) fn encounter_hint_draws(&self, _w: u32, surface_h: u32) -> Vec<TextDraw> {
        // A diagnostic row like the shell's others, so it takes the same
        // toggle: retail prints nothing here, and a permanent caption over
        // the game is worse than the confusion it was written to prevent.
        if !self.diag_rows || !self.session.host.world.show_encounter_hint() {
            return Vec::new();
        }
        let dim = [0.7f32, 0.7, 0.7, 1.0];
        text_draws_for(
            &self
                .font
                .layout_ascii("no random encounters in this scene (retail)"),
            (8, surface_h as i32 - 20),
            dim,
        )
    }
}
