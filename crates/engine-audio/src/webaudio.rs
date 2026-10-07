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

use crate::StreamResampler;

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

/// Closing the context on drop stops the `ScriptProcessorNode` with it. The
/// node stays connected to the destination after the Rust side is gone, so
/// without this a dropped output (a second `new()` replacing the first) keeps
/// calling a freed closure on every buffer - wasm-bindgen's "closure invoked
/// recursively or after being dropped", forever.
impl Drop for WebAudioOut {
    fn drop(&mut self) {
        let _ = self.ctx.close();
    }
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
}

impl crate::AudioSink for WebAudioOut {
    fn with_core<R>(&self, f: impl FnOnce(&mut StreamResampler) -> R) -> R {
        f(&mut self.state.borrow_mut())
    }
}
