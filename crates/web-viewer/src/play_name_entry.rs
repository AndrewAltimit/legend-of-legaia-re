//! Browser **name-entry overlay** - the opening `town01` lead-character naming
//! prompt, the last screen of the New Game flow before free-roam play.
//!
//! Nothing here re-implements the screen. The state machine is
//! [`legaia_engine_core::name_entry`] (opened by the field VM's pinned
//! op-`0x49` while the establishing-sweep timeline runs, stepped through
//! [`World::step_name_entry`](legaia_engine_core::world::World::step_name_entry))
//! and the geometry is the shared `legaia-engine-ui` pair
//! [`ui::name_entry_draws_for`] (text) + [`ui::name_entry_chrome_sprite_draws_for`]
//! (the two filigree windows + hand cursor). This module is only the bridge:
//! pad edges in, draw-list JSON out - the browser twin of the native window's
//! `name_entry_draws` / `name_entry_chrome_sprite_draws`.
//!
//! **The screen lands with its state.** The op-`0x49` gate reports `Armed`
//! while `name_entry_active()` holds and flips to `Done` when the SM commits,
//! so the suspended field script resumes on its own - and the committed name
//! is written into the live party record (`World::party_names[slot]`), which
//! is what the status panel and every later dialogue read. It is not a
//! cosmetic overlay: skipping the commit would park the opening timeline
//! forever, the same failure mode an unclosed field shop produces.
//!
//! REF: FUN_801E6B34 (renderer), FUN_801F03F0 (state machine)

use super::*;
use crate::runtime::LegaiaRuntime;
use legaia_engine_core::name_entry::{
    CHAR_CELLS, Control, GRID, GRID_COLS, NameEntry, NameEntryInput, NameEntryState,
};
use legaia_engine_ui::{self as ui, NameEntryView};

/// Project the engine's [`NameEntry`] onto the renderer-agnostic view the
/// shared builders consume. Mirrors the native window's `name_entry_view`;
/// `engine-ui` deliberately does not depend on `engine-core`, so each host
/// owns this few-line projection rather than the screen itself.
fn name_entry_view<'a>(entry: &'a NameEntry, frame: u64) -> NameEntryView<'a> {
    let (grid_cursor, control_cursor) = if entry.cursor < CHAR_CELLS {
        (
            Some((entry.cursor / GRID_COLS, entry.cursor % GRID_COLS)),
            None,
        )
    } else {
        let idx = match entry.control_at(entry.cursor) {
            Some(Control::Backspace) => Some(0),
            Some(Control::Default) => Some(1),
            Some(Control::End) => Some(2),
            None => None,
        };
        (None, idx)
    };
    NameEntryView {
        grid_rows: &GRID,
        name: &entry.name,
        default_name: &entry.default_name,
        grid_cursor,
        control_cursor,
        confirming: entry.state == NameEntryState::Confirm,
        confirm_yes: entry.confirm_yes,
        // Retail blinks the caret at 75% duty off the frame counter's
        // `& 0x18` bits.
        caret_on: (frame & 0x18) != 0,
    }
}

/// Test-only probes for the disc-gated New Game oracle
/// (`tests/new_game_flow_parity.rs`). Native-only so the wasm export surface
/// the page consumes stays exactly the player-facing API.
#[cfg(not(target_arch = "wasm32"))]
impl LegaiaRuntime {
    /// Enter `town01` **as the new-game opening** - the state the prologue
    /// hand-off leaves behind, which is what makes `enter_field_scene`
    /// install the establishing timeline whose op-`0x49` opens name entry. A
    /// plain `enter_field("town01")` is a casual visit and never does.
    pub fn debug_enter_town01_opening(&mut self) -> Result<(), String> {
        let host = self.scene_host.as_mut().ok_or("no disc loaded")?;
        host.world.entering_town01_opening = true;
        self.enter_field(legaia_asset::new_game::OPENING_SCENE)
            .map(|_| ())
            .map_err(|e| format!("{e:?}"))
    }

    /// Drop the title art so the menu-row fallback path can be exercised (the
    /// browser equivalent of a PROT.DAT-only load, where PROT 0888 is absent).
    pub fn debug_drop_title_atlas(&mut self) {
        self.title_atlas = None;
    }

    /// The live world frame counter - the "did the field resume" observable.
    pub fn debug_world_frame(&self) -> u64 {
        self.scene_host.as_ref().map(|h| h.world.frame).unwrap_or(0)
    }

