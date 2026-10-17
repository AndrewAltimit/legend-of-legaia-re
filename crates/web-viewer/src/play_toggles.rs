//! The play page's twins of the native window's launch switches that were
//! native-only: `--retail`, `--set-flag`, `--no-entry-pulse`,
//! `LEGAIA_SMART_MONSTERS`, `LEGAIA_WORLD_MAP_MARKERS`,
//! `LEGAIA_BATTLE_FRAME_CADENCE` and `LEGAIA_ARTS_SAVED_LIST`.
//!
//! Each writes the same engine state the native launch writes - the world's
//! `WorldToggles`, the system-flag bank, `OptionsState::pin_retail` - so the
//! two hosts differ only in when the switch is thrown (the page at any time,
//! the window at launch). The world toggles live on the session's `World`,
//! which outlives a scene change; the page also keeps its own copy and
//! re-asserts it on every scene entry, so a switch thrown before a disc is
//! loaded holds once one is.

use crate::runtime::LegaiaRuntime;
use wasm_bindgen::prelude::*;

/// The page-side switches, re-asserted on every scene entry.
#[derive(Debug, Clone, Copy)]
pub struct PageToggles {
    /// `LEGAIA_SMART_MONSTERS`: a monster's single-target attack picks the
    /// lowest-HP living party member. Non-faithful, off by default.
    pub smart_monsters: bool,
    /// `LEGAIA_WORLD_MAP_MARKERS`: the port's per-placement overworld entity
    /// markers. A debug overlay retail does not draw; off by default.
    pub world_map_markers: bool,
    /// `--retail`: the persisted engine-only options pinned to their retail
    /// values for the session, and the scene-entry pulse off.
    pub retail: bool,
    /// The scene-entry VDF pulse enhancement; `false` is the native
    /// `--no-entry-pulse`. On by default, as on the window. Retail mode
    /// holds it off whatever this says.
    pub entry_pulse: bool,
    /// `LEGAIA_BATTLE_FRAME_CADENCE`: the battle action SM on retail's
    /// once-a-battle-frame cadence (`WorldToggles::battle_sm_frame_cadence`).
    /// Staged off on both hosts.
    pub battle_frame_cadence: bool,
    /// `LEGAIA_ARTS_SAVED_LIST`: the ring's Arts arm opens the legacy
    /// saved-chain list instead of retail's per-press input
    /// (`WorldToggles::arts_saved_list`). Off by default.
    pub arts_saved_list: bool,
}

impl Default for PageToggles {
    fn default() -> Self {
        Self {
            smart_monsters: false,
            world_map_markers: false,
            retail: false,
            entry_pulse: true,
            battle_frame_cadence: false,
            arts_saved_list: false,
        }
    }
}

