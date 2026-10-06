//! The play page's BGM: the field VM's op-`0x35` events route into the
//! page's [`crate::play_sfx::PageDirector`] - the native window's
//! `AudioBgmDirector` (`crates/engine-session/src/bgm.rs`), one
//! implementation of `legaia_engine_core::scene::BgmDirector` for both hosts,
//! so a start / pause / stop lands the same way on each. What stays here is
//! the page's own hand-off timing: the title -> load hand-off and the start
//! owed to an audio output that arrived late.

use crate::runtime::LegaiaRuntime;
use legaia_engine_core::scene::BgmDirector;
use wasm_bindgen::prelude::*;

/// The title -> load hand-off.
///
/// The native window runs `bgm.stop()` followed by
/// `BootSession::restore_field_bgm()` once a save-select Load commits -
/// **after** it has entered the save's scene and loaded the save over it
/// (`enter_field_live_from_save`). The title theme has to let go of the
/// score, and the loaded save's own op-`0x35` track
/// (`World::audio.current_bgm`) has to come back, because the field VM will
/// not re-emit a start for music that was already playing when the save was
/// written. Retail's load route releases the theme the same way
/// (`FUN_800266E0` + `FUN_80026520` on the BGM slot at `0x801DFB74` in the
/// title tick's `LaunchFade` arm) before master mode 2 brings the field up.
///
/// The page asks for the hand-off *before* it enters the scene (it learns the
/// scene from the same poll), and running it there read the pre-load world's
/// track, then had the scene entry clear the dedupe latch under it - so the
/// scene's own start of the same track restarted it from the top. The call
/// therefore only arms the hand-off; the page's scene entry performs it after
/// the save lands (`run_pending_bgm_handoff`), and a tick that finds it still
/// armed - the page declined the entry - performs it there.
///
/// Returns whether the hand-off is armed (audio is up).
#[wasm_bindgen]
impl LegaiaRuntime {
    pub fn play_bgm_title_handoff(&mut self) -> bool {
        self.bgm_handoff_pending = self.audio_director().is_some();
        self.bgm_handoff_pending
    }
}

impl LegaiaRuntime {
    /// Perform an armed title -> load hand-off ([`Self::play_bgm_title_handoff`]):
    /// stop the running score, then start the loaded world's own track through
    /// the page's director. A no-op when nothing is armed. With no track in
    /// the save the stop still ran - silence, not a stale theme, which is the
    /// native behaviour too.
    pub(crate) fn run_pending_bgm_handoff(&mut self) {
        if !std::mem::take(&mut self.bgm_handoff_pending) {
            return;
        }
        if let Some(d) = self.audio_director() {
            d.stop();
        }
        self.start_world_bgm();
    }

    /// Start the world's last-routed op-`0x35` track
    /// (`World::audio.current_bgm`) through the page's director - a
    /// scene-local id plays retail's fallback track
    /// ([`legaia_engine_core::scene::bgm_bank_id`]), as the native
    /// `BootSession::restore_field_bgm` plays it. Returns whether it started.
    pub(crate) fn start_world_bgm(&mut self) -> bool {
        let Some(host) = self.scene_host.as_ref() else {
            return false;
        };
        let Some(id) = host.world.audio.current_bgm else {
            return false;
        };
        let Ok(Some(entry)) =
            host.music_bank_entry_bytes(legaia_engine_core::scene::bgm_bank_id(id))
        else {
            return false;
        };
        let Some(d) = self.audio_director() else {
            return false;
        };
        d.start_owned_vab(id, &entry);
        d.last_started == Some(id)
    }

    /// Bring the scene's own track up on an audio output that arrived late.
    ///
    /// The page's `WebAudioOut` exists only after a user gesture, and every
    /// op-`0x35` start routed before it was drained and dropped with no
    /// director to hear it - so the scene played silent until its script
    /// happened to start music again, a whole town visit. The native window
    /// opens its device with the session and never has the gap. Starts
    /// `World::audio.current_bgm` (the last start the field VM routed) when
    /// no track is sounding yet; the title screen scores itself
    /// (`play_title_bgm`), so a session still on it is left alone.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn start_current_bgm_on_late_audio(&mut self) {
        if self.boot_title.is_some() {
            return;
        }
        if self.audio_director().is_none_or(|d| d.is_attached()) {
            return;
        }
        self.start_world_bgm();
    }
}
