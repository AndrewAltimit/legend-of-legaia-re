//! The play page's host hotkeys: the native window's option / cheat / camera
//! keys, resolved through the one shared table
//! ([`legaia_engine_session::hotkeys`]).
//!
//! The page used to bind `F3` alone, in JavaScript, so `I`, `F8`, `F9`, `F4`
//! and the rest - the keys the docs name for the enhancement toggles - did
//! nothing in the browser. [`LegaiaRuntime::play_host_hotkey`] takes the raw
//! `KeyboardEvent.code`, refuses what the native window refuses under a
//! front-end screen, runs the shared options edit itself, and hands back the
//! hotkey's id for the part only the page holds: a checkbox to mirror, its
//! HUD bar, its debug vantage.

use crate::runtime::LegaiaRuntime;
use legaia_engine_session::hotkeys::{
    FrontEndScreens, HOST_HOTKEYS, HostHotkey, apply_options_hotkey,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Resolve and apply one host hotkey press. Returns the hotkey's id
    /// (`HostHotkey::id`), or `""` when `code` is no hotkey or the hotkey is
    /// refused while a front-end screen owns the frame (`HostHotkey::refused`,
    /// the rule the native window asks). An options hotkey has already been applied
    /// and persisted when this returns; the page mirrors the stored value into
    /// its controls.
    pub fn play_host_hotkey(&mut self, code: &str) -> String {
        let Some(hotkey) = HostHotkey::from_dom_code(code) else {
            return String::new();
        };
        let screens = FrontEndScreens {
            boot_logos: self.boot_logos_is_active(),
            title: self.boot_title_is_active(),
            // The page's Continue opens the pause menu's Load row: its save
            // select is `pause_menu`.
            save_select: false,
            pause_menu: self.play_menu_is_open(),
            game_over: self.game_over.is_some(),
        };
        if hotkey.refused(screens) {
            return String::new();
        }
        if apply_options_hotkey(&mut self.options_state, hotkey) {
            self.persist_and_apply_options();
        }
        hotkey.id().to_string()
    }

    /// The hotkey table as JSON (`[{"id", "code"}]`), so the page labels its
    /// controls with the keys the engine binds rather than a second list.
    pub fn host_hotkeys_json(&self) -> String {
        host_hotkeys_json()
    }
}

/// [`HOST_HOTKEYS`] as `[{"id", "code"}]`.
pub fn host_hotkeys_json() -> String {
    let rows: Vec<serde_json::Value> = HOST_HOTKEYS
        .iter()
        .map(|(hk, code)| serde_json::json!({ "id": hk.id(), "code": code }))
        .collect();
    serde_json::Value::Array(rows).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every hotkey has an action on the page. The JavaScript keys its action
    /// table by [`HostHotkey::id`]; a hotkey with no entry there would be
    /// consumed (its default prevented) and do nothing, which is the gap this
    /// module closes.
    #[test]
    fn every_hotkey_has_a_page_action() {
        let page = include_str!("../../../site/_content/play.html");
        let view = include_str!("../../../site/js/play-view-input.js");
        for (hk, _) in HOST_HOTKEYS {
            let id = hk.id();
            let named = page.contains(&format!("{id}:")) || view.contains(&format!("'{id}'"));
            assert!(named, "no page action for hotkey '{id}'");
        }
    }

    #[test]
    fn json_lists_every_hotkey() {
        let v: serde_json::Value = serde_json::from_str(&host_hotkeys_json()).unwrap();
        assert_eq!(v.as_array().unwrap().len(), HOST_HOTKEYS.len());
    }
}
