//! The expansion game modes on the play page: the mode list for the page's
//! picker and the entry / exit calls.
//!
//! The seat itself is the engine's (`World::open_pack_mode_picker`,
//! `World::enter_pack_mode`, ticked under `SceneMode::Expansion` by the
//! world tick off the pad word the page already routes), and so is its
//! screen: the panels come out of `screen_layers::compose_screen_prims` and
//! the labels out of `play_overlay_draws_json`, both through
//! `legaia_engine_screens::expansion_modes` - the composition the native
//! window draws. This module only lets the page's UI panel enter it, the
//! twin of the native window's backquote key and `--mode` flag.

use crate::runtime::LegaiaRuntime;
use legaia_engine_mode::mode::SceneMode;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// The mode instances the seat offers right now (the active pack's, or
    /// the built-in practice set) as
    /// `{"open": bool, "modes": [{"id", "name", "kind"}]}`; `open` is whether
    /// the seat owns the frame. `{"open": false, "modes": []}` before a
    /// scene is loaded.
    pub fn play_pack_modes_json(&self) -> String {
        let Some(host) = self.scene_host.host() else {
            return r#"{"open":false,"modes":[]}"#.to_string();
        };
        let modes: Vec<_> = host
            .world
            .pack_mode_entries()
            .into_iter()
            .map(|e| serde_json::json!({ "id": e.id, "name": e.name, "kind": e.kind.label() }))
            .collect();
        serde_json::json!({
            "open": host.world.mode == SceneMode::Expansion,
            "modes": modes,
        })
        .to_string()
    }

    /// Open the in-game mode picker over the running scene. `false` before
    /// a scene is loaded, or outside the field and the overworld.
    pub fn play_pack_mode_picker(&mut self) -> bool {
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        if !matches!(host.world.mode, SceneMode::Field | SceneMode::WorldMap) {
            return false;
        }
        host.world.open_pack_mode_picker();
        true
    }

    /// Enter mode instance `id` straight away. Returns `""` on success, else
    /// why it could not start.
    pub fn play_pack_mode_enter(&mut self, id: &str) -> String {
        let Some(host) = self.scene_host.host_mut() else {
            return "Load a scene first.".to_string();
        };
        if !matches!(
            host.world.mode,
            SceneMode::Field | SceneMode::WorldMap | SceneMode::Expansion
        ) {
            return "Modes open from the field or the overworld.".to_string();
        }
        match host.world.enter_pack_mode(id) {
            Ok(()) => String::new(),
            Err(e) => e.to_string(),
        }
    }

    /// Leave the mode seat (a session still in play is forfeited).
    pub fn play_pack_mode_exit(&mut self) {
        if let Some(host) = self.scene_host.host_mut() {
            host.world.exit_pack_mode();
        }
    }
}
