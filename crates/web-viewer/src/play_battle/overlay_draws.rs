//! The battle overlay's draw lists: spoils chrome, post-battle overlay, arts
//! input strip, HUD frame, banner, command chips, chrome sprites and the
//! composed `battle_overlay_draws`.

use super::*;

impl LegaiaRuntime {
    /// Out-of-battle battle presentation, in **surface pixels**: the
    /// post-battle spoils panel and the game-over panel. Both sit outside
    /// [`SceneMode::Battle`], which is why they are not part of
    /// [`Self::battle_overlay_draws`], and both use the same shared
    /// `engine-ui` builder + world model as the native window
    /// (`window/battle.rs`, `window/boot_cutscene.rs`).
    ///
    /// The "this scene rolls no encounters" hint does **not** belong here.
    /// A non-empty overlay clears the canvas before it blits, so a passive
    /// hint routed through this list would wipe whatever the frame had
    /// already painted for the first seconds of a town. The page reads
    /// [`Self::scene_rolls_encounters`] and prints its own notice instead.
    /// The post-battle report's window chrome (gold nine-slice over the blue
    /// fill), one frame per window the shared builder describes. Rides the
    /// page's system-UI sprite array with the rest of the menu chrome.
    pub(crate) fn battle_spoils_chrome_sprite_draws(
        &self,
        assets: &crate::play_menu::PlayMenuAssets,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<ui::SpriteDraw> {
        if self.game_over.is_some() {
            return Vec::new();
        }
        let Some(w) = self.scene_host.host().map(|h| &h.world) else {
            return Vec::new();
        };
        let Some(rects) = assets.chrome_rects() else {
            return Vec::new();
        };
        // The loss window (screen element `0x42`) shares the report frame.
        if let Some(defeat) = w.battle_defeat_banner() {
            let (origin, scale) =
                crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
            return ui::battle_defeat_windows(defeat.line.as_deref(), defeat.slide_y)
                .iter()
                .flat_map(|win| ui::menu_window_chrome_draws_for(rects, win.rect, origin, scale))
                .collect();
        }
        let Some(banner) = w.battle_spoils_banner() else {
            return Vec::new();
        };
        let leader = w.battle_spoils_leader();
        let view = ui::BattleSpoilsView {
            xp: banner.xp,
            gold: banner.gold,
            level_ups: &banner.level_ups,
            drops: &banner.drops,
            leader: &leader,
            subject: banner.subject,
            slide: banner.slide,
        };
        let (origin, scale) = crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
        ui::battle_spoils_windows(&view)
            .iter()
            .flat_map(|win| ui::menu_window_chrome_draws_for(rects, win.rect, origin, scale))
            .collect()
    }

    pub(crate) fn post_battle_overlay_draws(
        &self,
        assets: &crate::play_menu::PlayMenuAssets,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<TextDraw> {
        let Some(w) = self.scene_host.host().map(|h| &h.world) else {
            return Vec::new();
        };
        let font = assets.font_ref();
        let mut out: Vec<TextDraw> = Vec::new();

        // Party wipe owns the frame, and draws nothing on it: retail's next
        // frame after the wipe store is the title overlay fading in, so the
        // hand-off holds the frozen battle frame and adds no chrome. The
        // native host's `boot_ui_draws` arm is silent for the same reason.
        if self.game_over.is_some() {
            return out;
        }

        if let Some(defeat) = w.battle_defeat_banner() {
            let windows = ui::battle_defeat_windows(defeat.line.as_deref(), defeat.slide_y);
            let (origin, scale) =
                crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
            let mut draws = ui::battle_result_line_draws_for(font, &windows);
            ui::scale_stage_text_draws(&mut draws, origin, scale);
            out.extend(draws);
        } else if let Some(banner) = w.battle_spoils_banner() {
            let leader = w.battle_spoils_leader();
            let view = ui::BattleSpoilsView {
                xp: banner.xp,
                gold: banner.gold,
                level_ups: &banner.level_ups,
                drops: &banner.drops,
                leader: &leader,
                subject: banner.subject,
                slide: banner.slide,
            };
            let windows = ui::battle_spoils_windows(&view);
            let (origin, scale) =
                crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
            let mut draws = ui::battle_spoils_draws_for(font, &view, &windows);
            ui::scale_stage_text_draws(&mut draws, origin, scale);
            out.extend(draws);
        }

        out
    }

    /// The **arts command-input** chrome for the browser play page:
    /// `(sprites, texts)`. The sprites are the shared composition
    /// (`legaia_engine_screens::battle_screens::arts_input_chrome_draws`, the
    /// native window's call too), placed by the stage `origin` / `scale`; the
    /// screen is baked art, so the text half is always empty - the Begin |
    /// Reselect pick is the party's commit confirm, drawn by the command-chip
    /// cluster.
    pub(crate) fn arts_input_stage_draws(
        &self,
        _font: &legaia_font::Font,
        chrome: Option<&ui::SaveMenuAtlasRects>,
        origin: (i32, i32),
        scale: u32,
    ) -> (Vec<ui::SpriteDraw>, Vec<TextDraw>) {
        let Some(world) = self.scene_host.host().map(|h| &h.world) else {
            return (Vec::new(), Vec::new());
        };
        (
            legaia_engine_screens::battle_screens::arts_input_chrome_draws(
                world, chrome, origin, scale,
            ),
            Vec::new(),
        )
    }

    /// The shared battle screens' inputs, borrowed from this page's holders;
    /// `None` while no scene is up.
    pub(crate) fn battle_screen_inputs<'a>(
        &'a self,
        assets: &'a crate::play_menu::PlayMenuAssets,
    ) -> Option<legaia_engine_screens::battle_screens::BattleScreenInputs<'a>> {
        let font = assets.font_ref();
        Some(legaia_engine_screens::battle_screens::BattleScreenInputs {
            world: &self.scene_host.host()?.world,
            hud: &self.battle_hud,
            font,
            chrome: assets.chrome_rects(),
            badges: assets.battle_badges(),
            solid_src: ui::font_solid_src(font),
        })
    }

