//! The native output stream: [`AudioOut`], one cpal stream driving the shared
//! [`StreamResampler`] mix core. The browser host's twin is
//! [`crate::WebAudioOut`]; both pull frames from the same core.

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::*;

trait Sample: cpal::Sample + Copy {
    fn from_i16(s: i16) -> Self;
}
impl Sample for f32 {
    fn from_i16(s: i16) -> f32 {
        s as f32 / i16::MAX as f32
    }
}
impl Sample for i16 {
    fn from_i16(s: i16) -> i16 {
        s
    }
}
impl Sample for u16 {
    fn from_i16(s: i16) -> u16 {
        ((s as i32) + 32_768) as u16
    }
}

/// Audio output handle. Owns the cpal stream + a thread-shared SPU model.
pub struct AudioOut {
    _stream: cpal::Stream,
    /// Shared SPU + resampler state. Locked once per cpal callback.
    pub(crate) state: Arc<Mutex<StreamResampler>>,
    pub device_rate: u32,
    pub channels: u16,
}

impl AudioOut {
    /// Open the default audio output device. Picks an f32/i16/u16 format
    /// supported by the device, defaulting to whatever the device prefers.
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("no default output device"))?;
        let config = device
            .default_output_config()
            .context("query default output config")?;
        let device_rate = config.sample_rate().0;
        let channels = config.channels();
        let state = Arc::new(Mutex::new(StreamResampler::new(device_rate)));

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                Self::build_stream::<f32>(&device, &config.into(), state.clone(), channels)?
            }
            cpal::SampleFormat::I16 => {
                Self::build_stream::<i16>(&device, &config.into(), state.clone(), channels)?
            }
            cpal::SampleFormat::U16 => {
                Self::build_stream::<u16>(&device, &config.into(), state.clone(), channels)?
            }
            other => return Err(anyhow!("unsupported sample format {:?}", other)),
        };
        stream.play().context("start audio stream")?;
        log::info!(
            "audio: device='{}' rate={} channels={}",
            device.name().unwrap_or_default(),
            device_rate,
            channels
        );
        Ok(Self {
            _stream: stream,
            state,
            device_rate,
            channels,
        })
    }

    fn build_stream<S>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        state: Arc<Mutex<StreamResampler>>,
        channels: u16,
    ) -> Result<cpal::Stream>
    where
        S: cpal::SizedSample + Sample,
    {
        let stream = device.build_output_stream::<S, _, _>(
            config,
            move |out: &mut [S], _: &cpal::OutputCallbackInfo| {
                // Recover from a poisoned lock rather than panicking on the
                // audio thread (matches `AudioOut::lock`); a poisoned guard
                // still yields a usable resampler, just with stale state.
                let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                let chans = channels as usize;
                let frames = out.len() / chans;
                for f in 0..frames {
                    let (mut l, mut r) = s.next_frame();
                    // Monaural downmix (options "Sound: Monaural").
                    if s.mono {
                        let m = ((l as i32 + r as i32) / 2) as i16;
                        l = m;
                        r = m;
                    }
                    // Mono device: average. Stereo+: feed L/R, dup any
                    // surround channels with the dominant side.
                    if chans == 1 {
                        let mono = ((l as i32 + r as i32) / 2) as i16;
                        out[f] = S::from_i16(mono);
                    } else {
                        out[f * chans] = S::from_i16(l);
                        out[f * chans + 1] = S::from_i16(r);
                        for c in 2..chans {
                            let pick = if c % 2 == 0 { l } else { r };
                            out[f * chans + c] = S::from_i16(pick);
                        }
                    }
                }
            },
            |err| log::error!("audio output error: {err}"),
            None,
        )?;
        Ok(stream)
    }

    /// Lock the shared state, recovering from poisoning instead of panicking.
    /// The `state` mutex is also held inside the real-time cpal callback, so a
    /// panic while it is held would poison it; recovering keeps a single fault
    /// from cascading into every subsequent lock. On the unpoisoned path this
    /// is identical to `self.state.lock().unwrap()`.
    fn lock(&self) -> std::sync::MutexGuard<'_, StreamResampler> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Toggle the monaural downmix (the retail options screen's
    /// "Sound: Stereo / Monaural" row). When on, the output callback
    /// averages L/R into both channels.
    pub fn set_mono(&self, mono: bool) {
        self.lock().mono = mono;
    }

    /// Master mute gate (engine-only). While muted the output stream keeps
    /// running and every producer (sequencer, SPU voices, XA stream, fade
    /// engine) keeps ticking - only the rendered frames are zeroed - so
    /// unmuting resumes playback exactly in sync, mid-track.
    pub fn set_muted(&self, muted: bool) {
        self.lock().muted = muted;
    }

    /// Current state of the master mute gate.
    pub fn is_muted(&self) -> bool {
        self.lock().muted
    }

    /// Run a closure with mutable access to the underlying SPU model. This
    /// is how the engine pushes voice attributes, key-on/off masks, and
    /// sample uploads. Locks for the duration of the closure.
    pub fn with_spu<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Spu) -> R,
    {
        let mut s = self.lock();
        f(&mut s.spu)
    }

    /// Convenience: play `pcm` (mono i16, at `input_rate`) as a one-shot.
    /// Synthesises a single SPU-ADPCM-shaped pseudo-block by injecting the
    /// PCM as a "raw" voice that bypasses the ADPCM stage.
    ///
    /// NOTE: this path is for the asset viewer's "preview decoded VAG WAV
    /// without re-encoding" use case. The full SPU mixer is the production
    /// path; see [`Self::with_spu`] for that.
    pub fn play_pcm_mono(&self, pcm: Vec<i16>, input_rate: u32) {
        let mut s = self.lock();
        // Use voice 0 as the dedicated preview slot. Park the PCM at a
        // fixed SPU-RAM region by re-encoding into ADPCM blocks first.
        let blocks = pcm_to_silence_padded_adpcm(&pcm);
        s.spu.ram.write_at(0x1000, &blocks);
        // Pitch: pcm is at `input_rate`, SPU plays at 44_100. step = input/44100.
        let pitch = ((input_rate as u64 * PITCH_UNITY as u64) / SPU_INTERNAL_RATE as u64)
            .min(0x3FFF) as u16;
        {
            let v = &mut s.spu.voices[0];
            v.start_addr = 0x1000;
            v.loop_addr = None;
            v.pitch = pitch.max(1);
            v.vol_left = 0x3FFF;
            v.vol_right = 0x3FFF;
            v.adsr_cfg = AdsrConfig::default();
        }
        // Split borrow: `voices` and `ram` are disjoint fields of `Spu`,
        // so referencing them via the same destructure lets the borrow
        // checker prove no aliasing.
        let spu::Spu {
            ref mut voices,
            ref ram,
            ..
        } = s.spu;
        voices[0].key_on(ram);
    }

    /// Stop the preview voice immediately (voice 0).
    pub fn stop(&self) {
        let mut s = self.lock();
        s.spu.voices[0].key_off();
    }

    /// Install a streaming XA-ADPCM voice. Replaces any active XA stream
    /// without crossfading. The cpal callback mixes XA samples into the
    /// SPU output at 44.1 kHz; a one-shot stream auto-detaches when the
    /// cursor runs off the end (set `looping = true` for BGM).
    ///
    /// `gain` uses the same Q1.14 fixed-point as SPU voice volumes -
    /// pass `0x4000` for unity (no attenuation).
    pub fn play_xa(
        &self,
        pcm: Vec<i16>,
        sample_rate: u32,
        channels: legaia_xa::Channels,
        looping: bool,
        gain: u16,
    ) {
        let mut s = self.lock();
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
    }

    /// Install an arts-voice battle shout, modeling the retail CD/XA
    /// scheduling contract instead of the FMV one-shot path.
    ///
    /// Two retail behaviours the plain [`AudioOut::play_xa`] cannot express:
    ///
    /// * **Response-presentation delay.** `start_delay_frames` (in 44.1 kHz
    ///   SPU samples) holds the shout silent before its first audible sample,
    ///   mirroring the CD controller's fixed seek/read-to-first-sector
    ///   latency. A caller that requests the shout on the animation-start
    ///   frame gets a start that *trails* the animation by this delay -
    ///   matching the post-fix retail sync - rather than racing ahead of it
    ///   (the pre-fix bug where "XA audio began well before the animation").
    ///
    /// * **Back-to-back no-drop.** If a shout is already active, a new request
    ///   is *queued* (staged into `pending_xa`) rather than cutting the active
    ///   one mid-play; the queued shout starts the sample the active one runs
    ///   off its end. This is the counterpart to the retail back-to-back
    ///   Hyper-Art fix (the later clip must not be dropped). One deep - a
    ///   third request while one is active and one queued replaces the queued
    ///   one (retail plays combo shouts strictly in sequence, so only the most
    ///   recent pending clip is meaningful).
    ///
    /// Shouts are always one-shot mono/stereo at unity-ish `gain`; `looping`
    /// has no analogue here.
    pub fn play_xa_shout(
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
        stage_xa_shout(&mut self.lock(), shout);
    }

    /// Install a streaming XA-ADPCM voice fed from a buffer of raw XA-ADPCM
    /// sound-group bytes (128-byte aligned). Decodes the entire buffer
    /// up-front through the new [`legaia_xa::StreamingDecoder`] before
    /// staging it as an [`XaPlayback`]; this is behaviourally equivalent
    /// to the all-at-once [`legaia_xa::decode`] path but exercises the
    /// incremental decoder so future producer-thread / ring-buffer
    /// consumers share the same surface.
    ///
    /// Engines streaming long XA tracks from a disc image should chunk the
    /// sectors through [`legaia_xa::StreamingDecoder::feed`] directly and
    /// stage decoded PCM into [`AudioOut::play_xa`] in batches (the audio
    /// callback can't block on disc I/O).
    pub fn play_xa_streaming(
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
        // Drop trailing partial group bytes; XA spec is whole-group aligned.
        self.play_xa(pcm, sample_rate, channels, looping, gain);
        Ok(())
    }

    /// Detach the active XA stream (if any). Subsequent frames mix only
    /// the SPU output.
    pub fn stop_xa(&self) {
        let mut s = self.lock();
        s.xa = None;
        s.pending_xa = None;
    }

    /// `true` if an XA stream is currently attached and not yet exhausted.
    pub fn xa_active(&self) -> bool {
        let s = self.lock();
        s.xa.as_ref().is_some_and(|x| !x.is_done())
    }

    /// Playback position of the active XA stream in seconds, or `None` when
    /// no stream is attached. The cursor advances inside the cpal callback at
    /// the audio device's true rate, so this is a hardware-paced clock - a
    /// video player can drive its frame advance off it to keep MDEC video in
    /// lock-step with the interleaved XA track (no drift from a separate
    /// wall-clock timer). The value is `cursor_frames / sample_rate` and is
    /// monotonic until the one-shot stream runs off the end (where it pins at
    /// the stream duration).
    pub fn xa_cursor_secs(&self) -> Option<f64> {
        let s = self.lock();
        s.xa.as_ref().map(|x| {
            if x.sample_rate == 0 {
                0.0
            } else {
                x.cursor / x.sample_rate as f64
            }
        })
    }

    /// Install a sequencer immediately. The cpal callback ticks it once per
    /// SPU sample (every `1 / 44100` s) for sample-accurate timing.
    /// Replacing an existing sequencer silences any active notes from the
    /// prior one (use [`Self::crossfade_to`] for a smooth transition).
    pub fn attach_sequencer(&self, seq: Sequencer) {
        self.lock().attach_sequencer(seq);
    }

    /// Detach the active sequencer (if any) and key-off whatever it had
    /// running. Cancels any in-progress crossfade.
    pub fn detach_sequencer(&self) {
        self.lock().detach_sequencer();
    }

    /// Gate the sequencer tick without detaching it. When `paused` is
    /// `true` the sequencer clock stops and the notes it had sounding are
    /// keyed off, as retail's paused-slot service does (`FUN_800638D8`);
    /// call with `false` to resume from where the sequencer left off.
    pub fn set_sequencer_paused(&self, paused: bool) {
        self.lock().set_sequencer_paused(paused);
    }

    /// Rewind the attached sequencer to its first event (no-op with none
    /// attached). Op-`0x35` sub-op `4` replays the slot's sequence from its
    /// start - `FUN_80026478` -> `FUN_80062880(id, 1, 1)` ->
    /// `FUN_800628F0`, which resets the read cursor before it plays - so a
    /// director's resume rewinds before it reopens the gate.
    pub fn rewind_sequencer(&self) {
        self.lock().rewind_sequencer();
    }

    /// Whether the sequencer gate is currently closed
    /// ([`Self::set_sequencer_paused`]). Both BGM directors key their pause
    /// state on this one bit - the native `AudioBgmDirector` and the browser
    /// twin over `WebAudioOut::sequencer_paused` - so a
    /// movie that ducks the score and the op-`0x35` pause arms read and
    /// write the same latch.
    pub fn sequencer_paused(&self) -> bool {
        self.lock().sequencer_paused
    }

    /// Set the attached sequencer's master volume (`SsSeqSetVol`-shaped,
    /// `0..=127`) in place, without restarting it. The battle's audio duck
    /// rides this: retail ramps `_DAT_8007B910` and re-applies it through
    /// `FUN_800267A8` -> `FUN_80062004` each frame (`docs/subsystems/battle-action.md`
    /// § the `_DAT_8007B910` ramps are an audio duck). No-op with no
    /// sequencer attached; a pending cross-fade target inherits it when it
    /// installs.
    pub fn set_sequencer_master_vol(&self, vol: u8) {
        let mut s = self.lock();
        if let Some(seq) = s.sequencer.as_mut() {
            seq.set_master_vol(vol);
        }
        if let Some(seq) = s.pending_seq.as_mut() {
            seq.set_master_vol(vol);
        }
    }

    /// Cross-fade from the currently-playing sequencer to `new_seq` over
    /// `fade_samples` SPU-rate samples (44 100 Hz). The existing sequencer
    /// fades out, then `new_seq` is swapped in and fades back up to full
    /// volume, all inside the audio callback without glitching.
    ///
    /// If no sequencer is currently playing, `new_seq` is installed
    /// immediately at full volume (same as [`Self::attach_sequencer`]).
    ///
    /// `fade_samples = 0` attaches immediately (same as
    /// [`Self::attach_sequencer`]).
    pub fn crossfade_to(&self, new_seq: Sequencer, fade_samples: u32) {
        self.lock().crossfade_to(new_seq, fade_samples);
    }

    /// Swap the active sequencer for `new_seq` **immediately**, faithful to
    /// retail's hard-cut BGM changes: the outgoing track is key-offed (its
    /// notes release through their ADSR envelopes) and `new_seq` starts
    /// sounding from its own first event this same instant. Unlike
    /// [`Self::crossfade_to`], the incoming track's intro is never held silent
    /// behind a fade-out - the new sequencer is active on the very next SPU
    /// sample.
    ///
    /// `fade_in_samples` is a short click-guard ramp (a couple of frames at
    /// most) that rises the SPU master from near-silence to full as the swap
    /// lands, so an abrupt onset can't pop. Pass `0` for a true hard cut. Use
    /// this - not `crossfade_to` - for BGM transitions where the new track's
    /// intro must be heard.
    pub fn swap_bgm(&self, new_seq: Sequencer, fade_in_samples: u32) {
        self.lock().swap_bgm(new_seq, fade_in_samples);
    }

    /// Snapshot of the sequencer's progress, returned `None` if no sequencer
    /// is currently attached. Caller-side polling for UI / progress bars.
    pub fn sequencer_progress(&self) -> Option<SequencerProgress> {
        self.lock().sequencer_progress()
    }
}

