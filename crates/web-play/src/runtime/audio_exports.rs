//! WebAudio lifecycle exports: init, resume, title BGM, gain and readiness.

use super::*;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// The title theme starts through the session's `start_title_bgm` on the
    /// page's headless director, twice in a row (the stop ahead of the start
    /// lets a re-opened title restart it).
    #[test]
    fn play_title_bgm_starts_the_title_theme_through_the_session() {
        let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
            return;
        };
        let Ok(bytes) = std::fs::read(&disc) else {
            eprintln!("[skip] disc unreadable (disc-gated)");
            return;
        };
        let mut rt = LegaiaRuntime::new();
        rt.load_disc(bytes, String::new()).expect("load_disc");
        assert!(rt.play_title_bgm(), "the title theme starts");
        assert!(rt.play_title_bgm(), "and starts again after its own stop");
        let id = legaia_engine_system::music_labels::TITLE_THEME_BGM_ID;
        assert_eq!(
            rt.scene_host.director().and_then(|d| d.last_started),
            Some(id)
        );
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Attempt to start the WebAudio backend. Must be called from a user-gesture
    /// handler (browser autoplay policy). `true` on success.
    ///
    /// Once up, the scene's BGM plays automatically: every [`Self::tick_frame`]
    /// routes the field VM's op-`0x35` music events through the same port-side
    /// VAB + SEQ + SPU path the audio audition page uses. This call also
    /// starts the track the scene's script last started - that start was
    /// routed before the output existed and so was dropped - and parks the
    /// default level ([`BGM_DEFAULT_GAIN`] slider units) on the output node
    /// so the level matches the page's slider. Browsers often open the
    /// `AudioContext` suspended even inside a gesture - call
    /// [`Self::audio_resume`] right after this to make it audible.
    pub fn audio_init(&mut self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            // Idempotent: a second output would replace the first, and the
            // page's whole audio state (director, staged banks, the playing
            // track) is built over the one it has.
            if self.audio_out.is_some() {
                return true;
            }
            match WebAudioOut::new() {
                Ok(out) => {
                    out.set_gain(BGM_DEFAULT_GAIN);
                    #[allow(clippy::arc_with_non_send_sync)]
                    let out = std::sync::Arc::new(out);
                    self.audio_out = Some(out);
                    // The director is built over the new output and staged
                    // at once.
                    self.scene_host.set_director(None);
                    let _ = self.audio_director();
                    // Every start routed before this was dropped with no
                    // director to hear it: bring the scene's track up now.
                    self.start_current_bgm_on_late_audio();
                    true
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("audio_init: {e}").into());
                    false
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        false
    }

    /// Resume the BGM `AudioContext`. Browsers construct it in `suspended`
    /// state even when the constructor runs inside a user gesture; the play
    /// page calls this from its gesture handler right after [`Self::audio_init`]
    /// to make the audio actually sound. Resolved no-op when audio isn't up.
    #[cfg(target_arch = "wasm32")]
    pub fn audio_resume(&self) -> js_sys::Promise {
        match self.audio_out.as_ref() {
            Some(out) => out.resume(),
            None => js_sys::Promise::resolve(&JsValue::UNDEFINED),
        }
    }

    /// Swap the score to the title theme - the browser twin of the native
    /// post-wipe `BootUiState::Title` hand-off: stop the running track,
    /// then start [`legaia_engine_system::music_labels::TITLE_THEME_BGM_ID`]
    /// as an owned-VAB global track. The page calls this when the
    /// party-wipe hold resolves to the title card, so the battle BGM never
    /// outlives the battle. `false` when audio is down or the disc entry
    /// doesn't resolve - the stop still ran, leaving silence rather than
    /// the stale track.
    ///
    /// The start is the session's (`BootSession::start_title_bgm`, the
    /// native window's call too); only the stop ahead of it is this page's.
    pub fn play_title_bgm(&mut self) -> bool {
        use legaia_engine_scene_host::scene_host::BgmDirector;
        let id = legaia_engine_system::music_labels::TITLE_THEME_BGM_ID;
        let Some(d) = self.audio_director() else {
            return false;
        };
        d.stop();
        let Some(session) = self.scene_host.session_mut() else {
            return false;
        };
        session.start_title_bgm() && session.bgm.as_ref().and_then(|d| d.last_started) == Some(id)
    }

    /// Set the BGM output gain in page-slider units: `1.0` is the page
    /// default ([`BGM_DEFAULT_GAIN`]); the slider spans 0x (mute) to 10x.
    /// No-op when audio isn't up.
    ///
    /// The site's browser master trim
    /// ([`legaia_engine_audio::webaudio::WEB_MASTER_TRIM`] - the same `0.25`
    /// `site/js/layout.js` publishes as `window.LEGAIA_MASTER_TRIM`) is
    /// applied by `WebAudioOut::set_gain` itself, so the slider value passes
    /// through untouched. This page used to multiply by its own copy of that
    /// factor first, applying the site trim TWICE and leaving the play page a
    /// factor of four - about 12 dB - under the minigames page, whose
    /// `site/js/minigame-bgm.js` output stage applies it once.
    #[cfg(target_arch = "wasm32")]
    pub fn audio_set_gain(&self, gain: f32) {
        if let Some(out) = self.audio_out.as_ref() {
            out.set_gain(gain);
        }
    }

    /// Whether the WebAudio backend is live (`audio_init` succeeded). The play
    /// page reads this to decide whether it still needs the user's audio-enable
    /// gesture.
    pub fn audio_ready(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            self.audio_out.is_some()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            false
        }
    }
}
