//! The field submode screens' line lists with their strings read off the
//! user's disc: the code lock (slot `0x21`), the flag window (slot `0x23`)
//! and the coin counter. Each screen's state and line builder is the
//! `World`'s (`legaia_engine_core::field_submode_code_lock`,
//! `field_submode_flag_window`, `field_submode_screen`); the host adds the
//! field-overlay strings (extraction entry `0897`) it alone can read.

use super::SceneHost;
use legaia_engine_core::field_submode_screen::{SubmodeLine, coin_counter_lines};
use legaia_engine_core::incense_notice::FIELD_OVERLAY_PROT_INDEX;
use legaia_engine_field::field_overlay::field_overlay_cstr;
use legaia_engine_field::field_submode_code_lock::{CODE_LOCK_HEADER_VA, CODE_LOCK_SLOT};
use legaia_engine_field::field_submode_flag_window::{
    FLAG_WINDOW_LEGEND_VAS, FLAG_WINDOW_RECORD, FlagWindowLine,
};
use legaia_engine_field::submode_hub_actors::slot;

impl SceneHost {
    /// [`legaia_engine_core::world::World::code_lock_lines`] with the header read off the
    /// user's disc (the field overlay, extraction entry `0897`). The native
    /// window and the browser play page both draw this.
    pub fn code_lock_lines(&self) -> Vec<FlagWindowLine> {
        let s = &self.world.field_vm.submode_screen;
        if !s.is_open() || s.actor.state != CODE_LOCK_SLOT {
            return Vec::new();
        }
        let header = self
            .index
            .entry_bytes(FIELD_OVERLAY_PROT_INDEX)
            .ok()
            .and_then(|b| field_overlay_cstr(&b, CODE_LOCK_HEADER_VA).map(<[u8]>::to_vec))
            .unwrap_or_default();
        self.world.code_lock_lines(&header)
    }
}

impl SceneHost {
    /// [`legaia_engine_core::world::World::flag_window_lines`] with the legend read off the
    /// user's disc (the field overlay, extraction entry `0897`). The native
    /// window and the browser play page both draw this.
    pub fn flag_window_lines(&self) -> Vec<FlagWindowLine> {
        let s = &self.world.field_vm.submode_screen;
        if !s.is_open() || !s.installed_windows.contains(&FLAG_WINDOW_RECORD) {
            return Vec::new();
        }
        let bytes = self.index.entry_bytes(FIELD_OVERLAY_PROT_INDEX).ok();
        let read = |va: u32| -> Vec<u8> {
            bytes
                .as_deref()
                .and_then(|b| field_overlay_cstr(b, va))
                .map(<[u8]>::to_vec)
                .unwrap_or_default()
        };
        let (a, b) = (
            read(FLAG_WINDOW_LEGEND_VAS[0]),
            read(FLAG_WINDOW_LEGEND_VAS[1]),
        );
        self.world.flag_window_lines([&a, &b])
    }
}

impl SceneHost {
    /// [`legaia_engine_core::field_submode_screen::coin_counter_lines`] with the labels read off the user's disc (the
    /// field overlay, extraction entry `0897`). The native window and the
    /// browser play page both draw this, through
    /// `legaia_engine_ui::text_lines::pen_line_draws_for`.
    pub fn coin_counter_lines(&self) -> Vec<SubmodeLine> {
        let screen = &self.world.field_vm.submode_screen;
        if !screen.is_open() || screen.actor.state != slot::COIN_COUNTER {
            return Vec::new();
        }
        let bytes = self.index.entry_bytes(FIELD_OVERLAY_PROT_INDEX).ok();
        coin_counter_lines(screen, |va| {
            bytes
                .as_deref()
                .and_then(|b| field_overlay_cstr(b, va))
                .map(<[u8]>::to_vec)
                .unwrap_or_default()
        })
    }
}
