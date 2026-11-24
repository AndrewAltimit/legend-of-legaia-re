//! The run rules on the play page: the New Game rules screen the title
//! hands NEW GAME to, the Boss Rush's stage entries and screens, and an
//! ironman run's card rules over the rack's port 1.
//!
//! Every rule is the engine's (`legaia_engine_session::run_rules`, the
//! native window's too): the rules screen sits on the world, so its
//! composition is the shared `legaia_engine_screens::run_rules` one, drawn
//! here onto the title's menu canvas ([`LegaiaRuntime::play_run_rules_draws_json`])
//! and in game through the screen-prim pass and the overlay's text layer.

use legaia_engine_session::run_rules::{
    BossRushHostStep, RunRulesStep, SeedKey, boss_rush_screen_input, boss_rush_screen_up,
    boss_rush_step_on, open_run_rules_screen, seed_editor_key, seed_editor_open,
    seed_editor_set_text, set_run_rules_seed, step_run_rules_screen,
};
use legaia_render_kernels::screen_prim::ScreenPrim;
use wasm_bindgen::prelude::*;

use crate::play_menu::{quad_json, stage_transform};
use crate::runtime::LegaiaRuntime;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Open the New Game rules screen over the card rack (the native
    /// window's NEW GAME arm). `false` without a disc session or the menu
    /// font, or in retail-faithful mode, which leaves the page to start the
    /// retail New Game.
    pub fn play_run_rules_open(&mut self) -> bool {
        // Retail-faithful: NEW GAME is retail's straight start.
        if self.page_toggles.retail || !self.ensure_menu_assets() {
            return false;
        }
        let entropy = js_sys_now_entropy();
        let ports = [self.cards[0].as_ref(), self.cards[1].as_ref()];
        let Some(h) = self.scene_host.host_mut() else {
            return false;
        };
        open_run_rules_screen(&mut h.world, ports, entropy);
        true
    }

    /// Step the rules screen on an edge-triggered pad word. `""` while it
    /// runs, `"back"` when the player backed out (the page reopens the
    /// title), `"start"` when the run is staged: the page then calls
    /// [`Self::play_new_game`], which starts it (story or Boss Rush).
    pub fn play_run_rules_step(&mut self, edge: u16) -> String {
        let Some(session) = self.scene_host.session_mut() else {
            return String::new();
        };
        match step_run_rules_screen(&mut session.host.world, edge) {
            RunRulesStep::Running => String::new(),
            RunRulesStep::Cancelled => "back".to_string(),
            RunRulesStep::Start { rules, source } => {
                crate::console_log(&format!("new game: run rules {:?}", rules.summary()));
                session.stage_run_rules(rules, source.map(|s| *s));
                "start".to_string()
            }
        }
    }

    /// Type a randomizer seed into the open rules screen.
    pub fn play_run_rules_set_seed(&mut self, seed: &str) {
        if let Some(h) = self.scene_host.host_mut() {
            set_run_rules_seed(&mut h.world, seed);
        }
    }

    /// Whether the rules screen's seed editor is up: the page shows its
    /// text box over the canvas ([`Self::play_run_rules_seed_rect`]) while
    /// it is.
    pub fn play_run_rules_seed_editing(&self) -> bool {
        self.scene_host
            .host()
            .is_some_and(|h| seed_editor_open(&h.world))
    }

    /// The seed editor's text, as the page's text box shows it.
    pub fn play_run_rules_seed_text(&self) -> String {
        self.scene_host
            .host()
            .and_then(|h| h.world.run_rules.setup.as_ref())
            .and_then(|s| s.setup.seed_editor.as_ref())
            .map(|e| e.text.clone())
            .unwrap_or_default()
    }

    /// Replace the seed editor's text from the page's text box.
    pub fn play_run_rules_seed_input(&mut self, text: &str) {
        if let Some(h) = self.scene_host.host_mut() {
            seed_editor_set_text(&mut h.world, text);
        }
    }

    /// Close the seed editor from the page's text box: Enter takes the
    /// seed (`take`), Escape drops the edit.
    pub fn play_run_rules_seed_close(&mut self, take: bool) {
        if let Some(h) = self.scene_host.host_mut() {
            let key = if take {
                SeedKey::Accept
            } else {
                SeedKey::Cancel
            };
            seed_editor_key(&mut h.world, key);
        }
    }

    /// Where the page puts its seed text box, in surface pixels
    /// `[x, y, w, h]`: the editor window's text line.
    pub fn play_run_rules_seed_rect(&self, surface_w: u32, surface_h: u32) -> Vec<i32> {
        let (origin, scale) = stage_transform(surface_w.max(1), surface_h.max(1));
        let s = scale as i32;
        let (x, y, w, _) = legaia_engine_ui::run_rules_screen::MENU_GRID_FRAME;
        let (x, y, w, h) = (x + 52, y + 5, w - 68, 18);
        vec![origin.0 + x * s, origin.1 + y * s, w * s, h * s]
    }

    /// The rules screen's draws for the title's menu canvas, in surface
    /// pixels: `{"open", "rects": [{"x","y","w","h","rgba"}], "sprites",
    /// "texts"}` - the backdrop as rects, the windows as chrome-atlas
    /// sprites, the labels as font quads.
    pub fn play_run_rules_draws_json(&self, surface_w: u32, surface_h: u32) -> String {
        let (Some(h), Some(assets)) = (self.scene_host.host(), self.menu_assets.as_ref()) else {
            return r#"{"open":false,"rects":[],"texts":[]}"#.to_string();
        };
        let Some(screen) =
            legaia_engine_screens::run_rules::active_screen(&h.world, &prompt_keys())
        else {
            return r#"{"open":false,"rects":[],"texts":[]}"#.to_string();
        };
        let (origin, scale) = stage_transform(surface_w.max(1), surface_h.max(1));
        let s = scale as i32;
        let rects: Vec<_> = screen
            .prims()
            .iter()
            .filter_map(|p| match p {
                ScreenPrim::Flat(q) => {
                    let xs = q.xy.map(|(x, _)| i32::from(x));
                    let ys = q.xy.map(|(_, y)| i32::from(y));
                    let (x0, y0) = (*xs.iter().min()?, *ys.iter().min()?);
                    let (x1, y1) = (*xs.iter().max()?, *ys.iter().max()?);
                    Some(serde_json::json!({
                        "x": origin.0 + x0 * s,
                        "y": origin.1 + y0 * s,
                        "w": (x1 - x0) * s,
                        "h": (y1 - y0) * s,
                        "rgba": q.color,
                    }))
                }
                _ => None,
            })
            .collect();
        let mut texts = screen.texts(assets.font_ref());
        legaia_engine_ui::scale_stage_text_draws(&mut texts, origin, scale);
        let sprites = assets
            .chrome_rects()
            .map(|r| screen.sprites(r, origin, scale))
            .unwrap_or_default();
        serde_json::json!({
            "open": true,
            "rects": rects,
            "sprites": sprites.iter().map(quad_json).collect::<Vec<_>>(),
            "texts": texts.iter().map(quad_json).collect::<Vec<_>>(),
        })
        .to_string()
    }

    /// The run's one-line summary for the page's status line (`""` under
    /// the retail rules).
    pub fn play_run_summary(&self) -> String {
        self.scene_host
            .host()
            .and_then(|h| h.world.run_rules_summary())
            .unwrap_or_default()
    }
}

