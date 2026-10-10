//! [`AudioSink`]: the control surface every audio output shares.
//!
//! The native [`crate::AudioOut`] (a cpal stream) and the browser's
//! [`crate::WebAudioOut`] (a `ScriptProcessorNode`) both drive one
//! [`StreamResampler`] mix core from their device callback and differ only in
//! how they lock it - a `Mutex` shared with the real-time thread, or a
//! `RefCell` on the page's single thread. Everything a host asks of an output
//! (sequencer attach / swap / pause, XA streams, mute, the SPU itself) is a
//! method on the core, so it is written once here as a provided method over
//! [`AudioSink::with_core`], and a director generic over `AudioSink` runs
//! unchanged on either host.

use anyhow::Result;

use crate::spu::Spu;
use crate::{SequencerProgress, StreamResampler, XaPlayback, sequencer::Sequencer};

/// The config-file volume level (`0..=10`) whose bus gain is unity: the
/// default, which leaves the retail mix bit-identical.
pub const VOLUME_LEVEL_UNITY: u8 = 8;

/// Q1.14 bus gain for a `0..=10` volume level: linear, `level / 8`, so 0 is
/// silent, [`VOLUME_LEVEL_UNITY`] is unity (`0x4000`) and 10 is a 1.25x boost
/// (the mix saturates rather than wraps). Levels above 10 clamp to 10.
pub const fn volume_level_gain(level: u8) -> u16 {
    let level = if level > 10 { 10 } else { level } as u32;
    (level * crate::spu::BUS_GAIN_UNITY as u32 / VOLUME_LEVEL_UNITY as u32) as u16
}

/// An audio output: anything that owns a [`StreamResampler`] and feeds a
/// device from it. Implementors provide [`Self::with_core`]; every control
/// method is provided on top of it.
pub trait AudioSink {
    /// Run `f` with exclusive access to the mix core, under whatever lock the
    /// output shares with its device callback.
    fn with_core<R>(&self, f: impl FnOnce(&mut StreamResampler) -> R) -> R;

    /// Toggle monaural output (the retail options screen's "Sound: Stereo /
    /// Monaural" row): each SPU voice sounds at the larger of its two
    /// volumes on both sides, as libsnd's `SsSetMono` does ([`crate::Spu::mono`]),
    /// and the final mix (XA included) is averaged into both channels.
    fn set_mono(&self, mono: bool) {
        self.with_core(|s| {
            s.mono = mono;
            s.spu.mono = mono;
        });
    }

    /// Master mute gate. Every producer (sequencer, SPU voices, XA stream,
    /// fade engine) keeps ticking while muted - only the rendered frames are
    /// zeroed - so unmuting resumes in sync, mid-track.
    fn set_muted(&self, muted: bool) {
        self.with_core(|s| s.muted = muted);
    }

    /// The engine-only BGM / SFX bus volumes (the config file's `bgm_volume`
    /// / `sfx_volume`, each `0..=10`): every sequencer voice is scaled by the
    /// BGM level, every cue voice by the SFX level ([`Spu::bus_gain`] via
    /// [`volume_level_gain`]). The default level 8 is unity - the retail mix.
    /// XA streams (voice / FMV audio) ride neither bus; mute covers them.
    fn set_bus_volumes(&self, bgm_level: u8, sfx_level: u8) {
        self.with_core(|s| {
            s.spu.bus_gain = [volume_level_gain(bgm_level), volume_level_gain(sfx_level)];
        });
    }

    /// Current state of the master mute gate.
    fn is_muted(&self) -> bool {
        self.with_core(|s| s.muted)
    }

    /// Run a closure with mutable access to the SPU model - how the engine
    /// pushes voice attributes, key-on/off masks and sample uploads.
    fn with_spu<R>(&self, f: impl FnOnce(&mut Spu) -> R) -> R {
        self.with_core(|s| f(&mut s.spu))
    }

    /// Install a streaming XA-ADPCM voice, replacing any active stream (and
    /// any queued shout) without crossfading. The device callback mixes it
    /// into the SPU output at 44.1 kHz; a one-shot stream detaches when its
    /// cursor runs off the end (`looping = true` for BGM).
    ///
    /// `gain` is Q1.14 like SPU voice volumes - `0x4000` is unity.
    fn play_xa(
        &self,
        pcm: Vec<i16>,
        sample_rate: u32,
        channels: legaia_xa::Channels,
        looping: bool,
        gain: u16,
    ) {
        self.with_core(|s| {
            s.pending_xa = None;
            s.xa = Some(XaPlayback {
                pcm,
                sample_rate,
                channels,
                looping,
                gain,
                cursor: 0.0,
                start_delay: 0,
            });
        });
    }

    /// Install an arts-voice battle shout with the retail CD/XA scheduling
    /// contract rather than the FMV one-shot one.
    ///
    /// * **Response-presentation delay.** `start_delay_frames` (44.1 kHz SPU
    ///   samples) holds the shout silent before its first audible sample,
    ///   the CD controller's seek/read-to-first-sector latency, so a shout
    ///   requested on the animation-start frame trails the animation.
    /// * **Back-to-back no-drop.** A shout requested while one is sounding is
    ///   queued (one deep) and starts the sample the active one ends; a third
    ///   request replaces the queued one.
    fn play_xa_shout(
        &self,
        pcm: Vec<i16>,
        sample_rate: u32,
        channels: legaia_xa::Channels,
        gain: u16,
        start_delay_frames: u32,
    ) {
        let shout = XaPlayback {
            pcm,
            sample_rate,
            channels,
            looping: false,
            gain,
            cursor: 0.0,
            start_delay: start_delay_frames,
        };
        self.with_core(|s| crate::stage_xa_shout(s, shout));
    }

