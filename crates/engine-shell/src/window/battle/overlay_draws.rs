//! The battle's text and sprite overlays outside the HUD proper: the spoils
//! screen, the passive-ability readout and the encounter hint.

use super::*;

impl PlayWindowApp {
    /// Post-battle report text - the spoils panel or the loss window - or
    /// empty when neither is up
    /// (`legaia_engine_screens::battle_screens::battle_result_text_draws`, the
    /// browser play page's call too).
    pub(in crate::window) fn battle_spoils_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<TextDraw> {
        legaia_engine_screens::battle_screens::battle_result_text_draws(
            &self.session.host.world,
            &self.font,
            surface_w,
            surface_h,
        )
    }

    /// The post-battle report's window chrome
    /// (`legaia_engine_screens::battle_screens::battle_result_chrome_draws`).
    /// Rides the system-UI atlas slot with the rest of the menu chrome; empty
    /// before the atlas is resident.
    pub(in crate::window) fn battle_spoils_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_ui::SpriteDraw> {
        let Some(rects) = self.save_menu.as_ref().map(|a| &a.rects) else {
            return Vec::new();
        };
        legaia_engine_screens::battle_screens::battle_result_chrome_draws(
            &self.session.host.world,
            rects,
            surface_w,
            surface_h,
        )
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
    /// The engine's half is `WorldPassiveHud::passive_hud_points` (three head-relative
    /// world points, lifted by fractions of the player's `+0x72`) and
    /// `WorldPassiveHud::passive_hud_icons` (the icon list for a resolved anchor); the
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
        let icons = legaia_engine_screens::field_screens::passive_hud_icons(
            &self.session.host.world,
            &cam.to_cols_array(),
        );
        let (origin, scale) = self.save_select_stage(surface_w, surface_h);
        legaia_engine_screens::field_screens::passive_hud_text_draws(
            &self.font, &icons, origin, scale,
        )
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
