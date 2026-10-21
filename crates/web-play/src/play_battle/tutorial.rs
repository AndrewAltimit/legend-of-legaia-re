//! The sparring tutorial's prompt boxes: stage rect, box list, text and
//! chrome draws, and the JSON model export.

use super::*;

impl LegaiaRuntime {
    /// Tutorial prompt rows and Koru's timed-fight strip in stage pixels
    /// (`legaia_engine_screens::battle_screens::battle_tutorial_stage_draws`,
    /// the native window's call too).
    pub(crate) fn battle_tutorial_stage_draws(
        &self,
        font: &legaia_font::Font,
        has_chrome: bool,
    ) -> Vec<TextDraw> {
        let Some(host) = self.scene_host.host() else {
            return Vec::new();
        };
        legaia_engine_screens::battle_screens::battle_tutorial_stage_draws(
            &host.world,
            font,
            has_chrome,
        )
    }

    /// Tutorial prompt-box chrome and Koru's timed-fight strip
    /// (`legaia_engine_screens::battle_screens::battle_tutorial_chrome_draws`,
    /// the native window's call too).
    pub(crate) fn battle_tutorial_chrome_draws(
        &self,
        font: &legaia_font::Font,
        rects: &ui::SaveMenuAtlasRects,
        origin: (i32, i32),
        scale: u32,
    ) -> Vec<SpriteDraw> {
        let Some(host) = self.scene_host.host() else {
            return Vec::new();
        };
        legaia_engine_screens::battle_screens::battle_tutorial_chrome_draws(
            &host.world,
            font,
            rects,
            origin,
            scale,
        )
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// The live sparring-tutorial model behind the prompt boxes the overlay
    /// composes, as JSON.
    ///
    /// `{"armed": false, "prompts": <n>, "flag_armed": <bool>}` outside the
    /// tutorial fight, where `prompts` is how many prompt strings the disc
    /// supplied and `flag_armed` whether the disc's one-shot arm
    /// (`battle_tutorial::TUTORIAL_ARM_FLAG`) is currently raised. While
    /// armed it gains `"lesson": <0..=5>`, `"queued": <n>` and
    /// `"box": {"text", "style", "waits", "rect": [x, y, w, h]}` (`null`
    /// between prompts).
    ///
    /// The boxes themselves ride `play_overlay_draws_json`; this is the state
    /// the page can read to know a prompt owns the pad (a waiting box parks
    /// the whole battle loop, so a host that shows a "press X" affordance
    /// needs to know). It is also the surface on which "the page never primes
    /// the tutorial" is a *measurable* claim rather than a count of text rows
    /// that happen to be zero.
    pub fn play_battle_tutorial_json(&self) -> String {
        let Some(world) = self.scene_host.host().map(|h| &h.world) else {
            return r#"{"armed":false,"prompts":0,"flag_armed":false}"#.to_string();
        };
        let prompts = world.battle.tutorial_script.len();
        let flag_armed =
            world.system_flag_test(legaia_engine_battle::battle_tutorial::TUTORIAL_ARM_FLAG);
        let Some(lesson) = world.battle_tutorial_lesson() else {
            return serde_json::json!({
                "armed": false,
                "prompts": prompts,
                "flag_armed": flag_armed,
            })
            .to_string();
        };
        // Same font the compose measures with, so the reported rect is the
        // rect the page draws - the disc's own proportional font when it
        // decoded, the placeholder metrics otherwise.
        let font = self
            .menu_font
            .clone()
            .unwrap_or_else(legaia_font::Font::placeholder);
        let tbox = world.battle_tutorial_box().map(|b| {
            let width = ui::battle_tutorial_text_width(&font, &b.text);
            let rect = b.rect(width);
            serde_json::json!({
                "text": b.text,
                "style": b.style,
                "waits": b.waits_for_input,
                "rect": rect.map(|(x, y, w, h)| vec![x, y, w, h]),
            })
        });
        serde_json::json!({
            "armed": true,
            "prompts": prompts,
            "flag_armed": flag_armed,
            "lesson": lesson.raw(),
            "box": tbox,
            "queued": world.battle.tutorial_boxes.len(),
        })
        .to_string()
    }
}
