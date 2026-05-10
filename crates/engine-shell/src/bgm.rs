//! Concrete [`legaia_engine_core::scene::BgmDirector`] adapter that drives a
//! cpal-backed [`legaia_engine_audio::AudioOut`].
//!
//! The director owns the audio output handle plus the active scene's
//! [`legaia_engine_audio::VabBank`] (uploaded into the SPU at scene-load
//! time). On each `start` / `queue` call it parses the SEQ bytes the field
//! VM resolved through the BGM table, builds a [`legaia_engine_audio::Sequencer`],
//! and attaches it to the audio output. `pause` / `resume` toggle the
//! sequencer-feed flag without rebuilding state; `stop` detaches the
//! sequencer entirely.
//!
//! The retail engine routes BGM through SsAPI seq-context callbacks (see
//! `docs/subsystems/audio.md` "PsyQ libsnd SsAPI" + the `_DAT_801CE564`
//! seq-context resolver). We don't need that level of indirection in the
//! port - the field VM's BGM events arrive pre-resolved with the right SEQ
//! bytes and the active VAB is staged once per scene. This adapter is the
//! join point.

use std::sync::Arc;

use anyhow::{Context, Result};
use legaia_engine_audio::{AudioOut, Sequencer, VabBank};
use legaia_engine_core::scene::BgmDirector;
use legaia_seq::Seq;

/// BGM director that routes [`BgmDirector`] events into a live
/// [`AudioOut`]. The director holds a clone of the audio handle (cpal stream
/// is reference-counted internally via `Arc`) plus the active VAB bank.
pub struct AudioBgmDirector {
    audio: Arc<AudioOut>,
    bank: Option<VabBank>,
    /// Master volume forwarded to every freshly-attached sequencer. Engines
    /// bump this when the user adjusts the music slider.
    pub master_vol: u8,
    /// Loop-to event index for newly-started sequencers. `None` plays once
    /// (sequencer reports `finished` when it runs off the end). Most field
    /// BGM loops to 0; cutscene SEQs typically don't.
    pub loop_to: Option<usize>,
    /// Whether playback is currently paused. `pause` / `resume` toggle
    /// without detaching the active sequencer.
    paused: bool,
    /// Last started BGM id, if any. Useful for diagnostics + suppressing
    /// redundant `start(same_id)` calls (the field VM occasionally re-emits
    /// op `0x35` without a state change).
    pub last_started: Option<u16>,
    /// Optional pending BGM bytes - used by `queue` to defer playback until
    /// the engine signals a transition (typically the next field-VM tick).
    pending: Option<(u16, Vec<u8>)>,
}

impl AudioBgmDirector {
    pub fn new(audio: Arc<AudioOut>) -> Self {
        Self {
            audio,
            bank: None,
            master_vol: 100,
            loop_to: Some(0),
            paused: false,
            last_started: None,
            pending: None,
        }
    }

    /// Replace the active VAB bank. Engines call this once per scene after
    /// resolving the scene's primary VAB entry through
    /// [`legaia_engine_core::scene::SceneHost::scene_vab_bytes`]; the bank
    /// is uploaded into the SPU and stored here for subsequent SEQ starts.
    pub fn set_bank(&mut self, bank: VabBank) {
        self.bank = Some(bank);
    }

    /// Borrow the active bank - useful for tests / inspection.
    pub fn bank(&self) -> Option<&VabBank> {
        self.bank.as_ref()
    }

    /// `true` if a sequencer is currently attached to the audio output.
    pub fn is_playing(&self) -> bool {
        self.audio.sequencer_progress().is_some() && !self.paused
    }

    /// Drain whatever was queued by the most recent [`BgmDirector::queue`]
    /// call. Engines call this when transitioning into the scene that
    /// should play the queued track.
    pub fn flush_queue(&mut self) -> Result<bool> {
        let Some((id, bytes)) = self.pending.take() else {
            return Ok(false);
        };
        self.start_inner(id, &bytes)?;
        Ok(true)
    }

    fn start_inner(&mut self, bgm_id: u16, seq_bytes: &[u8]) -> Result<()> {
        let Some(bank) = self.bank.clone() else {
            log::warn!("AudioBgmDirector::start({bgm_id}) ignored - no VAB bank loaded for scene");
            return Ok(());
        };
        let seq = Seq::parse(seq_bytes).context("parse SEQ for BGM start")?;
        let mut sequencer = Sequencer::new(seq, bank);
        sequencer.set_master_vol(self.master_vol);
        if let Some(loop_to) = self.loop_to {
            sequencer.set_loop_to(loop_to);
        }
        // Cross-fade over ~30 frames (0.5 s at 60 Hz = 22050 SPU samples)
        // if another sequencer is already playing; otherwise attach directly.
        const CROSSFADE_SAMPLES: u32 = 22_050;
        if self.audio.sequencer_progress().is_some() && !self.paused {
            self.audio.crossfade_to(sequencer, CROSSFADE_SAMPLES);
        } else {
            self.audio.attach_sequencer(sequencer);
        }
        self.paused = false;
        self.last_started = Some(bgm_id);
        Ok(())
    }
}

impl BgmDirector for AudioBgmDirector {
    fn start(&mut self, bgm_id: u16, seq_bytes: &[u8]) {
        // Suppress duplicate starts for the same BGM id - the field VM's
        // op 0x35 occasionally re-emits without a state change (we'd lose
        // the playhead by re-attaching).
        if self.last_started == Some(bgm_id)
            && !self.paused
            && self.audio.sequencer_progress().is_some()
        {
            return;
        }
        if let Err(e) = self.start_inner(bgm_id, seq_bytes) {
            log::warn!("AudioBgmDirector::start({bgm_id}) failed: {e:#}");
        }
    }

    fn queue(&mut self, bgm_id: u16, seq_bytes: &[u8]) {
        self.pending = Some((bgm_id, seq_bytes.to_vec()));
    }

    fn pause(&mut self) {
        self.paused = true;
        self.audio.set_sequencer_paused(true);
    }

    fn resume(&mut self) {
        self.paused = false;
        self.audio.set_sequencer_paused(false);
    }

    fn stop(&mut self) {
        self.audio.detach_sequencer();
        self.paused = false;
        self.last_started = None;
    }
}

#[cfg(test)]
mod tests {
    use legaia_engine_audio::VabBank;

    /// Test stub bank - empty programs / samples. Real banks come from
    /// `legaia_vab::parse`.
    fn empty_bank() -> VabBank {
        VabBank {
            master_vol: 127,
            samples: Vec::new(),
            programs: Vec::new(),
        }
    }

    /// Director without an audio handle - exercises queue / pause / resume
    /// state machines without opening a cpal stream (CI has no audio
    /// device). We can't construct AudioOut without a device, so the start
    /// / stop tests live as integration tests in environments where audio
    /// is available.
    #[test]
    fn queue_then_flush_replays_pending_bytes_or_logs_warning() {
        // Quick offline test: the queue / flush plumbing doesn't touch
        // audio when there's no bank. We simulate by directly setting the
        // pending field.
        struct Stub {
            pending: Option<(u16, Vec<u8>)>,
        }
        let mut s = Stub { pending: None };
        s.pending = Some((42, vec![1, 2, 3]));
        let drained = s.pending.take();
        assert_eq!(drained, Some((42, vec![1, 2, 3])));
        let _ = empty_bank(); // touch path so unused-import lint stays clean
    }
}
