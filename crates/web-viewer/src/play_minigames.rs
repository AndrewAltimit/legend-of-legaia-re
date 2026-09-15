//! In-world minigames on the play page: the draw + input side of the
//! sessions the shared scene host installs.
//!
//! Entry is already host-agnostic: the field-VM op-`0x3E` `op0 >= 100` arm
//! and the world-map `MinigameDoor` walk-on publish `World::minigames
//! .pending_warp`, and `SceneHost::tick` drains it into
//! `enter_fishing / enter_slot / enter_baka / enter_muscle / enter_dance`
//! on both hosts. Fishing has its own module ([`crate::play_fishing`]); this
//! one is the surface for the other four `SceneMode`s, which the page used
//! to enter with a frozen field and no UI.

use legaia_engine_ui::{SpriteDraw, TextDraw};

use crate::runtime::LegaiaRuntime;

/// Presentation state for the in-world minigame screens.
#[derive(Default)]
pub(crate) struct MinigameUi {}

impl LegaiaRuntime {
    /// Per-tick presentation step. Cheap no-op outside a minigame mode.
    pub(crate) fn tick_minigame_ui(&mut self) {
        let _ = &mut self.minigame_ui;
    }

    /// Overlay quads (surface pixels) for the active in-world minigame,
    /// appended to `play_overlay_draws_json`'s lists. Empty outside one.
    /// Read-only by design (the composite holds the menu assets borrowed);
    /// any per-frame state moves in [`Self::tick_minigame_ui`].
    pub(crate) fn minigame_overlay_draws(
        &self,
        _font: &legaia_font::Font,
        _surface_w: u32,
        _surface_h: u32,
    ) -> (Vec<SpriteDraw>, Vec<TextDraw>) {
        (Vec::new(), Vec::new())
    }
}