    /// The battle HUD's chrome sprites, the item window's frames and the
    /// command chips, or outside battle the field-side message banner's frame,
    /// for the page's system-UI atlas array. The composition is
    /// `legaia_engine_screens::battle_screens::battle_chrome_sprite_draws`,
    /// the native window's call too.
    pub(crate) fn battle_chrome_sprite_draws(
        &self,
        assets: &crate::play_menu::PlayMenuAssets,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<ui::SpriteDraw> {
        self.battle_screen_inputs(assets)
            .map(|inputs| {
                legaia_engine_screens::battle_screens::battle_chrome_sprite_draws(
                    &inputs,
                    surface_w.max(1),
                    surface_h.max(1),
                )
            })
            .unwrap_or_default()
    }

    /// Battle overlay text draws in **surface pixels**: the HUD frame's text,
    /// the font fallback of the floating value readout, the diagnostic
    /// encounter banner and the player-driven submenus, or a Muscle Dome
    /// leg's HUD text and chip labels. The list is the shared
    /// `legaia_engine_screens::battle_screens::battle_text_draws` (the native
    /// window's call too) plus the page's value-readout fallback.
    ///
    /// The sparring-tutorial prompt box is not part of this list: its rect is
    /// in retail's 320x240 stage space, and it is folded into the
    /// stage-scaled group in `play_overlay_draws_json`.
    pub(crate) fn battle_overlay_draws(
        &self,
        assets: &crate::play_menu::PlayMenuAssets,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<TextDraw> {
        let Some(inputs) = self.battle_screen_inputs(assets) else {
            return Vec::new();
        };
        let mut out = legaia_engine_screens::battle_screens::battle_text_draws(
            &inputs,
            self.encounter_banner.as_ref().map(|(_, l)| l.as_str()),
            surface_w.max(1),
            surface_h.max(1),
        );
        // Floating value readout: the numeral a landed hit throws, over the
        // struck actor. With a battle render up, the page draws retail's own
        // 24x24 cells off the resident effect atlas through the screen-prim
        // pass (`battle_value_readout_prims`) and this font fallback stays
        // empty; it only draws before the battle VRAM exists.
        if inputs.world.mode == SceneMode::Battle {
            out.extend(self.battle_value_readout_draws(assets.font_ref(), surface_w, surface_h));
        }
        out
    }
}