    /// `true` while the opening cutscene timeline still owns the scene. It
    /// must eventually clear, or the player never gets the controls back.
    pub fn debug_timeline_active(&self) -> bool {
        self.scene_host
            .as_ref()
            .is_some_and(|h| h.world.cutscene_timeline_active())
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// `true` while the name-entry overlay is up. The page freezes the field
    /// and routes every pad edge into [`Self::name_entry_input`] while this
    /// holds - the overlay is modal, exactly as it is natively.
    pub fn name_entry_is_active(&self) -> bool {
        self.scene_host
            .as_ref()
            .is_some_and(|h| h.world.name_entry_active())
    }

    /// Step the overlay one frame from an edge-triggered PSX pad word (same
    /// bit layout as [`Self::set_pad`]). Cross confirms the cell under the
    /// cursor (or the Yes/No row); Triangle is the backspace shortcut while
    /// editing and cancels the confirm prompt.
    ///
    /// Returns `true` on the frame the name commits - at which point the
    /// entry closes, the name is in the party record, and the op-`0x49` gate
    /// releases the suspended opening script on its next step.
    ///
    /// The world frame counter is advanced here (and only here) while the
    /// overlay is up, because the field tick is frozen under it and the
    /// caret blink is derived from that counter.
    pub fn name_entry_input(&mut self, edge: u16) -> bool {
        let Some(h) = self.scene_host.as_mut() else {
            return false;
        };
        if !h.world.name_entry_active() {
            return false;
        }
        let input = NameEntryInput {
            up: edge & 0x0010 != 0,
            down: edge & 0x0040 != 0,
            left: edge & 0x0080 != 0,
            right: edge & 0x0020 != 0,
            confirm: edge & 0x4000 != 0, // Cross
            cancel: edge & 0x1000 != 0,  // Triangle
        };
        let committed = h.world.step_name_entry(input);
        h.world.frame = h.world.frame.wrapping_add(1);
        committed
    }

    /// Live overlay state for the page's status line (and headless checks):
    /// ```text
    /// { "open": true, "name": "Vahn", "default": "Vahn", "cursor": 116,
    ///   "control": 2,        // 0 = BS, 1 = restore default, 2 = Select
    ///   "glyph": "A"|null,   // glyph under a grid cursor
    ///   "confirming": false, "confirm_yes": false }
    /// ```
    /// `{"open":false}` when no entry is up. A read-only projection of the
    /// engine SM - the page never writes name state, it only reports it.
    pub fn name_entry_state_json(&self) -> String {
        let Some(entry) = self
            .scene_host
            .as_ref()
            .and_then(|h| h.world.name_entry.as_ref())
        else {
            return r#"{"open":false}"#.to_string();
        };
        let control = entry.control_at(entry.cursor).map(|c| match c {
            Control::Backspace => 0,
            Control::Default => 1,
            Control::End => 2,
        });
        serde_json::json!({
            "open": true,
            "name": entry.name,
            "default": entry.default_name,
            "cursor": entry.cursor,
            "control": control,
            "glyph": entry.glyph_at(entry.cursor).map(|g| g.to_string()),
            "confirming": entry.state == NameEntryState::Confirm,
            "confirm_yes": entry.confirm_yes,
        })
        .to_string()
    }

    /// The committed display name for a party slot - the name-entry result
    /// once confirmed, else the disc's new-game template default. The page
    /// shows it in the HUD so the naming is visibly *in the save*, not just
    /// on a screen that came and went.
    pub fn party_display_name(&self, slot: usize) -> String {
        self.scene_host
            .as_ref()
            .map(|h| h.world.party_name(slot).to_string())
            .unwrap_or_default()
    }

    /// Draw lists for the overlay, in surface pixels:
    /// `{ "open": bool, "sprites": [...menu-chrome quads...],
    ///    "texts": [...dialog-font quads...] }` - the same two-layer shape
    /// the pause menu / shop / dialog use, blitted by the page's chrome and
    /// font atlas blitters.
    ///
    /// Both layers come from the shared `engine-ui` builders at the
    /// retail-traced stage geometry, then through the common stage transform
    /// so text and chrome stay locked together.
    pub fn name_entry_draws_json(&mut self, surface_w: u32, surface_h: u32) -> String {
        const CLOSED: &str = r#"{"open":false,"sprites":[],"texts":[]}"#;
        if !self.name_entry_is_active() {
            return CLOSED.to_string();
        }
        // The overlay can be the FIRST screen of a session (the opening
        // timeline opens it before the player ever pauses), so the shared
        // menu chrome + font may not have been built yet.
        if !self.ensure_menu_assets() {
            return CLOSED.to_string();
        }
        let Some(h) = self.scene_host.as_ref() else {
            return CLOSED.to_string();
        };
        let Some(entry) = h.world.name_entry.as_ref() else {
            return CLOSED.to_string();
        };
        let Some(assets) = self.menu_assets.as_ref() else {
            return CLOSED.to_string();
        };
        let view = name_entry_view(entry, h.world.frame);
        let (origin, scale) = crate::play_menu::stage_transform(surface_w, surface_h);

        let mut texts = ui::name_entry_draws_for(assets.font_ref(), &view);
        ui::scale_stage_text_draws(&mut texts, origin, scale);

        let sprites = match assets.chrome_rects() {
            Some(rects) => ui::name_entry_chrome_sprite_draws_for(rects, &view, origin, scale),
            None => Vec::new(),
        };

        serde_json::json!({
            "open": true,
            "sprites": sprites.iter().map(crate::play_menu::quad_json).collect::<Vec<_>>(),
            "texts": texts.iter().map(crate::play_menu::quad_json).collect::<Vec<_>>(),
        })
        .to_string()
    }
}
