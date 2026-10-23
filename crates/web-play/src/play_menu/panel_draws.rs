//! `play_menu_draws_json`: the page's pause-menu draw export. Every screen
//! but Load / Save is the shared projection
//! (`legaia_engine_screens::pause_screens`), the native window's call too.

use super::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Build the two draw lists for the current menu state, in surface pixels.
    /// Shape:
    /// ```text
    /// { "open": true,
    ///   "sprites": [ { "dst":[x,y,w,h], "src":[x,y,w,h], "color":[r,g,b,a] } ],
    ///   "texts":   [ ... ] }
    /// ```
    /// `sprites` sample the chrome atlas, `texts` the font atlas. `open` is
    /// `false` (and the lists empty) when no menu is up.
    pub fn play_menu_draws_json(&self, surface_w: u32, surface_h: u32) -> String {
        let (Some(menu), Some(assets)) = (self.play_menu.as_ref(), self.menu_assets.as_ref())
        else {
            return r#"{"open":false,"sprites":[],"texts":[]}"#.to_string();
        };
        // Nothing to draw before the wipe spawns the menu: the field shows.
        if !self.pause_menu_spawned() {
            return r#"{"open":false,"sprites":[],"texts":[]}"#.to_string();
        }
        let (origin, scale) = stage_transform(surface_w.max(1), surface_h.max(1));
        let mut sprites: Vec<SpriteDraw> = Vec::new();
        let mut texts: Vec<TextDraw> = Vec::new();
        // The save screen's subtractive darkening (`ui::SaveScreenDarken`):
        // the sprite index of its `B - F` quad and the grey it subtracts.
        let mut darken: Option<(usize, u8)> = None;

        // The kind-0x0D entry pair replaces the root list rather than
        // overlaying it: both sub-screens open with `05 00` (close every
        // window) before opening their own, so the command rows are not on
        // screen while either is up.
        let ctx = assets.menu_ctx(origin, scale);
        let world = self.menu_world();
        let context_screen = match (self.root_menu(), world) {
            (Some(root), Some(w)) => pause_screens::context_locked_screen_draws(&ctx, root, w),
            _ => PauseMenuDraws::default(),
        };
        let screen = match &menu.sub {
            _ if !context_screen.is_empty() => context_screen,
            None => match (self.root_menu(), world) {
                (Some(root), Some(w)) => pause_screens::field_menu_root_draws(
                    &ctx,
                    root,
                    w,
                    menu.save_flow.refusal().map(|r| r.message()),
                ),
                _ => PauseMenuDraws::default(),
            },
            Some(PlaySub::Session(sub)) => match sub.as_ref() {
                FieldMenuSubsession::Save(s) => {
                    darken = self.build_save_select(
                        assets,
                        s,
                        &menu.save_flow,
                        &mut sprites,
                        &mut texts,
                        origin,
                        scale,
                    );
                    PauseMenuDraws::default()
                }
                sub => pause_screens::field_menu_sub_draws(
                    &ctx,
                    sub,
                    world,
                    self.menu.equip_info.as_ref(),
                ),
            },
        };
        sprites.extend(screen.sprites);
        texts.extend(screen.texts);

        // Window 7 - the spell level-up notice - overlays whichever menu
        // screen is current while the shared runtime holds the beat. Both
        // hosts route it through the one shared composition, so neither can
        // pick a different painter or a different stand-in cursor.
        if let Some(notice) = self.menu.spell_level_notice() {
            texts.extend(spell_level_notice_draws(
                &assets.menu_ctx(origin, scale),
                &notice.line,
            ));
        }
        // Window 8 - the art-learned notice, through the same shared
        // composition the native window calls.
        if let Some(notice) = self.menu.art_learned_notice() {
            texts.extend(art_learned_notice_draws(
                &assets.menu_ctx(origin, scale),
                &notice.lines,
            ));
        }

        serde_json::json!({
            "open": true,
            "sprites": sprites.iter().map(quad_json).collect::<Vec<_>>(),
            "texts": texts.iter().map(quad_json).collect::<Vec<_>>(),
            // The page's 2D canvas has no `B - F` composite: it blits the
            // sprites before `at`, subtracts `level` from every channel of
            // what is drawn, and blits the rest past the quad itself.
            "darken": darken.map(|(at, level)| serde_json::json!({"at": at, "level": level})),
        })
        .to_string()
    }
}
