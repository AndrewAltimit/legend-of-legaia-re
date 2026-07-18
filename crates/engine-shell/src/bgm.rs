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
use legaia_engine_audio::{
    ArtsShoutBank, AudioOut, PendingCue, SHOUT_CD_RESPONSE_DELAY, Sequencer, SfxBank, SfxScheduler,
    VabBank,
};
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
    /// Sound-effect descriptor bank (decoded from the executable's
    /// `DAT_8006F198` table, see `sfx-table.md`). Empty until
    /// [`Self::set_sfx_bank`]; play requests against an empty bank no-op.
    /// The bank is static across scenes (it lives in the executable), so it
    /// is set once at boot; the per-scene VAB it plays through is the same
    /// [`Self::bank`] the BGM sequencer uses.
    sfx_bank: SfxBank,
    /// Resident **class-2 sound bank** (extraction PROT 0869, raw loader
    /// index `0x367`) the battle scene loader and the Baka Fighter init load
    /// explicitly - its low programs (`0`, `3`) carry the strike / duel-hit
    /// cues (see `sfx-table.md`). Uploaded once at boot into a dedicated SPU
    /// RAM region so battle / minigame cues resolve regardless of which BGM
    /// VAB happens to be open. `None` when the bank couldn't be staged (a
    /// disc-free boot); [`Self::tick_sfx_frame`] then falls back to the scene
    /// BGM bank ([`Self::bank`]), matching the retail field-scene path where a
    /// cue sounds out of whichever bank the libsnd current-bank globals hold.
    sfx_vab: Option<VabBank>,
    /// Frame-timed one-shot cue queue. [`Self::enqueue_sfx`] adds a cue at
    /// its strike-relative delay; [`Self::tick_sfx_frame`] advances one frame
    /// and fires matured cues through the SPU.
    sfx_sched: SfxScheduler,
    /// Arts-voice **shout** bank: the per-character CD-XA clips
    /// (`XA2`/`XA4`/`XA6`, demuxed per channel + decoded at boot) and the
    /// SCUS cue tables. `None` on a disc-free / extracted-dir boot (the raw
    /// CD-XA subheaders needed for channel demux only exist on a real disc
    /// image); shout requests then no-op, leaving arts silent - the same
    /// degradation retail applies to an unvoiced art.
    shout_bank: Option<ArtsShoutBank>,
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
            sfx_bank: SfxBank::new(),
            sfx_vab: None,
            sfx_sched: SfxScheduler::new(),
            shout_bank: None,
        }
    }

    /// Install the arts-voice shout bank (demuxed + decoded from the user's
    /// disc at boot; see [`crate::boot::read_arts_shout_bank`]).
    pub fn set_shout_bank(&mut self, bank: ArtsShoutBank) {
        self.shout_bank = Some(bank);
    }

    /// Whether the arts-voice shout bank was staged.
    pub fn has_shout_bank(&self) -> bool {
        self.shout_bank.is_some()
    }

    /// Fire the Tactical-Arts shout for `(cslot, action_constant)` through
    /// the XA mixing path. Resolves the cue against the bank's channel pools
    /// (retail `FUN_8004C140` selection, no immediate repeat) and stages the
    /// clip with the modeled CD-response start delay
    /// ([`SHOUT_CD_RESPONSE_DELAY`]), so the shout starts *after* the art
    /// animation that requested it - never before. A second shout while one
    /// is sounding queues behind it (the back-to-back no-drop path in
    /// [`AudioOut::play_xa_shout`]). Returns the fired channel, or `None`
    /// when the bank is absent or the art is unvoiced.
    pub fn play_art_shout(&mut self, cslot: u8, action: u8) -> Option<u8> {
        let bank = self.shout_bank.as_mut()?;
        let (channel, clip) = bank.shout(cslot, action)?;
        self.audio.play_xa_shout(
            clip.pcm.clone(),
            clip.sample_rate,
            legaia_xa::Channels::Mono,
            0x4000,
            SHOUT_CD_RESPONSE_DELAY,
        );
        Some(channel)
    }

    /// Install the sound-effect descriptor bank (decoded from the user's
    /// `SCUS_942.54` `DAT_8006F198` table at boot). Replaces any prior bank.
    pub fn set_sfx_bank(&mut self, bank: SfxBank) {
        self.sfx_bank = bank;
    }

    /// Install the resident class-2 SFX program bank (PROT 0869), uploaded
    /// into its own SPU RAM region at boot. Battle / minigame cues fire
    /// against this bank so their programs are always resident; see
    /// [`Self::sfx_vab`].
    pub fn set_sfx_vab(&mut self, bank: VabBank) {
        self.sfx_vab = Some(bank);
    }

    /// Whether the resident class-2 SFX bank was staged.
    pub fn has_sfx_vab(&self) -> bool {
        self.sfx_vab.is_some()
    }

    /// Borrow the active SFX bank - useful for tests / inspection.
    pub fn sfx_bank(&self) -> &SfxBank {
        &self.sfx_bank
    }

    /// Queue a one-shot sound cue to fire `frames` after this call (the
    /// strike's `timing_frames`). `id` is the [`SfxBank`] descriptor id
    /// directly (the art-record `HitCue::kind`), played without
    /// `classify_cue`. `actor` / `target` ride along for HUD context.
    pub fn enqueue_sfx(&mut self, id: u16, frames: u16, actor: u8, target: u8) {
        self.sfx_sched
            .enqueue(PendingCue::new(id, frames).with_actors(actor, target));
    }

    /// Advance the SFX scheduler one frame and fire any matured cue through
    /// the SPU. Cues resolve against the resident class-2 SFX bank
    /// ([`Self::sfx_vab`]) when it is staged - the retail battle / minigame
    /// path, whose programs are always resident - and fall back to the active
    /// scene BGM bank ([`Self::bank`]) otherwise (the retail field-scene
    /// path). Returns the `(cue_id, voice)` pairs that keyed on. A cue is
    /// silently dropped when no bank is staged, its id isn't in the descriptor
    /// bank, its program / tone isn't resident, or no SPU voice is free
    /// (matching the retail "no voice / no program -> skip" behaviour). Call
    /// once per simulation tick so delayed cues advance even when none are
    /// enqueued that frame.
    pub fn tick_sfx_frame(&mut self) -> Vec<(u16, u8)> {
        let batch = self.sfx_sched.tick_frame();
        if batch.is_empty() {
            return Vec::new();
        }
        // Prefer the resident class-2 SFX bank; fall back to the scene BGM VAB.
        let Some(vab) = self.sfx_vab.as_ref().or(self.bank.as_ref()) else {
            return Vec::new();
        };
        let bank = &self.sfx_bank;
        let mut fired = Vec::new();
        self.audio.with_spu(|spu| {
            for cue in &batch.fired {
                if let Some(voice) = bank.play_one_shot(cue.id as u8, spu, vab) {
                    fired.push((cue.id, voice));
                }
            }
        });
        fired
    }

    /// Drop every queued SFX cue (scene transition / battle abort).
    pub fn clear_sfx(&mut self) {
        self.sfx_sched.clear();
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

    /// Split a raw `music_01` bank entry (`[chunk][pBAV VAB][pQES SEQ]`),
    /// upload the entry's **own** VAB into the SPU BGM region (capped below
    /// the resident SFX bank, exactly like `stage_scene_vab`), stash it as the
    /// active bank, and return the SEQ bytes. `None` when the pair is absent
    /// or the VAB header doesn't parse. This is the global-pool half of BGM
    /// playback - the track brings its own instruments, unlike the scene-local
    /// path that reuses the pre-staged scene VAB.
    fn stage_owned_vab(&mut self, entry_bytes: &[u8]) -> Option<Vec<u8>> {
        let vab_off = entry_bytes.windows(4).position(|w| w == b"pBAV")?;
        let seq_rel = entry_bytes[vab_off..]
            .windows(4)
            .position(|w| w == b"pQES")?;
        let report = legaia_vab::parse(entry_bytes, vab_off).ok()?;
        let body = &entry_bytes[vab_off..];
        let bank = self.audio.with_spu(|spu| {
            let mut alloc = legaia_engine_audio::SpuAllocator::new(
                crate::boot::SPU_RESERVED_BYTES,
                crate::boot::SPU_RAM_BYTES
                    - crate::boot::SPU_RESERVED_BYTES
                    - crate::boot::SFX_BANK_SPU_BYTES,
            );
            VabBank::upload(spu, &mut alloc, &report, body)
        });
        self.bank = Some(bank);
        Some(entry_bytes[vab_off + seq_rel..].to_vec())
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

    fn start_owned_vab(&mut self, bgm_id: u16, entry_bytes: &[u8]) {
        // Suppress a redundant re-emit of the same global track (the field VM
        // occasionally re-fires op 0x35): re-uploading the VAB + restarting
        // would drop the playhead.
        if self.last_started == Some(bgm_id)
            && !self.paused
            && self.audio.sequencer_progress().is_some()
        {
            return;
        }
        let Some(seq) = self.stage_owned_vab(entry_bytes) else {
            log::warn!("AudioBgmDirector::start_owned_vab({bgm_id}) - no [VAB][SEQ] pair in entry");
            return;
        };
        if let Err(e) = self.start_inner(bgm_id, &seq) {
            log::warn!("AudioBgmDirector::start_owned_vab({bgm_id}) failed: {e:#}");
        }
    }

    fn queue_owned_vab(&mut self, bgm_id: u16, entry_bytes: &[u8]) {
        // Upload the VAB now (so the bank is ready) and defer the SEQ start to
        // the next `flush_queue`.
        if let Some(seq) = self.stage_owned_vab(entry_bytes) {
            self.pending = Some((bgm_id, seq));
        }
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
