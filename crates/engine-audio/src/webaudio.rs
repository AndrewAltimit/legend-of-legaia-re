//! WebAudio backend for `wasm32` targets (activated by the `audio-webaudio`
//! feature). Provides a [`WebAudioOut`] that mirrors the public API of
//! [`crate::AudioOut`] so engine code can be written against the same surface
//! regardless of platform.
//!
//! The mix runs on the page's main thread, because that is where the engine
//! is: a cue keys the SPU directly. What varies is how finished frames reach
//! the device.
//!
//! - **Worklet.** An `AudioWorkletProcessor` plays blocks posted to it and
//!   reports what it played; each report tops the queue up to [`MixPump`]'s
//!   target. A cue sounds one queue-depth after it fires. `postMessage` needs
//!   no `SharedArrayBuffer`, so no COOP / COEP headers, which a static host
//!   cannot set. The processor's source is inlined and loaded from a blob URL,
//!   so the bundle stays one file set. Reports arrive as message events, which
//!   background-tab timer throttling does not touch.
//! - **Script processor.** The fallback where `audioWorklet` is missing (an
//!   insecure origin) or will not load: a 4096-frame `ScriptProcessorNode`, a
//!   cue landing at the next buffer.
//!
//! Must be initialised from a user-gesture handler to satisfy the browser
//! autoplay policy - call [`WebAudioOut::new`] inside e.g. a button click.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use web_sys::{AudioProcessingEvent, AudioWorkletNode, MessageEvent, MessagePort};

use crate::StreamResampler;
use crate::mix_pump::MixPump;

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
/// The **native** cpal path does not use this trim: the desktop hosts scale
/// the finished mix by their own user-facing master volume
/// ([`crate::AudioSink::set_master_volume`], default
/// [`crate::DESKTOP_MASTER_VOLUME_DEFAULT`]).
pub const WEB_MASTER_TRIM: f32 = 0.25;

/// Plays posted blocks of interleaved stereo and reports `[played, dry frames,
/// lowest queue since the last report]` every third render quantum. Silence
/// before the first block is the context starting up, not a stall; silence after
/// it is one.
const WORKLET_SOURCE: &str = r#"
class LegaiaMixSink extends AudioWorkletProcessor {
  constructor() {
    super();
    this.blocks = [];
    this.at = 0;
    this.queued = 0;
    this.low = Infinity;
    this.played = 0;
    this.dry = 0;
    this.quanta = 0;
    this.port.onmessage = (e) => {
      this.blocks.push(e.data);
      this.queued += e.data.length / 2;
    };
  }
  process(_inputs, outputs) {
    const out = outputs[0];
    const l = out[0];
    const r = out[1] || out[0];
    let i = 0;
    while (i < l.length && this.blocks.length) {
      const b = this.blocks[0];
      const n = Math.min(l.length - i, (b.length - this.at) / 2);
      for (let k = 0; k < n; k++) {
        l[i + k] = b[this.at + k * 2];
        r[i + k] = b[this.at + k * 2 + 1];
      }
      i += n;
      this.at += n * 2;
      if (this.at >= b.length) {
        this.blocks.shift();
        this.at = 0;
      }
    }
    this.queued -= i;
    if (this.played > 0 || i > 0) {
      this.low = Math.min(this.low, this.queued);
      this.dry += l.length - i;
    }
    this.played += i;
    if (++this.quanta % 3 === 0) {
      this.port.postMessage([this.played, this.dry, this.low]);
      this.low = Infinity;
    }
    return true;
  }
}
registerProcessor('legaia-mix-sink', LegaiaMixSink);
"#;

/// Set `globalThis.__LEGAIA_AUDIO_PATH = 'script'` before the output opens to
/// take the fallback where the worklet would work: how the two are compared.
const PATH_OVERRIDE: &str = "__LEGAIA_AUDIO_PATH";

/// How frames reach the device.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WebAudioPath {
    /// The worklet module is still loading; nothing is playing yet.
    Opening,
    Worklet,
    ScriptProcessor,
}

/// The live node and the callback it fires. Dropping the callback while the
/// node can still fire it is wasm-bindgen's "closure invoked after being
/// dropped", so [`Inner`]'s drop detaches it first.
enum Driver {
    None,
    Script {
        node: web_sys::ScriptProcessorNode,
        _callback: Closure<dyn FnMut(AudioProcessingEvent)>,
    },
    Worklet {
        _node: AudioWorkletNode,
        port: MessagePort,
        _callback: Closure<dyn FnMut(MessageEvent)>,
    },
}

struct Inner {
    ctx: web_sys::AudioContext,
    state: RefCell<StreamResampler>,
    /// User-controllable gain stage between the driver node and the
    /// destination, so a host can scale output without re-mixing in WASM. The
    /// play page hangs its volume slider here.
    gain: web_sys::GainNode,
    pump: RefCell<MixPump>,
    driver: RefCell<Driver>,
    path: Cell<WebAudioPath>,
}