impl LegaiaRuntime {
    /// The run rules' frame step, the native window's `poll_run_rules`
    /// twin: a Boss Rush's next stage entered and armed, and an ironman
    /// run's card rules applied to the rack's port 1 (flagged written for the
    /// page to store). Returns the scene a stage entered, for the frame's
    /// rebuild.
    pub(crate) fn poll_run_rules(&mut self) -> Option<&'static str> {
        let entered = match boss_rush_step_on(&mut crate::resume::PageNewGame(self)) {
            BossRushHostStep::Entered(scene) => Some(scene),
            _ => None,
        };
        let h = self.scene_host.session_mut()?;
        let erase = h.host.world.take_ironman_erase();
        let autosave = h.host.world.take_run_autosave();
        if erase.is_none() && !autosave {
            return entered;
        }
        use legaia_engine_save_state::WorldSaveState;
        let sf = autosave.then(|| h.host.world.save_full());
        let resume = h.current_resume();
        let index = h.host.index.clone();
        let Some(card) = self.cards[0].as_mut() else {
            return entered;
        };
        use legaia_engine_menu_runtime::card_write::{ironman_autosave, ironman_erase_run};
        if let Some(run_id) = erase {
            ironman_erase_run(card, run_id, None);
        }
        if let Some(sf) = sf
            && let Err(e) = ironman_autosave(card, &sf, &resume, Some(&index))
        {
            crate::console_log(&format!("ironman: autosave failed: {e}"));
        }
        self.cards_written[0] = true;
        entered
    }

    /// Whether the Boss Rush intermission or result owns the frame.
    pub(crate) fn boss_rush_screen_owns_frame(&self) -> bool {
        self.scene_host
            .host()
            .is_some_and(|h| boss_rush_screen_up(&h.world))
    }

    /// One tick of the Boss Rush screen on `edge`. `true` when the player
    /// dismissed a finished run's result: the page returns to the title.
    pub(crate) fn boss_rush_screen_step(&mut self, edge: u16) -> bool {
        self.scene_host
            .host_mut()
            .is_some_and(|h| boss_rush_screen_input(&mut h.world, edge))
    }
}

/// The rules screens' key names: the page's live bindings, or the
/// controller's buttons while one was pressed last
/// ([`crate::pad_bindings::prompt_label`], the minigame captions' rule).
pub(crate) fn prompt_keys() -> legaia_engine_screens::run_rules::PromptKeys {
    legaia_engine_screens::run_rules::PromptKeys {
        keyboard: crate::pad_bindings::last_pad().is_none(),
        ..legaia_engine_screens::run_rules::PromptKeys::from_labels(
            crate::pad_bindings::prompt_label,
        )
    }
}

/// A seed-roll entropy word off the page clock (wall time in the browser,
/// a fixed word off it).
fn js_sys_now_entropy() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() as u64) ^ 0x5EED
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0x5EED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rules_screen_draws_nothing_until_opened() {
        let rt = LegaiaRuntime::new();
        assert!(
            rt.play_run_rules_draws_json(640, 480)
                .contains("\"open\":false")
        );
        assert_eq!(rt.play_run_summary(), "");
    }
}