/// Convert raw PCM into a stream of "silence-filtered" SPU-ADPCM blocks
/// that decode to (approximately) the original samples.
///
/// The trick: filter=0 / shift=0 / nibble = `(pcm[i] >> 12) & 0xF` decodes
/// (per `legaia_xa::F0[0]=0`) to exactly `(nibble_signed << 12)`. So if we
/// encode the top 4 bits of each sample, the decoder reproduces the top
/// 4 bits - a coarse but functional preview. Good enough for "does sample
/// N play and at the right pitch?"
///
/// For full-fidelity playback we'd round-trip through real ADPCM encoding,
/// but that's another module worth of code; preview path keeps it simple.
pub(crate) fn pcm_to_silence_padded_adpcm(pcm: &[i16]) -> Vec<u8> {
    if pcm.is_empty() {
        return vec![0u8; BLOCK_BYTES * 2]; // empty + end block
    }
    let n_full = pcm.len() / SAMPLES_PER_BLOCK;
    let leftover = pcm.len() % SAMPLES_PER_BLOCK;
    let n_blocks = n_full + if leftover > 0 { 1 } else { 0 };
    let mut out = vec![0u8; (n_blocks + 1) * BLOCK_BYTES];

    for b in 0..n_blocks {
        let off = b * BLOCK_BYTES;
        out[off] = 0x00; // filter=0, shift=0
        out[off + 1] = 0x00; // no flags
        for i in 0..SAMPLES_PER_BLOCK {
            let sample_idx = b * SAMPLES_PER_BLOCK + i;
            let s = pcm.get(sample_idx).copied().unwrap_or(0);
            // Quantise to top 4 bits, signed.
            let q = ((s >> 12) & 0xF) as u8;
            let byte_off = off + 2 + (i / 2);
            if i % 2 == 0 {
                out[byte_off] = (out[byte_off] & 0xF0) | q;
            } else {
                out[byte_off] = (out[byte_off] & 0x0F) | (q << 4);
            }
        }
    }
    // Terminator block: end+repeat clear, but flag.end set so voice stops.
    let last = n_blocks * BLOCK_BYTES;
    out[last] = 0x00;
    out[last + 1] = 0x01; // end flag, no repeat
    out
}
