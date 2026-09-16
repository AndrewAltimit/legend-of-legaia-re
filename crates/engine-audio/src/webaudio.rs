//! WebAudio backend for `wasm32` targets (activated by the `audio-webaudio`
//! feature). Provides a [`WebAudioOut`] that mirrors the public API of
//! [`crate::AudioOut`] so engine code can be written against the same surface
//! regardless of platform.
//!
//! Implemented via a `ScriptProcessorNode` (deprecated but universally
//! supported; `AudioWorkletNode` would require shipping a separate JS worker
//! file and is deferred). The node drives the SPU mixer and SEQ sequencer
//! from a periodic callback on the main browser thread.
//!
//! Must be initialised from a user-gesture handler to satisfy the browser
//! autoplay policy - call [`WebAudioOut::new`] inside e.g. a button click.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use web_sys::AudioProcessingEvent;

use crate::spu::Spu;
use crate::{Sequencer, SequencerProgress, StreamResampler, XaPlayback};

/// Master output trim for the browser hosts, applied by
/// [`WebAudioOut::set_gain`] on top of the caller's value.
///
/// A loudness setting, not a mix control: the browser pages were simply too
/// loud, and scaling once at the output stage leaves every visible volume
/// control at its full range and every relative balance intact. The site's
/// JS audio paths (minigames, media browser) carry the same factor in
/// `site/js/layout.js` - keep the two in step, or a page mixing WASM and JS
/// sound will drift.
///
/// The **native** cpal path is deliberately untouched: the desktop window
/// mixes at the retail-nominal level and has an OS volume control in front
/// of it, so it has no such problem to solve.
pub const WEB_MASTER_TRIM: f32 = 0.25;

/// WebAudio-backed audio output for `wasm32` targets.
///
/// The `ScriptProcessorNode` fires a callback every 4096 output frames
/// (~92 ms at 44.1 kHz). Inside that callback the SPU mixer and SEQ
/// sequencer are advanced by the same [`StreamResampler`] that the native
/// cpal path uses, so playback quality is identical on both targets.
pub struct WebAudioOut {
    ctx: web_sys::AudioContext,
    /// Must be kept alive for the duration of the stream - dropping this
    /// de-registers the `onaudioprocess` callback and silences the node.
    _onaudioprocess: Closure<dyn FnMut(AudioProcessingEvent)>,
    state: Rc<RefCell<StreamResampler>>,
    /// User-controllable gain stage between the `ScriptProcessorNode` and
    /// the destination, so a host can scale output without re-mixing in
    /// WASM. Defaults to unity; the play page hangs its volume slider here.
    gain: web_sys::GainNode,
}

