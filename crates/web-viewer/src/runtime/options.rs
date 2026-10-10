//! Persisted options: the `localStorage` round trip and the live side
//! effects of the runtime's `OptionsState`.

use super::*;

/// `localStorage` key the play page's options live under - the browser twin
/// of the native window's `OPTIONS_CONFIG_FILE`.
///
/// The two hosts persist through the *same* serde impl on
/// [`legaia_engine_core::options::OptionsState`]; only the byte store
/// differs, because a browser has no filesystem. Before this the browser had
/// no store at all: the Sound row was decorative and every setting was lost
/// on reload, which reads as "the options screen does nothing" rather than as
/// a missing capability.
pub const OPTIONS_STORAGE_KEY: &str = "legaia.options";

/// Read the persisted options, falling back to [`Default`] when nothing is
/// stored, the store is unavailable (private mode, non-browser target) or the
/// stored JSON no longer parses. Mirrors `OptionsState::load_or_default`.
pub(super) fn load_persisted_options() -> legaia_engine_core::options::OptionsState {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(raw) = options_storage().and_then(|s| s.get_item(OPTIONS_STORAGE_KEY).ok())
            && let Some(raw) = raw
            && let Ok(state) = serde_json::from_str(&raw)
        {
            return state;
        }
    }
    legaia_engine_core::options::OptionsState::default()
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
        #[cfg(target_arch = "wasm32")]
        if let Some(store) = options_storage()
            && let Ok(json) = serde_json::to_string(&self.options_state)
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
    /// screen's Stereo / Monaural row, the engine-only master mute and the
    /// engine-only `bgm_volume` / `sfx_volume` bus levels - plus the
    /// simulation knobs.
    pub(crate) fn apply_options_side_effects(&mut self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(audio) = self.audio_out.as_ref() {
            audio.set_mono(matches!(
                self.options_state.audio,
                legaia_engine_core::options::AudioMode::Mono
            ));
            audio.set_muted(self.options_state.muted);
            audio.set_bus_volumes(self.options_state.bgm_volume, self.options_state.sfx_volume);
        }
        if let Some(host) = self.scene_host.host_mut() {
            // The simulation knobs (precise movement, Field Move default,
            // reduce flashing, battle Select Attack) through the one push the
            // native window re-asserts each tick.
            self.options_state.apply_to_world(&mut host.world);
            if let Some(on) = self.precise_movement_override {
                host.world.locomotion.precise_movement = on;
            }
        }
        // The follow-camera distance preset, the same host knob the native
        // window re-asserts each tick (`window/event_handler/redraw.rs`).
        self.scene_host.camera_mut().distance = self.options_state.camera_distance;
    }
}
