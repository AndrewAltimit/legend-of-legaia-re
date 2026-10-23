//! Persisted options: the `localStorage` round trip and the live side
//! effects of the runtime's `OptionsState`.

use super::*;

/// `localStorage` key the play page's options live under - the browser twin
/// of the native window's `OPTIONS_CONFIG_FILE`.
///
/// The two hosts persist through the *same* serde impl on
/// [`legaia_engine_menus::options::OptionsState`]; only the byte store
/// differs, because a browser has no filesystem. Before this the browser had
/// no store at all: the Sound row was decorative and every setting was lost
/// on reload, which reads as "the options screen does nothing" rather than as
/// a missing capability.
pub const OPTIONS_STORAGE_KEY: &str = "legaia.options";

/// Read the persisted options, falling back to [`Default`] when nothing is
/// stored, the store is unavailable (private mode, non-browser target) or the
/// stored JSON no longer parses. Mirrors `OptionsState::load_or_default`.
pub(crate) fn load_persisted_options() -> legaia_engine_menus::options::OptionsState {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(raw) = options_storage().and_then(|s| s.get_item(OPTIONS_STORAGE_KEY).ok())
            && let Some(raw) = raw
            && let Ok(state) = serde_json::from_str(&raw)
        {
            return state;
        }
    }
    legaia_engine_menus::options::OptionsState::default()
}

/// The page's `localStorage`, when this target has one and the browser lets
/// us reach it (it throws in private mode on some engines).
#[cfg(target_arch = "wasm32")]
fn options_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

impl LegaiaRuntime {
    /// Apply the live side effects of [`Self::options_state`] and persist it -
    /// the browser twin of the native window's `persist_and_apply_options`,
    /// called from the same place (an Options sub-session closing).
    pub(crate) fn persist_and_apply_options(&mut self) {
        self.apply_options_side_effects();
        // Retail-faithful mode (`crate::play_toggles`) pins the engine-only
        // knobs for the session only: the store gets them as they were before
        // the pin, and every other row with this session's edits
        // (`OptionsPins::persistable`, the native window's rule). The store
        // used to skip the write outright, dropping volumes, mute, time of day
        // and the retail rows edited while the mode held.
        #[cfg(target_arch = "wasm32")]
        if let Some(store) = options_storage()
            && let Ok(json) =
                serde_json::to_string(&self.options_pins.persistable(&self.options_state))
            && store.set_item(OPTIONS_STORAGE_KEY, &json).is_err()
        {
            crate::console_log("options: localStorage write failed");
        }
    }

    /// Push the current options into their live consumers without touching
    /// the store (also called once when audio comes up, so a persisted
    /// Monaural / muted state applies to a fresh `AudioContext`).
    ///
    /// The same audio knobs the native window applies - the retail options
    /// screen's Stereo / Monaural row, the engine-only master mute, the
    /// engine-only `bgm_volume` / `sfx_volume` bus levels and the engine-only
    /// `downmix_mono` full downmix - plus the
    /// simulation knobs.
    pub(crate) fn apply_options_side_effects(&mut self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(audio) = self.audio_out.as_ref() {
            audio.set_mono(matches!(
                self.options_state.audio,
                legaia_engine_system::option_values::AudioMode::Mono
            ));
            audio.set_muted(self.options_state.muted);
            audio.set_bus_volumes(self.options_state.bgm_volume, self.options_state.sfx_volume);
            audio.set_downmix(self.options_state.downmix_mono);
        }
        // The simulation knobs (precise movement, Field Move default, reduce
        // flashing, the battle options, the tile windows, the ground fog) and
        // the follow-camera distance preset, through the session step both
        // hosts also re-assert on every tick (`BootSession::apply_option_knobs`).
        self.apply_option_knobs();
    }

    /// [`legaia_engine_session::BootSession::apply_option_knobs`] over the
    /// page's options and its VR precise-movement override; before a disc is
    /// loaded only the slot's camera exists to take the distance preset.
    pub(crate) fn apply_option_knobs(&mut self) {
        match self.scene_host.session_mut() {
            Some(session) => {
                session.apply_option_knobs(&self.options_state, self.precise_movement_override)
            }
            None => self.scene_host.camera_mut().distance = self.options_state.camera_distance,
        }
    }
}