impl WebAudioOut {
    /// Open the browser's default audio output. Returns an error if
    /// `AudioContext` construction fails (e.g. still blocked by autoplay
    /// policy before a user gesture, or if the browser doesn't support it).
    pub fn new() -> anyhow::Result<Self> {
        let ctx = web_sys::AudioContext::new()
            .map_err(|e| anyhow::anyhow!("AudioContext::new: {:?}", e))?;
        let device_rate = ctx.sample_rate() as u32;
        let state = Rc::new(RefCell::new(StreamResampler::new(device_rate)));

        // ScriptProcessorNode: 4096-frame buffer, 0 input channels, 2 output (L/R).
        let node = ctx
            .create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(
                4096, 0, 2,
            )
            .map_err(|e| anyhow::anyhow!("createScriptProcessor: {:?}", e))?;

        let state_cb = Rc::clone(&state);
        let closure =
            Closure::<dyn FnMut(AudioProcessingEvent)>::new(move |event: AudioProcessingEvent| {
                let output = match event.output_buffer() {
                    Ok(b) => b,
                    Err(_) => return,
                };
                let length = output.length() as usize;
                let mut left = vec![0.0f32; length];
                let mut right = vec![0.0f32; length];
                {
                    let mut s = state_cb.borrow_mut();
                    // The monaural downmix is applied here rather than inside
                    // `next_frame`, exactly as the cpal callback applies it -
                    // the mute gate is in `next_frame` and reaches both
                    // backends for free, but the downmix is a per-callback
                    // channel decision and each backend has to make it.
                    let mono = s.mono;
                    for i in 0..length {
                        let (l, r) = s.next_frame();
                        let (l, r) = if mono {
                            let m = ((l as i32 + r as i32) / 2) as i16;
                            (m, m)
                        } else {
                            (l, r)
                        };
                        left[i] = l as f32 / i16::MAX as f32;
                        right[i] = r as f32 / i16::MAX as f32;
                    }
                }
                let _ = output.copy_to_channel(&left, 0);
                let _ = output.copy_to_channel(&right, 1);
            });

        node.set_onaudioprocess(Some(closure.as_ref().unchecked_ref()));

        // Insert a `GainNode` between the script processor and the
        // destination so the JS side can scale SPU output without
        // re-mixing in WASM. Default gain matches the engine-shell cpal
        // path (1.0); the play page overrides it from its volume slider.
        let gain = ctx
            .create_gain()
            .map_err(|e| anyhow::anyhow!("createGain: {:?}", e))?;
        // Same trim `set_gain` applies, so the pre-first-slider-update level
        // matches everything after it (this node is live from the moment the
        // stream starts - leaving 1.0 here would make the opening moments of
        // a page the only loud thing on it).
        gain.gain().set_value(WEB_MASTER_TRIM);
        node.connect_with_audio_node(&gain)
            .map_err(|e| anyhow::anyhow!("AudioNode::connect(script -> gain): {:?}", e))?;
        gain.connect_with_audio_node(&ctx.destination())
            .map_err(|e| anyhow::anyhow!("AudioNode::connect(gain -> destination): {:?}", e))?;

        Ok(Self {
            ctx,
            _onaudioprocess: closure,
            state,
            gain,
        })
    }

    /// Set the post-mixer gain. `1.0` matches the native cpal path, and the
    /// browser hosts drive it from a user-facing volume control rather than
    /// from a fixed compensation factor - the mixer's nominal level is the
    /// same on both targets, so a large constant here is loudness, not
    /// correction.
    ///
    /// [`WEB_MASTER_TRIM`] is applied on top of whatever the caller asks for,
    /// so callers keep passing their slider's own value and the browser pages
    /// come out at the site's output level.
    pub fn set_gain(&self, gain: f32) {
        self.gain.gain().set_value(gain * WEB_MASTER_TRIM);
    }

    /// Sample rate of the underlying browser `AudioContext`. The
    /// `StreamResampler` resamples SPU output from 44.1 kHz to this rate.
    pub fn device_rate(&self) -> u32 {
        self.ctx.sample_rate() as u32
    }

    /// Nudge the browser's `AudioContext` into `running` state. Browsers
    /// typically construct AudioContexts in `suspended` state - even when
    /// the constructor runs inside a user-gesture handler - and require a
    /// `.resume()` call to actually produce sound. Returns the underlying
    /// promise so callers can `await` the transition if they want to
    /// sequence "audio is now audible" UI updates against it.
    pub fn resume(&self) -> js_sys::Promise {
        self.ctx
            .resume()
            .unwrap_or_else(|_| js_sys::Promise::resolve(&JsValue::UNDEFINED))
    }

    /// Toggle the monaural downmix (the retail options screen's
    /// "Sound: Stereo / Monaural" row) - the twin of
    /// [`crate::AudioOut::set_mono`]. Without it the browser hosts had no
    /// reader for that row at all, so the option was decorative on one host
    /// and live on the other.
    pub fn set_mono(&self, mono: bool) {
        self.state.borrow_mut().mono = mono;
    }

    /// Master mute gate - the twin of [`crate::AudioOut::set_muted`]. The
    /// producers keep ticking while muted (the gate lives in `next_frame`),
    /// so unmuting resumes mid-track in sync.
    pub fn set_muted(&self, muted: bool) {
        self.state.borrow_mut().muted = muted;
    }

    /// Current state of the master mute gate.
    pub fn is_muted(&self) -> bool {
        self.state.borrow().muted
    }

