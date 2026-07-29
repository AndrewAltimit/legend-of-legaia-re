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
use crate::{Sequencer, StreamResampler};

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
        gain.gain().set_value(1.0);
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
    pub fn set_gain(&self, gain: f32) {
        self.gain.gain().set_value(gain);
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

    /// Gate the sequencer tick. When `paused`, the sequencer clock stops
    /// while SPU voices already sounding continue to decay via their ADSR.
    pub fn set_sequencer_paused(&self, paused: bool) {
        self.state.borrow_mut().sequencer_paused = paused;
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
}
