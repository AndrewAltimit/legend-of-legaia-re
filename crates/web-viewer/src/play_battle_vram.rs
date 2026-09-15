//! Mid-battle VRAM re-upload channel for the play page.
//!
//! The native window re-stamps the battle VRAM every battle tick
//! (`window/battle.rs`: `tick_battle_face_stamps`, `tick_battle_status_clut`,
//! `tick_battle_effect_clut`) and its renderer samples the live texture. The
//! page uploads its battle VRAM once per fight (`play_battle_generation`), so
//! those three stamps never reach the screen here. This module is the
//! channel that closes that: the drains run in
//! [`LegaiaRuntime::tick_battle_vram_channel`] and the page re-uploads the
//! battle VRAM when [`LegaiaRuntime::play_battle_vram_take_dirty`] reports a
//! change.

use wasm_bindgen::prelude::*;

use crate::runtime::LegaiaRuntime;

/// Per-fight state of the re-upload channel.
#[derive(Default)]
pub(crate) struct BattleVramChannel {
    /// Set when a re-stamp changed texels since the page last re-uploaded.
    pub(crate) dirty: bool,
}

impl LegaiaRuntime {
    /// Run the per-tick battle VRAM re-stamps. Cheap no-op outside battle.
    pub(crate) fn tick_battle_vram_channel(&mut self) {}
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// `true` once per change: the battle VRAM texels moved since the page
    /// last re-uploaded them (re-read `play_battle_vram_bytes`).
    pub fn play_battle_vram_take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.battle_vram.dirty)
    }
}