    /// Decode a buffer of raw XA-ADPCM sound-group bytes (128-byte aligned)
    /// through [`legaia_xa::StreamingDecoder`] and stage the PCM as an XA
    /// stream ([`Self::play_xa`]).
    fn play_xa_streaming(
        &self,
        raw_bytes: &[u8],
        sample_rate: u32,
        channels: legaia_xa::Channels,
        looping: bool,
        gain: u16,
    ) -> Result<()> {
        let mut decoder = legaia_xa::StreamingDecoder::new(legaia_xa::DecodeOptions {
            channels,
            sample_rate,
            bits: legaia_xa::BitsPerSample::Four,
        });
        let mut pcm = Vec::with_capacity(raw_bytes.len() / 128 * 224);
        decoder.feed(raw_bytes, &mut pcm)?;
        // Trailing partial group bytes are dropped; XA is whole-group aligned.
        self.play_xa(pcm, sample_rate, channels, looping, gain);
        Ok(())
    }

    /// Detach the active XA stream and drop any queued shout.
    fn stop_xa(&self) {
        self.with_core(|s| {
            s.xa = None;
            s.pending_xa = None;
        });
    }

    /// `true` while an XA stream is attached and not yet exhausted.
    fn xa_active(&self) -> bool {
        self.with_core(|s| s.xa.as_ref().is_some_and(|x| !x.is_done()))
    }

    /// Playback position of the active XA stream in seconds (`None` with no
    /// stream). The cursor advances inside the device callback at the
    /// device's true rate, so this is a hardware-paced clock a video player
    /// can lock its frame advance to; it pins at the stream duration once a
    /// one-shot runs off the end.
    fn xa_cursor_secs(&self) -> Option<f64> {
        self.with_core(|s| {
            s.xa.as_ref().map(|x| {
                if x.sample_rate == 0 {
                    0.0
                } else {
                    x.cursor / x.sample_rate as f64
                }
            })
        })
    }

    /// Install a sequencer immediately, ticked once per SPU sample. The prior
    /// sequencer's notes are silenced (use [`Self::crossfade_to`] for a
    /// smooth transition).
    fn attach_sequencer(&self, seq: Sequencer) {
        self.with_core(|s| s.attach_sequencer(seq));
    }

    /// Detach the active sequencer (if any), key off whatever it had
    /// sounding, and cancel any in-progress crossfade.
    fn detach_sequencer(&self) {
        self.with_core(|s| s.detach_sequencer());
    }

    /// Gate the sequencer tick without detaching it. `true` stops the clock
    /// and keys off its notes, as retail's paused-slot service does
    /// (`FUN_800638D8`); `false` resumes from the playhead.
    fn set_sequencer_paused(&self, paused: bool) {
        self.with_core(|s| s.set_sequencer_paused(paused));
    }

    /// Rewind the attached sequencer to its first event (no-op with none).
    /// Op-`0x35` sub-op `4` replays the slot's sequence from its start
    /// (`FUN_80026478` -> `FUN_80062880(id, 1, 1)` -> `FUN_800628F0`), so a
    /// director's resume rewinds before it reopens the gate.
    fn rewind_sequencer(&self) {
        self.with_core(|s| s.rewind_sequencer());
    }

    /// Whether the sequencer gate is closed ([`Self::set_sequencer_paused`]).
    /// Every BGM director keys its pause state on this one bit, so a movie
    /// that ducks the score and the op-`0x35` pause arms share one latch.
    fn sequencer_paused(&self) -> bool {
        self.with_core(|s| s.sequencer_paused)
    }

    /// Set the attached sequencer's master volume (`SsSeqSetVol`-shaped,
    /// `0..=127`) in place, without restarting it. The battle audio duck
    /// rides this: retail ramps `_DAT_8007B910` and re-applies it through
    /// `FUN_800267A8` -> `FUN_80062004` each frame. A pending cross-fade
    /// target inherits it when it installs.
    fn set_sequencer_master_vol(&self, vol: u8) {
        self.with_core(|s| {
            if let Some(seq) = s.sequencer.as_mut() {
                seq.set_master_vol(vol);
            }
            if let Some(seq) = s.pending_seq.as_mut() {
                seq.set_master_vol(vol);
            }
        });
    }

    /// Cross-fade from the current sequencer to `new_seq` over
    /// `fade_samples` SPU-rate (44.1 kHz) samples: the old one fades out,
    /// then `new_seq` swaps in and fades up. With no sequencer playing, or
    /// `fade_samples = 0`, `new_seq` installs immediately at full volume.
    fn crossfade_to(&self, new_seq: Sequencer, fade_samples: u32) {
        self.with_core(|s| s.crossfade_to(new_seq, fade_samples));
    }

    /// Swap the active sequencer for `new_seq` **immediately**, faithful to
    /// retail's hard-cut BGM changes: the outgoing track is keyed off (its
    /// notes release through their envelopes) and `new_seq` sounds from its
    /// first event on the next SPU sample - unlike [`Self::crossfade_to`],
    /// whose serial fade holds the incoming intro silent.
    ///
    /// `fade_in_samples` is a short click-guard ramp (a couple of frames at
    /// most) that only rises; `0` is a true hard cut.
    fn swap_bgm(&self, new_seq: Sequencer, fade_in_samples: u32) {
        self.with_core(|s| s.swap_bgm(new_seq, fade_in_samples));
    }

    /// Snapshot of the attached sequencer's progress, `None` with none
    /// attached. A duplicate-start guard keys on this: a re-emitted start for
    /// the track already sounding keeps its playhead, but the same id after
    /// the track was detached must restart it.
    fn sequencer_progress(&self) -> Option<SequencerProgress> {
        self.with_core(|s| s.sequencer_progress())
    }
}