/// WebAudio-backed audio output for `wasm32` targets. See the module docs for
/// the two paths; inside either, the SPU mixer and SEQ sequencer are advanced
/// by the same [`StreamResampler`] that the native cpal path uses, so playback
/// quality is identical on both targets.
pub struct WebAudioOut {
    inner: Rc<Inner>,
}

/// Closing the context on drop stops the driver node with it.
impl Drop for WebAudioOut {
    fn drop(&mut self) {
        match &*self.inner.driver.borrow() {
            Driver::Script { node, .. } => node.set_onaudioprocess(None),
            Driver::Worklet { port, .. } => port.set_onmessage(None),
            Driver::None => {}
        }
        let _ = self.inner.ctx.close();
    }
}

fn install_script_processor(inner: &Rc<Inner>) -> anyhow::Result<()> {
    // ScriptProcessorNode: 4096-frame buffer, 0 input channels, 2 output (L/R).
    let node = inner
        .ctx
        .create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(
            4096, 0, 2,
        )
        .map_err(|e| anyhow::anyhow!("createScriptProcessor: {:?}", e))?;
    let weak = Rc::downgrade(inner);
    let callback =
        Closure::<dyn FnMut(AudioProcessingEvent)>::new(move |event: AudioProcessingEvent| {
            let (Some(inner), Ok(output)) = (weak.upgrade(), event.output_buffer()) else {
                return;
            };
            let length = output.length() as usize;
            let mut left = vec![0.0f32; length];
            let mut right = vec![0.0f32; length];
            {
                let mut s = inner.state.borrow_mut();
                // The host downmix (`downmix_mono`) and the mute gate are both
                // inside `next_frame`, shared with the cpal backend.
                for i in 0..length {
                    let (l, r) = s.next_frame();
                    left[i] = l as f32 / i16::MAX as f32;
                    right[i] = r as f32 / i16::MAX as f32;
                }
            }
            let _ = output.copy_to_channel(&left, 0);
            let _ = output.copy_to_channel(&right, 1);
        });
    node.set_onaudioprocess(Some(callback.as_ref().unchecked_ref()));
    node.connect_with_audio_node(&inner.gain)
        .map_err(|e| anyhow::anyhow!("AudioNode::connect(script -> gain): {:?}", e))?;
    *inner.driver.borrow_mut() = Driver::Script {
        node,
        _callback: callback,
    };
    inner.path.set(WebAudioPath::ScriptProcessor);
    Ok(())
}

/// Mix what the queue is short of and post it to the processor.
fn pump(inner: &Inner, port: &MessagePort) {
    let frames = inner.pump.borrow_mut().take() as usize;
    if frames == 0 {
        return;
    }
    let mut block = vec![0.0f32; frames * 2];
    {
        let mut s = inner.state.borrow_mut();
        for f in block.as_chunks_mut::<2>().0 {
            let (l, r) = s.next_frame();
            f[0] = l as f32 / i16::MAX as f32;
            f[1] = r as f32 / i16::MAX as f32;
        }
    }
    let array = js_sys::Float32Array::from(block.as_slice());
    let transfer = js_sys::Array::of1(&array.buffer());
    let _ = port.post_message_with_transferable(&array, &transfer);
}

fn install_worklet_node(inner: &Rc<Inner>) -> Result<(), JsValue> {
    let options = web_sys::AudioWorkletNodeOptions::new();
    options.set_number_of_inputs(0);
    options.set_number_of_outputs(1);
    options.set_output_channel_count(&js_sys::Array::of1(&JsValue::from(2)));
    let node = AudioWorkletNode::new_with_options(&inner.ctx, "legaia-mix-sink", &options)?;
    let port = node.port()?;
    let weak = Rc::downgrade(inner);
    let reply = port.clone();
    let callback = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let Some(inner) = weak.upgrade() else { return };
        let report = js_sys::Array::from(&event.data());
        // The low word reads `Infinity` on a report with no block yet.
        let word = |i| report.get(i).as_f64().unwrap_or(0.0) as u64;
        inner.pump.borrow_mut().report(word(0), word(1), word(2));
        pump(&inner, &reply);
    });
    port.set_onmessage(Some(callback.as_ref().unchecked_ref()));
    node.connect_with_audio_node(&inner.gain)?;
    pump(inner, &port);
    *inner.driver.borrow_mut() = Driver::Worklet {
        _node: node,
        port,
        _callback: callback,
    };
    inner.path.set(WebAudioPath::Worklet);
    Ok(())
}

