//! The title's CONTINUE screen on the play page: the session's one owner
//! ([`ContinueScreen`]), the native window's too, with only this host's glue:
//! the card rack in, the blip and the parked save out, the draws through the
//! shared save-select composition.
//!
//! The page used to stand Continue up as a title-opened pause-menu
//! sub-screen (a `PlayMenu` with a Load sub-session), stepped through
//! `play_menu_input` and drawn through `play_menu_draws_json`; the native
//! window ran it as a standalone screen with a different frame order. The
//! title's outcome `"continue"` now hands the frame to
//! [`LegaiaRuntime::boot_continue_step`] /
//! [`LegaiaRuntime::boot_continue_draws_json`] until the step reports
//! `"loaded"` (the save is parked for `play_resume_save`) or `"back"` (the
//! page reopens the title).

use legaia_engine_session::title_continue::CardPorts;
use legaia_engine_session::{ContinueScreen, ContinueStep};
use legaia_engine_ui::pause_menu::stage_transform;
use legaia_engine_ui::{SpriteDraw, TextDraw};
use wasm_bindgen::prelude::*;

use crate::play_menu::quad_json;
use crate::runtime::LegaiaRuntime;

impl LegaiaRuntime {
    /// The rack's two ports as the session's Continue screen reads them.
    pub(crate) fn card_ports(&self) -> CardPorts<'_> {
        [self.cards[0].as_ref(), self.cards[1].as_ref()]
    }

    /// Open the title's Continue screen over the card rack. `false` when the
    /// menu font / chrome cannot be built (no disc), which leaves the title
    /// to come back up.
    pub(crate) fn open_boot_continue(&mut self) -> bool {
        if !self.ensure_menu_assets() {
            return false;
        }
        self.boot_continue = Some(ContinueScreen::open(self.card_ports()));
        true
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Whether the title's Continue screen owns the frame.
    pub fn boot_continue_is_open(&self) -> bool {
        self.boot_continue.is_some()
    }

    /// Step the title's Continue screen one frame on an edge-triggered PSX
    /// pad word. Returns `""` while it runs, `"loaded"` once a save was read
    /// off a card (parked for [`Self::play_resume_save`]; the screen is
    /// closed), or `"back"` when the player backed out (the screen is
    /// closed and the page reopens the title). The screen's menu blip fires
    /// here, off the edge its refusal notice and block grid let through.
    pub fn boot_continue_step(&mut self, edge: u16) -> String {
        let ports = [self.cards[0].as_ref(), self.cards[1].as_ref()];
        let Some(screen) = self.boot_continue.as_mut() else {
            return String::new();
        };
        let step = screen.step(edge, ports);
        if let Some(blip) = step.blip() {
            let _ = self.play_menu_blip(blip);
        }
        match step {
            ContinueStep::Running { .. } => String::new(),
            ContinueStep::BackedOut { .. } => {
                self.boot_continue = None;
                "back".to_string()
            }
            ContinueStep::Loaded { load, .. } => {
                self.boot_continue = None;
                crate::console_log(&format!(
                    "continue: loaded card {} block {} (scene '{}')",
                    load.port + 1,
                    load.block,
                    load.scene
                ));
                self.park_loaded_save(load.save, &load.scene);
                "loaded".to_string()
            }
        }
    }

    /// The Continue screen's draw lists in surface pixels, the same shape as
    /// [`Self::play_menu_draws_json`] (`open`, `sprites` over the chrome
    /// atlas, `texts` over the font atlas, `darken`). The title art behind
    /// it is [`Self::boot_title_backdrop_draws_json`].
    pub fn boot_continue_draws_json(&self, surface_w: u32, surface_h: u32) -> String {
        let (Some(screen), Some(assets)) = (self.boot_continue.as_ref(), self.menu_assets.as_ref())
        else {
            return r#"{"open":false,"sprites":[],"texts":[]}"#.to_string();
        };
        let (origin, scale) = stage_transform(surface_w.max(1), surface_h.max(1));
        let mut sprites: Vec<SpriteDraw> = Vec::new();
        let mut texts: Vec<TextDraw> = Vec::new();
        let darken = self.build_save_select(
            assets,
            screen.session(),
            screen.flow(),
            &mut sprites,
            &mut texts,
            origin,
            scale,
        );
        serde_json::json!({
            "open": true,
            "sprites": sprites.iter().map(quad_json).collect::<Vec<_>>(),
            "texts": texts.iter().map(quad_json).collect::<Vec<_>>(),
            "darken": darken.map(|(at, level)| serde_json::json!({"at": at, "level": level})),
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The title's Continue is retail's title card manager, not a pause row:
    /// it opens the Load screen where the pause menu itself is refused (a
    /// dialogue owns the player), and no pause menu opens under it.
    #[test]
    fn title_continue_opens_where_the_pause_menu_would_not() {
        let mut rt = LegaiaRuntime::new();
        let host = legaia_engine_scene_host::scene_host::SceneHost::from_prot_bytes(
            crate::play_fmv::tests::synthetic_prot(),
            None,
        )
        .expect("synthetic host");
        rt.scene_host.install(host, None).expect("session");
        rt.scene_host.host_mut().expect("host").world.dialog.current =
            Some(legaia_engine_field::dialog_state::DialogRequest {
                text_id: 0,
                inline: vec![0x1F, b'x', 0x00],
                world_x: 0,
                world_z: 0,
                depth_id: 0,
            });
        assert!(
            !rt.play_menu_open_row("Load"),
            "the pause menu's Load row is refused under a dialogue"
        );
        assert!(!rt.play_menu_is_open());
        assert!(rt.open_boot_continue(), "Continue opens anyway");
        assert!(rt.boot_continue_is_open());
        assert!(!rt.play_menu_is_open(), "no pause menu stands under it");
        let screen = rt.boot_continue.as_ref().expect("screen");
        assert_eq!(
            screen.session().mode(),
            legaia_engine_menus::save_select::SaveSelectMode::Load,
            "the Load screen"
        );
        // Circle on the pill row backs out and blips the cancel.
        let cues = rt.sfx.menu_cue_requests;
        assert_eq!(
            rt.boot_continue_step(legaia_engine_system::input::PadButton::Circle.mask()),
            "back"
        );
        assert!(!rt.boot_continue_is_open());
        assert_eq!(rt.sfx.menu_cue_requests, cues + 1, "the back-out blips");
    }
}
