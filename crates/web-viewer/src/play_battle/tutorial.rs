//! The sparring tutorial's prompt boxes: stage rect, box list, text and
//! chrome draws, and the JSON model export.

use super::*;

impl LegaiaRuntime {
    /// Every tutorial box on screen with its stage rect
    /// (`legaia_engine_screens::battle_screens::battle_tutorial_stage_boxes`,
    /// the native window's call too).
    pub(crate) fn battle_tutorial_stage_boxes(
        &self,
        font: &legaia_font::Font,
    ) -> Vec<TutorialStageBox<'_>> {
        let Some(host) = self.scene_host.host() else {
            return Vec::new();
        };
        legaia_engine_screens::battle_screens::battle_tutorial_stage_boxes(&host.world, font)
    }

    /// Tutorial prompt rows in stage pixels, at the rect origin on the retail
    /// 14-px pitch.
    pub(crate) fn battle_tutorial_stage_draws(
        &self,
        font: &legaia_font::Font,
        has_chrome: bool,
    ) -> Vec<TextDraw> {
        let mut out = Vec::new();
        for (rect, tbox) in self.battle_tutorial_stage_boxes(font) {
            out.extend(ui::battle_tutorial_text_draws_for(font, &tbox.text, rect));
            // Without the system-UI atlas there is no frame and no advance
            // hand, so keep a plain confirm hint as the only affordance a
            // waiting box would otherwise have.
            if tbox.waits_for_input && !has_chrome {
                let lines = tbox.text.lines().count() as i32;
                out.extend(ui::text_draws_for(
                    &font.layout_ascii("Cross=continue"),
                    (rect.0, rect.1 + lines * 14),
                    [0.75, 0.75, 0.8, 1.0],
                ));
            }
        }
        // Koru's timed-fight strip - the same stage-space text-actor family,
        // one shared builder with the native window.
        if let Some(strip) = self
            .scene_host
            .host()
            .and_then(|h| legaia_engine_core::world_views::timed_fight::timed_fight_strip(&h.world))
        {
            out.extend(ui::timed_fight_strip_text_draws(
                font,
                &ui::TimedFightStripView {
                    label: &strip.label,
                    turns_left: strip.turns_left,
                    hp_left: strip.hp_left,
                },
            ));
        }
        out
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