impl LegaiaRuntime {
    /// Write the page's switches onto the live world. Called by each setter
    /// and from every scene entry.
    pub(crate) fn apply_page_toggles(&mut self) {
        let t = self.page_toggles;
        if let Some(host) = self.scene_host.host_mut() {
            let toggles = &mut host.world.toggles;
            toggles.smarter_monster_targeting = t.smart_monsters;
            toggles.overworld_marker_overlay = t.world_map_markers;
            toggles.entry_pulse_enabled = t.entry_pulse && !t.retail;
            toggles.battle_sm_frame_cadence = t.battle_frame_cadence;
            toggles.arts_saved_list = t.arts_saved_list && !t.retail;
        }
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Monsters target the lowest-HP party member (the native window's
    /// `LEGAIA_SMART_MONSTERS=1`). The RNG stream is unchanged.
    pub fn set_smart_monsters(&mut self, on: bool) {
        self.page_toggles.smart_monsters = on;
        self.apply_page_toggles();
    }

    /// Whether [`Self::set_smart_monsters`] is on.
    pub fn smart_monsters(&self) -> bool {
        self.page_toggles.smart_monsters
    }

    /// Draw the overworld's per-placement entity markers (the native
    /// window's `LEGAIA_WORLD_MAP_MARKERS=1`). The page's marker pass already
    /// reads the world toggle.
    pub fn set_world_map_markers(&mut self, on: bool) {
        self.page_toggles.world_map_markers = on;
        self.apply_page_toggles();
    }

    /// Whether [`Self::set_world_map_markers`] is on.
    pub fn world_map_markers(&self) -> bool {
        self.page_toggles.world_map_markers
    }

    /// The scene-entry VDF pulse enhancement (off is the native
    /// `--no-entry-pulse`). Takes effect at the next scene entry, where the
    /// installer runs; retail mode holds it off.
    pub fn set_entry_pulse(&mut self, on: bool) {
        self.page_toggles.entry_pulse = on;
        self.apply_page_toggles();
    }

    /// Whether [`Self::set_entry_pulse`] is on.
    pub fn entry_pulse(&self) -> bool {
        self.page_toggles.entry_pulse
    }

    /// Run the battle action SM once a battle frame, retail's cadence (the
    /// native `LEGAIA_BATTLE_FRAME_CADENCE=1`). Staged off on both hosts.
    pub fn set_battle_frame_cadence(&mut self, on: bool) {
        self.page_toggles.battle_frame_cadence = on;
        self.apply_page_toggles();
    }

    /// Whether [`Self::set_battle_frame_cadence`] is on.
    pub fn battle_frame_cadence(&self) -> bool {
        self.page_toggles.battle_frame_cadence
    }

    /// Open the legacy saved-chain list from the command ring's Arts arm
    /// instead of retail's per-press input (the native
    /// `LEGAIA_ARTS_SAVED_LIST=1`). Retail mode holds it off.
    pub fn set_arts_saved_list(&mut self, on: bool) {
        self.page_toggles.arts_saved_list = on;
        self.apply_page_toggles();
    }

    /// Whether [`Self::set_arts_saved_list`] is on.
    pub fn arts_saved_list(&self) -> bool {
        self.page_toggles.arts_saved_list
    }

    /// Retail-faithful mode, the native `--retail`: pin every persisted
    /// engine-only option to its retail value (`OptionsState::pin_retail`:
    /// retail camera distance, 8-way movement, no photosensitivity slew,
    /// retail tile windows, no fog, no enhanced lighting) and turn the
    /// scene-entry pulse off, for this session only - nothing is written to
    /// the store, including an option the player edits while the mode holds
    /// (the page's render boxes re-apply the pinned values on every scene
    /// rebuild, and one such write used to bake the pin into the store). Off
    /// restores the stored options. The page's own render
    /// switches (lighting, lamp shadows, fog, occlusion fade) are its
    /// checkboxes, which the page sets beside this call.
    pub fn set_retail_mode(&mut self, on: bool) {
        if self.page_toggles.retail == on {
            return;
        }
        self.page_toggles.retail = on;
        if on {
            self.options_state.pin_retail();
        } else {
            self.options_state = crate::runtime::load_persisted_options();
        }
        self.apply_options_side_effects();
        self.apply_page_toggles();
    }

    /// Whether [`Self::set_retail_mode`] is on.
    pub fn retail_mode(&self) -> bool {
        self.page_toggles.retail
    }

    /// The persisted engine-only options still off their retail value, as a
    /// JSON array of names (`OptionsState::retail_deviations`) - empty while
    /// retail mode holds.
    pub fn retail_deviations_json(&self) -> String {
        serde_json::json!(self.options_state.retail_deviations()).to_string()
    }

    /// Raise (`on`) or clear a system / story flag in the shared bank
    /// (`DAT_80085758`), the native `--set-flag`. Returns `false` with no
    /// scene loaded.
    pub fn set_system_flag(&mut self, idx: u16, on: bool) -> bool {
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        if on {
            host.world.system_flag_set(idx);
        } else {
            host.world.system_flag_clear(idx);
        }
        true
    }

    /// Read a system / story flag (`false` with no scene loaded).
    pub fn system_flag(&self, idx: u16) -> bool {
        self.scene_host
            .host()
            .is_some_and(|h| h.world.system_flag_test(idx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The switches hold before a disc is loaded and the retail pin is
    /// session-only: off restores the stored options.
    #[test]
    fn retail_mode_pins_the_options_and_restores_them() {
        let mut rt = LegaiaRuntime::new();
        rt.set_smart_monsters(true);
        rt.set_world_map_markers(true);
        assert!(rt.smart_monsters() && rt.world_map_markers());
        // The entry pulse starts on, as on the window; the two staged
        // switches start off, as a native session with an empty environment.
        assert!(rt.entry_pulse());
        assert!(!rt.battle_frame_cadence() && !rt.arts_saved_list());
        rt.set_entry_pulse(false);
        rt.set_battle_frame_cadence(true);
        rt.set_arts_saved_list(true);
        assert!(!rt.entry_pulse() && rt.battle_frame_cadence() && rt.arts_saved_list());
        let stored = rt.options_state.clone();
        rt.set_retail_mode(true);
        assert!(rt.retail_mode());
        assert_eq!(rt.retail_deviations_json(), "[]");
        rt.set_retail_mode(false);
        assert_eq!(
            rt.options_state.retail_deviations(),
            stored.retail_deviations()
        );
        // No scene: the flag write is refused, not dropped silently.
        assert!(!rt.set_system_flag(0x142, true));
        assert!(!rt.system_flag(0x142));
    }
}