/// Load the processor's module, and take the fallback if any step refuses.
fn open_worklet(inner: &Rc<Inner>) -> Result<(), JsValue> {
    let worklet = inner.ctx.audio_worklet()?;
    let parts = js_sys::Array::of1(&JsValue::from_str(WORKLET_SOURCE));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("application/javascript");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &options)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)?;
    let loading = worklet.add_module(&url)?;
    let settle = |inner: Weak<Inner>, url: String, worked: bool| {
        Closure::once(move |_: JsValue| {
            let _ = web_sys::Url::revoke_object_url(&url);
            let Some(inner) = inner.upgrade() else { return };
            let worked = worked && install_worklet_node(&inner).is_ok();
            if !worked && let Err(e) = install_script_processor(&inner) {
                web_sys::console::error_1(&format!("audio output: {e}").into());
            }
        })
    };
    let done = settle(Rc::downgrade(inner), url.clone(), true);
    let failed = settle(Rc::downgrade(inner), url, false);
    let _ = loading.then2(&done, &failed);
    // One output's two one-shot callbacks: whichever does not fire stays allocated.
    done.forget();
    failed.forget();
    Ok(())
}

impl WebAudioOut {
    /// Open the browser's default audio output. Returns an error if
    /// `AudioContext` construction fails (e.g. still blocked by autoplay
    /// policy before a user gesture, or if the browser doesn't support it).
    pub fn new() -> anyhow::Result<Self> {
        let ctx = web_sys::AudioContext::new()
            .map_err(|e| anyhow::anyhow!("AudioContext::new: {:?}", e))?;
        let device_rate = ctx.sample_rate() as u32;

        // Insert a `GainNode` between the driver node and the destination so
        // the JS side can scale SPU output without re-mixing in WASM. Default
        // gain matches the engine-shell cpal path (1.0); the play page
        // overrides it from its volume slider.
        let gain = ctx
            .create_gain()
            .map_err(|e| anyhow::anyhow!("createGain: {:?}", e))?;
        // Same trim `set_gain` applies, so the pre-first-slider-update level
        // matches everything after it (this node is live from the moment the
        // stream starts - leaving 1.0 here would make the opening moments of
        // a page the only loud thing on it).
        gain.gain().set_value(WEB_MASTER_TRIM);
        gain.connect_with_audio_node(&ctx.destination())
            .map_err(|e| anyhow::anyhow!("AudioNode::connect(gain -> destination): {:?}", e))?;

        let has_worklet = js_sys::Reflect::get(&ctx, &"audioWorklet".into())
            .is_ok_and(|w| !w.is_undefined() && !w.is_null())
            && js_sys::Reflect::get(&js_sys::global(), &"AudioWorkletNode".into())
                .is_ok_and(|n| n.is_function());
        let forced_script = js_sys::Reflect::get(&js_sys::global(), &PATH_OVERRIDE.into())
            .ok()
            .and_then(|v| v.as_string())
            .is_some_and(|v| v == "script");

        let inner = Rc::new(Inner {
            state: RefCell::new(StreamResampler::new(device_rate)),
            pump: RefCell::new(MixPump::new(device_rate)),
            driver: RefCell::new(Driver::None),
            path: Cell::new(WebAudioPath::Opening),
            ctx,
            gain,
        });
        if !has_worklet || forced_script || open_worklet(&inner).is_err() {
            install_script_processor(&inner)?;
        }
        Ok(Self { inner })
    }

    /// The path in use, the queue and its target in frames, frames played and
    /// played as silence, the context's own latencies, for a page's
    /// diagnostics. JSON, because this is not an API anyone should build on.
    pub fn stats_json(&self) -> String {
        let pump = self.inner.pump.borrow();
        let latency = |name: &str| {
            js_sys::Reflect::get(&self.inner.ctx, &name.into())
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
        };
        format!(
            "{{\"path\":\"{:?}\",\"rate\":{},\"target\":{},\"depth\":{},\"played\":{},\"dry_frames\":{},\"base_latency\":{},\"output_latency\":{}}}",
            self.inner.path.get(),
            self.device_rate(),
            pump.target(),
            pump.depth(),
            pump.played(),
            pump.dry_frames(),
            latency("baseLatency"),
            latency("outputLatency"),
        )
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
        self.inner.gain.gain().set_value(gain * WEB_MASTER_TRIM);
    }

    /// Sample rate of the underlying browser `AudioContext`. The
    /// `StreamResampler` resamples SPU output from 44.1 kHz to this rate.
    pub fn device_rate(&self) -> u32 {
        self.inner.ctx.sample_rate() as u32
    }

    /// Nudge the browser's `AudioContext` into `running` state. Browsers
    /// typically construct AudioContexts in `suspended` state - even when
    /// the constructor runs inside a user-gesture handler - and require a
    /// `.resume()` call to actually produce sound. Returns the underlying
    /// promise so callers can `await` the transition if they want to
    /// sequence "audio is now audible" UI updates against it.
    pub fn resume(&self) -> js_sys::Promise {
        self.inner
            .ctx
            .resume()
            .unwrap_or_else(|_| js_sys::Promise::resolve(&JsValue::UNDEFINED))
    }
}

impl crate::AudioSink for WebAudioOut {
    fn with_core<R>(&self, f: impl FnOnce(&mut StreamResampler) -> R) -> R {
        f(&mut self.inner.state.borrow_mut())
    }
}
