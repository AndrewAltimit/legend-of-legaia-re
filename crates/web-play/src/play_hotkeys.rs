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
    FrontEndScreens, HOST_HOTKEYS, HostHotkey, apply_options_hotkey, options_hotkey_state,
    refused_notice, toggle_notice,
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
        if hotkey.refused(self.hotkey_screens()) {
            return String::new();
        }
        if apply_options_hotkey(&mut self.options_state, hotkey) {
            self.persist_and_apply_options();
        }
        hotkey.id().to_string()
    }

    /// The notice for a hotkey press [`Self::play_host_hotkey`] refused
    /// (`"Camera distance: only while walking around"`), the line the native
    /// window shows instead of leaving the press looking dead. `""` when
    /// `code` is no hotkey or is not refused now.
    pub fn play_host_hotkey_refusal(&self, code: &str) -> String {
        match HostHotkey::from_dom_code(code) {
            Some(hk) if hk.refused(self.hotkey_screens()) => refused_notice(hk),
            _ => String::new(),
        }
    }

    /// The notice line after hotkey `id` was applied (`"Ground fog: off"`),
    /// worded by the shared `toggle_notice` the native window uses. An
    /// options hotkey's state is read off the stored options; the page holds
    /// the rest (its HUD bar, its debug vantage, the encounter box) and
    /// passes that state as `page_state`. `""` for an unknown id.
    pub fn play_host_hotkey_notice(&self, id: &str, page_state: &str) -> String {
        host_hotkey_notice(&self.options_state, id, page_state)
    }

    /// The hotkey table as JSON (`[{"id", "code", "key", "label"}]`), so the
    /// page lists and labels the keys the engine binds rather than a second
    /// list.
    pub fn host_hotkeys_json(&self) -> String {
        host_hotkeys_json()
    }
}

impl LegaiaRuntime {
    /// The front-end screens owning the page's frame, as the hotkey gate
    /// reads them.
    fn hotkey_screens(&self) -> FrontEndScreens {
        FrontEndScreens {
            boot_logos: self.boot_logos_is_active(),
            title: self.boot_title_is_active(),
            // The title's Continue screen (`crate::boot_continue`).
            save_select: self.boot_continue_is_open(),
            pause_menu: self.play_menu_is_open(),
            game_over: self.game_over.is_some(),
        }
    }
}

/// The notice for hotkey `id` after its press: the options state when the
/// options hold it, `page_state` otherwise, and `"done"` for the one-shot
/// AP cheat (the native window's wording for each).
pub fn host_hotkey_notice(
    opts: &legaia_engine_menus::options::OptionsState,
    id: &str,
    page_state: &str,
) -> String {
    let Some(hk) = HOST_HOTKEYS
        .iter()
        .map(|&(hk, _)| hk)
        .find(|hk| hk.id() == id)
    else {
        return String::new();
    };
    if hk == HostHotkey::CheatMaxAp {
        return format!("{}: done", hk.label());
    }
    let state = options_hotkey_state(opts, hk).unwrap_or_else(|| page_state.to_string());
    toggle_notice(hk, &state)
}

/// [`HOST_HOTKEYS`] as `[{"id", "code", "key", "label"}]`: `key` is the
/// printable key name (`F9`, `I`), `label` what the key changes.
pub fn host_hotkeys_json() -> String {
    let rows: Vec<serde_json::Value> = HOST_HOTKEYS
        .iter()
        .map(|(hk, code)| {
            serde_json::json!({
                "id": hk.id(),
                "code": code,
                "key": hk.key_label(),
                "label": hk.label(),
            })
        })
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
        let rows = v.as_array().unwrap();
        assert_eq!(rows.len(), HOST_HOTKEYS.len());
        let fog = rows.iter().find(|r| r["id"] == "volumetric_fog").unwrap();
        assert_eq!(fog["key"], "F9");
        assert_eq!(fog["label"], "Ground fog");
    }

    /// The page's notice line reads as the native window's: options state
    /// from the stored options, the page's own state otherwise, the AP
    /// cheat as a one-shot.
    #[test]
    fn notices_match_the_native_wording() {
        use legaia_engine_menus::options::OptionsState;
        let o = OptionsState {
            volumetric_fog: false,
            ..OptionsState::default()
        };
        assert_eq!(
            host_hotkey_notice(&o, "volumetric_fog", "ignored"),
            "Ground fog: off"
        );
        assert_eq!(host_hotkey_notice(&o, "diag_rows", "on"), "Diagnostics: on");
        assert_eq!(
            host_hotkey_notice(&o, "cheat_max_ap", ""),
            "Cheat: fill AP: done"
        );
        assert_eq!(host_hotkey_notice(&o, "no_such_key", "on"), "");
    }

    /// The page shows the notices: a page that never calls the exports
    /// leaves every press of a page-held key (and every refused key) silent,
    /// which is the drift the native window's notice line closed.
    #[test]
    fn the_page_shows_hotkey_notices() {
        let view = include_str!("../../../site/js/play-view-input.js");
        let page = include_str!("../../../site/_content/play.html");
        assert!(view.contains("play_host_hotkey_notice"));
        assert!(view.contains("play_host_hotkey_refusal"));
        assert!(page.contains("host_hotkeys_json"));
    }
}