    /// Run a closure with mutable access to the underlying SPU model.
    pub fn with_spu<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Spu) -> R,
    {
        f(&mut self.state.borrow_mut().spu)
    }

    /// Install a sequencer. The `ScriptProcessorNode` callback ticks it once
    /// per SPU sample for sample-accurate timing. Replaces any active
    /// sequencer immediately (use [`Self::crossfade_to`] for smooth transitions).
    pub fn attach_sequencer(&self, seq: Sequencer) {
        let mut s = self.state.borrow_mut();
        if let Some(mut prev) = s.sequencer.take() {
            prev.stop(&mut s.spu);
        }
        s.pending_seq = None;
        s.master_fade = 1.0;
        s.fade_target = 1.0;
        s.fade_step = 0.0;
        s.sequencer = Some(seq);
    }

    /// Detach the active sequencer (if any) and key-off any sounding notes.
    pub fn detach_sequencer(&self) {
        let mut s = self.state.borrow_mut();
        if let Some(mut seq) = s.sequencer.take() {
            seq.stop(&mut s.spu);
        }
        s.pending_seq = None;
        s.master_fade = 1.0;
        s.fade_target = 1.0;
        s.fade_step = 0.0;
    }

    /// Gate the sequencer tick - the twin of
    /// [`crate::AudioOut::set_sequencer_paused`]. When `paused`, the
    /// sequencer clock stops and the notes it had sounding are keyed off;
    /// `false` resumes from the playhead.
    pub fn set_sequencer_paused(&self, paused: bool) {
        self.state.borrow_mut().set_sequencer_paused(paused);
    }

    /// Whether the sequencer gate is currently closed
    /// ([`Self::set_sequencer_paused`]). The browser BGM director keys the
    /// op-`0x35` sub-`0xA` unhalt-pause commit on this - the native
    /// director keeps its own latch, but here the gate is the only pause
    /// state there is.
    pub fn sequencer_paused(&self) -> bool {
        self.state.borrow().sequencer_paused
    }

    /// Cross-fade from the current sequencer to `new_seq` over
    /// `fade_samples` SPU-rate (44.1 kHz) samples. If no sequencer is
    /// active, `new_seq` is installed immediately at full volume.
    pub fn crossfade_to(&self, new_seq: Sequencer, fade_samples: u32) {
        let mut s = self.state.borrow_mut();
        if fade_samples == 0 || s.sequencer.is_none() {
            if let Some(mut prev) = s.sequencer.take() {
                prev.stop(&mut s.spu);
            }
            s.pending_seq = None;
            s.master_fade = 1.0;
            s.fade_target = 1.0;
            s.fade_step = 0.0;
            s.sequencer = Some(new_seq);
        } else {
            s.pending_seq = Some(new_seq);
            s.fade_target = 0.0;
            s.fade_step = 1.0 / fade_samples.max(1) as f32;
        }
    }

    /// Swap the active sequencer for `new_seq` **immediately** - the twin of
    /// [`crate::AudioOut::swap_bgm`], and the same
    /// [`StreamResampler::swap_bgm`] underneath, so both hosts hear one
    /// model.
    ///
    /// This is the hook a BGM *change* takes. [`Self::crossfade_to`] is a
    /// serial fade: it holds the incoming track in `pending_seq` and rolls
    /// the outgoing one down to silence first, so the new track's intro is
    /// still unplayed a fade-length after the script asked for it. Retail's
    /// changes are hard cuts, and a cutscene sting is mostly intro - which is
    /// why the browser had to reach this and not the crossfade.
    ///
    /// `fade_in_samples` is a short click-guard ramp on the SPU master (a
    /// couple of frames at most) that only ever rises, so the incoming track
    /// is audible from its first sample. Pass `0` for a true hard cut.
    pub fn swap_bgm(&self, new_seq: Sequencer, fade_in_samples: u32) {
        self.state.borrow_mut().swap_bgm(new_seq, fade_in_samples);
    }

    /// Snapshot of the attached sequencer's progress, `None` when no
    /// sequencer is attached - the twin of
    /// [`crate::AudioOut::sequencer_progress`]. The browser BGM director's
    /// duplicate-start guard keys on this: a re-emitted start for the track
    /// that is *already sounding* keeps its playhead, but the same id after
    /// the track was detached (stopped, or run off its end) must restart it.
    pub fn sequencer_progress(&self) -> Option<SequencerProgress> {
        self.state.borrow().sequencer_progress()
    }

    /// Set the attached sequencer's master volume (`SsSeqSetVol`-shaped,
    /// `0..=127`) in place, without restarting it - the twin of
    /// [`crate::AudioOut::set_sequencer_master_vol`]. The battle audio duck
    /// rides this: retail ramps `_DAT_8007B910` one unit per vsync and
    /// re-applies it through `FUN_800267A8` -> `FUN_80062004` each frame.
    /// No-op with no sequencer attached; a pending cross-fade target
    /// inherits it when it installs.
    pub fn set_sequencer_master_vol(&self, vol: u8) {
        let mut s = self.state.borrow_mut();
        if let Some(seq) = s.sequencer.as_mut() {
            seq.set_master_vol(vol);
        }
        if let Some(seq) = s.pending_seq.as_mut() {
            seq.set_master_vol(vol);
        }
    }

    /// Install a streaming XA-ADPCM voice, replacing any active stream
    /// without crossfading - the twin of [`crate::AudioOut::play_xa`]. The
    /// `ScriptProcessorNode` callback mixes it into the SPU output at
    /// 44.1 kHz inside the same [`StreamResampler`] the cpal path uses, so
    /// it sits **before** the post-mixer `GainNode` and rides the page's
    /// volume slider + [`WEB_MASTER_TRIM`] exactly like BGM and SFX do.
    ///
    /// `gain` is Q1.14 like SPU voice volumes - `0x4000` is unity.
    pub fn play_xa(
        &self,
        pcm: Vec<i16>,
        sample_rate: u32,
        channels: legaia_xa::Channels,
        looping: bool,
        gain: u16,
    ) {
        let mut s = self.state.borrow_mut();
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

    /// Install an arts-voice battle shout / one-shot CD-XA clip with the
    /// modelled CD-response start delay and the back-to-back no-drop queue -
    /// the twin of [`crate::AudioOut::play_xa_shout`], through the same
    /// [`crate::stage_xa_shout`] staging the cpal path and the
    /// [`crate::OfflineMixer`] share. A shout requested while one is sounding
    /// queues behind it (one deep); `start_delay_frames` is in SPU samples.
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
        crate::stage_xa_shout(&mut self.state.borrow_mut(), shout);
    }

    /// Decode a buffer of raw XA-ADPCM sound-group bytes (128-byte aligned)
    /// through [`legaia_xa::StreamingDecoder`] and stage the PCM as an XA
    /// stream - the twin of [`crate::AudioOut::play_xa_streaming`].
    pub fn play_xa_streaming(
        &self,
        raw_bytes: &[u8],
        sample_rate: u32,
        channels: legaia_xa::Channels,
        looping: bool,
        gain: u16,
    ) -> anyhow::Result<()> {
        let mut decoder = legaia_xa::StreamingDecoder::new(legaia_xa::DecodeOptions {
            channels,
            sample_rate,
            bits: legaia_xa::BitsPerSample::Four,
        });
        let mut pcm = Vec::with_capacity(raw_bytes.len() / 128 * 224);
        decoder.feed(raw_bytes, &mut pcm)?;
        self.play_xa(pcm, sample_rate, channels, looping, gain);
        Ok(())
    }

    /// Detach the active XA stream and drop any queued shout - the twin of
    /// [`crate::AudioOut::stop_xa`].
    pub fn stop_xa(&self) {
        let mut s = self.state.borrow_mut();
        s.xa = None;
        s.pending_xa = None;
    }

    /// `true` while an XA stream is attached and not yet exhausted - the
    /// twin of [`crate::AudioOut::xa_active`].
    pub fn xa_active(&self) -> bool {
        self.state
            .borrow()
            .xa
            .as_ref()
            .is_some_and(|x| !x.is_done())
    }

    /// Playback position of the active XA stream in seconds (`None` with no
    /// stream) - the twin of [`crate::AudioOut::xa_cursor_secs`]. Advanced
    /// inside the audio callback, so it is the device-paced clock a video
    /// player can lock its frame advance to.
    pub fn xa_cursor_secs(&self) -> Option<f64> {
        self.state.borrow().xa.as_ref().map(|x| {
            if x.sample_rate == 0 {
                0.0
            } else {
                x.cursor / x.sample_rate as f64
            }
        })
    }
}
