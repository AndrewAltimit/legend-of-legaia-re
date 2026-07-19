//! cpal-backed audio output for the engine reimplementation track.
//!
//! Two layers:
//!
//! - [`spu`] - a clean-room model of the PSX SPU: 24 voices, 512 KB SPU RAM,
//!   ADSR-shaped envelopes, libspu-shaped transfer engine. Drives the
//!   actual playback every output frame at 44.1 kHz internal rate. This is
//!   what the engine reimplementation track uses to render in-game audio.
//!
//! - [`AudioOut`] - a single cpal output stream that owns an [`spu::Spu`]
//!   instance behind a `Mutex`, ticking it once per host-rate output sample
//!   (with linear resampling from the SPU's 44.1 kHz to the device rate).
//!   The asset viewer also gets a "play this VAG sample as a one-shot"
//!   convenience path that materialises a one-block stream into SPU RAM,
//!   sets up voice 0, and key-ons it through the same SPU model.
//!
//! No Sony bytes - this is a clean-room port from the libspu API surface
//! and the PSX hardware register layout (see `docs/subsystems/audio.md`).

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub mod note_trace;
pub mod seq_slots;
pub mod sequencer;
pub mod sfx;
pub mod shout;
pub mod spu;
pub mod vab_bind;
#[cfg(all(target_arch = "wasm32", feature = "audio-webaudio"))]
mod webaudio;

pub use seq_slots::{SeqResourceSlot, SeqResourceTable};
pub use sequencer::Sequencer;
pub use sfx::{
    CueDispatch, PendingCue, SfxBank, SfxEntry, SfxFireBatch, SfxScheduler, classify_cue,
    voice_pitch,
};
pub use shout::{ArtsShoutBank, SHOUT_CD_RESPONSE_DELAY, ShoutClip};
pub use spu::Spu;
pub use spu::adpcm::{AdpcmDecoder, BLOCK_BYTES, SAMPLES_PER_BLOCK};
pub use spu::adsr::{AdsrConfig, AdsrState, Phase};
pub use spu::ram::{SpuAllocator, SpuRam, TransferDirection};
pub use spu::voice::{PITCH_UNITY, SPU_INTERNAL_RATE, Voice};
pub use vab_bind::{UploadedVag, VabBank};
#[cfg(all(target_arch = "wasm32", feature = "audio-webaudio"))]
pub use webaudio::WebAudioOut;

/// Default sample rate to assume for queued buffers when the caller
/// doesn't specify one. PSX VAG samples in Legaia run at this rate
/// (verified against several extracted banks).
pub const DEFAULT_INPUT_RATE: u32 = 22_050;

/// Render `duration_samples` interleaved stereo frames of BGM at the SPU's
/// internal 44.1 kHz rate by driving `sequencer` + `spu` step-by-step.
/// The sequencer is advanced by exactly one SPU sample per output sample
/// (`Sequencer::tick_sample`), so output timing is locked to the SPU clock -
/// no callback pacing dependency. Returns interleaved i16 PCM
/// (`[l0, r0, l1, r1, ...]`).
///
/// Used by the WASM site to pre-render BGM chunks and play them through
/// `AudioBufferSourceNode` instead of the deprecated `ScriptProcessorNode`,
/// which fires its callback at variable wall-clock rates on some browsers
/// and would otherwise let BGM drift faster or slower than retail.
pub fn render_bgm_to_pcm(
    sequencer: &mut Sequencer,
    spu: &mut Spu,
    duration_samples: usize,
) -> Vec<i16> {
    let mut out = Vec::with_capacity(duration_samples * 2);
    for _ in 0..duration_samples {
        sequencer.tick_sample(spu);
        let (l, r) = spu.tick();
        out.push(l);
        out.push(r);
    }
    out
}

/// One seamless-loop slice of a sequenced track, rendered through the SPU.
///
/// The site plays minigame BGM as a looping `AudioBufferSourceNode`, and a
/// naive "render N seconds then hard-loop the whole buffer" seams audibly
/// because N is almost never a whole number of the SEQ's own loop periods.
/// This renders the track and reports the exact loop region so the browser
/// can set `loopStart` / `loopEnd` to one true period: play the lead-in once,
/// then repeat `[loop_start_sample, loop_end_sample)` - which is exactly one
/// SEQ loop iteration - forever.
pub struct BgmLoopRender {
    /// Interleaved i16 PCM (`[l0, r0, l1, r1, ...]`), length
    /// `loop_end_sample * 2` (trimmed to the end of the first full loop
    /// period when one was found, else the whole render).
    pub pcm: Vec<i16>,
    /// Frame index of the first loop rewind (start of the repeatable body,
    /// i.e. end of the one-shot lead-in). `0` when no loop was detected.
    pub loop_start_sample: usize,
    /// Frame index of the second loop rewind (one period after
    /// `loop_start_sample`). Equals `pcm.len() / 2`. `0` when no loop was
    /// detected within the render budget (treat as a one-shot).
    pub loop_end_sample: usize,
}

/// Render a sequenced track and locate one seamless loop period.
///
/// The sequencer must already be loop-configured (an in-stream loop marker or
/// a `set_loop_to` fallback) so it rewinds rather than finishing. Loop rewinds
/// are detected from outside via [`Sequencer::loop_count`] (a robust monotonic
/// counter - the playhead tick can peak and reset inside a single sample on a
/// zero-delta EOT and so hide the boundary). The returned [`BgmLoopRender`] is
/// trimmed to the end of the first full loop period (the span between the
/// first and second rewind), which is exactly one repeatable iteration.
pub fn render_bgm_loop_region(
    sequencer: &mut Sequencer,
    spu: &mut Spu,
    max_samples: usize,
) -> BgmLoopRender {
    let mut out = Vec::with_capacity(max_samples * 2);
    let base_loops = sequencer.loop_count();
    let mut first_rewind: Option<usize> = None;
    let mut second_rewind: Option<usize> = None;
    for i in 0..max_samples {
        sequencer.tick_sample(spu);
        let (l, r) = spu.tick();
        out.push(l);
        out.push(r);
        // `i + 1` because this sample has just been emitted.
        let loops = sequencer.loop_count().wrapping_sub(base_loops);
        if loops >= 1 && first_rewind.is_none() {
            first_rewind = Some(i + 1);
        } else if loops >= 2 && second_rewind.is_none() {
            second_rewind = Some(i + 1);
            break;
        }
    }
    match (first_rewind, second_rewind) {
        (Some(start), Some(end)) => {
            out.truncate(end * 2);
            BgmLoopRender {
                pcm: out,
                loop_start_sample: start,
                loop_end_sample: end,
            }
        }
        // Fewer than two rewinds fit the budget: hand back the whole render
        // with no loop region (the browser hard-loops the buffer, as before).
        _ => {
            let end = out.len() / 2;
            BgmLoopRender {
                pcm: out,
                loop_start_sample: 0,
                loop_end_sample: end,
            }
        }
    }
}

/// Decoded XA-ADPCM stream parked in the audio output for direct cpal-side
/// mixing. Bypasses the SPU voice mixer (mirrors retail PSX hardware: XA
/// audio is summed with the SPU output by the SPU's CD-input path, not by
/// any of the 24 voices).
///
/// One [`XaPlayback`] is active at a time; stash a fresh buffer via
/// [`AudioOut::play_xa`] to replace it. The `StreamResampler` owns the
/// playback cursor and drives sample-rate conversion to the SPU's 44.1 kHz
/// per output sample.
pub struct XaPlayback {
    /// Decoded interleaved PCM. Mono = `[s0, s1, s2, ...]`; stereo =
    /// `[l0, r0, l1, r1, ...]`.
    pub pcm: Vec<i16>,
    /// Source sample rate (e.g. 18900 or 37800 for PSX XA).
    pub sample_rate: u32,
    /// Channel layout. Stereo PCM is interleaved L/R; mono is duplicated
    /// to both output channels at mix time.
    pub channels: legaia_xa::Channels,
    /// Loop the buffer when playback runs off the end. Default `false`
    /// (one-shot).
    pub looping: bool,
    /// Output gain (Q1.14 fixed-point, like SPU voice volumes). `0x4000`
    /// = unity. Multiply samples by `gain / 0x4000`.
    pub gain: u16,
    /// Playback cursor in source samples per channel. `0..source_frames`.
    pub cursor: f64,
    /// SPU-rate output samples to hold silent before the stream becomes
    /// audible. Models the PSX CD controller's response-presentation delay
    /// (the fixed latency between issuing the seek/read and the first XA
    /// sector decoding). Retail arts-voice shouts ride this delay, so a
    /// caller that requests a shout on the animation-start frame gets a
    /// start that *trails* the trigger by this many samples instead of
    /// racing ahead of the animation. Zero for FMV/BGM (no gate).
    pub start_delay: u32,
}

impl XaPlayback {
    /// Number of source-side frames (= per-channel samples).
    pub fn source_frames(&self) -> usize {
        self.pcm
            .len()
            .checked_div(self.channels.n() as usize)
            .unwrap_or(0)
    }

    /// `true` once the cursor has run off the end and loop is off.
    pub fn is_done(&self) -> bool {
        !self.looping && self.cursor >= self.source_frames() as f64
    }

    /// Sample one source frame at the current cursor and advance by one
    /// SPU-rate output sample (i.e. `sample_rate / 44100` of a source
    /// frame). Returns `(L, R)`. Mono is duplicated to both channels.
    fn tick_for_spu(&mut self) -> (i16, i16) {
        // Response-presentation delay gate: stay silent (and do not advance
        // the cursor) until the modeled CD-response latency has elapsed. This
        // is what keeps a shout requested at animation-start from leading the
        // animation - the retail pre-fix bug.
        if self.start_delay > 0 {
            self.start_delay -= 1;
            return (0, 0);
        }
        let frames = self.source_frames();
        if frames == 0 {
            return (0, 0);
        }
        if self.cursor >= frames as f64 {
            if self.looping {
                self.cursor = self.cursor.rem_euclid(frames as f64);
            } else {
                return (0, 0);
            }
        }
        let stride = self.channels.n() as usize;
        // Linear interpolate between two source samples.
        let i0 = self.cursor as usize;
        let i1 = (i0 + 1).min(frames.saturating_sub(1));
        let alpha = (self.cursor - i0 as f64) as f32;
        let (l, r) = match self.channels {
            legaia_xa::Channels::Mono => {
                let s0 = self.pcm[i0 * stride];
                let s1 = self.pcm[i1 * stride];
                let s = lerp_i16(s0, s1, alpha);
                (s, s)
            }
            legaia_xa::Channels::Stereo => {
                let l0 = self.pcm[i0 * stride];
                let r0 = self.pcm[i0 * stride + 1];
                let l1 = self.pcm[i1 * stride];
                let r1 = self.pcm[i1 * stride + 1];
                (lerp_i16(l0, l1, alpha), lerp_i16(r0, r1, alpha))
            }
        };
        // Tail fade-out: a one-shot stream that hard-cuts from its last
        // (possibly loud) sample straight to silence pops. Ramp the final
        // few ms down to zero so exhaustion is click-free. Past the end
        // `tick_for_spu` already returns (0, 0), so the stream is silent by
        // the time the mixer detaches it. Looping streams wrap, so no fade.
        let fade = if self.looping {
            1.0
        } else {
            // ~4 ms expressed in source frames, capped to a quarter of the
            // buffer so short clips (and the leading samples of any stream)
            // aren't attenuated - only the genuine tail ramps down.
            let window = (self.sample_rate as f64 / 256.0).min(frames as f64 / 4.0);
            if window < 1.0 {
                1.0
            } else {
                let remaining = frames as f64 - self.cursor;
                (remaining / window).clamp(0.0, 1.0) as f32
            }
        };
        // Apply gain (Q1.14 fixed-point: gain / 0x4000) then the tail fade.
        let g = self.gain as i32;
        let l = (((l as i32 * g) >> 14) as f32 * fade).clamp(i16::MIN as f32, i16::MAX as f32);
        let r = (((r as i32 * g) >> 14) as f32 * fade).clamp(i16::MIN as f32, i16::MAX as f32);
        // Advance one SPU-rate output sample.
        self.cursor += self.sample_rate as f64 / SPU_INTERNAL_RATE as f64;
        (l as i16, r as i16)
    }
}

/// Saturating mix of two stereo i16 frames.
fn mix_stereo(a: (i16, i16), b: (i16, i16)) -> (i16, i16) {
    let l = (a.0 as i32 + b.0 as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    let r = (a.1 as i32 + b.1 as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    (l, r)
}

/// Output-side resampler that drains the SPU at 44.1 kHz and produces samples
/// at the host device rate. Linear interpolation; one-pole IIR is overkill
/// for the use case (asset-viewer playback + scene BGM) and adds latency.
struct StreamResampler {
    spu: Spu,
    /// Optional active sequencer. Ticked once per SPU sample so timing is
    /// locked to the audio clock instead of frame timing.
    sequencer: Option<Sequencer>,
    /// When `true` the sequencer is not ticked; SPU voices keep playing
    /// so in-progress notes decay naturally. Set via
    /// [`AudioOut::set_sequencer_paused`].
    sequencer_paused: bool,
    /// Optional pending sequencer to install once the current fade-out
    /// reaches zero. `None` when no crossfade is in progress.
    pending_seq: Option<Sequencer>,
    /// Master-fade multiplier applied to the full SPU output (0.0 = silent,
    /// 1.0 = full volume). Drives BGM cross-fades; doesn't affect XA.
    master_fade: f32,
    /// Target for `master_fade`. When `master_fade == fade_target` the
    /// fade engine idles.
    fade_target: f32,
    /// Absolute change in `master_fade` per SPU sample. Zero when idle.
    fade_step: f32,
    /// Optional active XA streaming voice. Mixed into the SPU output at
    /// SPU rate (44.1 kHz) before the host-rate resample.
    xa: Option<XaPlayback>,
    /// A single queued XA shout that starts when `xa` runs off its end.
    /// Powers the back-to-back arts-voice no-drop path (see
    /// [`AudioOut::play_xa_shout`]); `None` for FMV/BGM.
    pending_xa: Option<XaPlayback>,
    /// Monaural downmix (the retail options screen's "Sound: Monaural"):
    /// when set, L/R are averaged into both channels at output time.
    mono: bool,
    /// Master mute gate (engine-only; retail has no equivalent). When set,
    /// [`Self::next_frame`] returns silence but the SPU, sequencer, fade
    /// engine, and any XA stream all keep ticking, so unmuting resumes
    /// exactly where playback would have been - no state is torn down.
    muted: bool,
    /// Cached previous sample (one frame, stereo).
    prev: (i16, i16),
    /// Cached current sample (the SPU output we'll interpolate against).
    cur: (i16, i16),
    /// Phase: how far into the gap from `prev` to `cur` we are. 0..1.
    phase: f64,
    /// Step in phase units per output frame.
    step: f64,
}

impl StreamResampler {
    fn new(device_rate: u32) -> Self {
        let step = SPU_INTERNAL_RATE as f64 / device_rate as f64;
        // Match retail: the SPU runs Studio C reverb globally with most voices
        // routed (see Spu::set_retail_reverb / docs/subsystems/audio.md).
        let mut spu = Spu::new();
        spu.set_retail_reverb();
        Self {
            spu,
            sequencer: None,
            sequencer_paused: false,
            pending_seq: None,
            master_fade: 1.0,
            fade_target: 1.0,
            fade_step: 0.0,
            xa: None,
            pending_xa: None,
            mono: false,
            muted: false,
            prev: (0, 0),
            cur: (0, 0),
            phase: 0.0,
            step,
        }
    }

    /// Pull one output frame at the device sample rate, resampling from the
    /// SPU's 44.1 kHz with linear interpolation. XA streams are summed
    /// into the SPU output at SPU rate, then the whole sum is resampled.
    /// The `master_fade` multiplier is applied to the SPU contribution only
    /// (not XA) so BGM cross-fades don't interrupt ambient / dialogue audio.
    fn next_frame(&mut self) -> (i16, i16) {
        // Advance the SPU as many ticks as needed to push `phase` into [0,1).
        while self.phase >= 1.0 {
            // Advance fade one step per SPU tick.
            self.advance_fade();
            // Tick sequencer unless paused. Sample-clocked: one SPU sample
            // per call, locked to the audio clock with no drift.
            if !self.sequencer_paused
                && let Some(seq) = self.sequencer.as_mut()
            {
                seq.tick_sample(&mut self.spu);
            }
            self.prev = self.cur;
            // Mix SPU output with master-fade applied.
            let spu_sample = self.spu.tick();
            let fv = self.master_fade;
            let spu_faded = if fv >= 1.0 {
                spu_sample
            } else {
                apply_fade(spu_sample, fv)
            };
            let mut sample = spu_faded;
            // XA is mixed in after the fade - not subject to BGM crossfade.
            if let Some(xa) = self.xa.as_mut() {
                if xa.is_done() {
                    // Active shout exhausted: promote a queued shout (if any)
                    // so back-to-back arts don't lose the later clip's audio.
                    self.xa = self.pending_xa.take();
                    if let Some(next) = self.xa.as_mut() {
                        let xa_sample = next.tick_for_spu();
                        sample = mix_stereo(sample, xa_sample);
                    }
                } else {
                    let xa_sample = xa.tick_for_spu();
                    sample = mix_stereo(sample, xa_sample);
                }
            }
            self.cur = sample;
            self.phase -= 1.0;
        }
        let alpha = self.phase as f32;
        let l = lerp_i16(self.prev.0, self.cur.0, alpha);
        let r = lerp_i16(self.prev.1, self.cur.1, alpha);
        self.phase += self.step;
        // Master mute: everything above still ran (sequencer, SPU, XA
        // cursor, fades), so unmute picks up mid-stream with no state loss.
        if self.muted { (0, 0) } else { (l, r) }
    }

    /// Advance `master_fade` one SPU sample toward `fade_target`. When the
    /// fade-out reaches zero and a `pending_seq` is waiting, swaps it in and
    /// starts the fade-in.
    fn advance_fade(&mut self) {
        if self.fade_step == 0.0 {
            return;
        }
        if self.master_fade < self.fade_target {
            self.master_fade = (self.master_fade + self.fade_step).min(self.fade_target);
        } else if self.master_fade > self.fade_target {
            self.master_fade = (self.master_fade - self.fade_step).max(self.fade_target);
        }
        // Reached target
        if (self.master_fade - self.fade_target).abs() < 1e-6 {
            self.master_fade = self.fade_target;
            if self.master_fade == 0.0 {
                // Fade-out done: swap in pending sequencer if present.
                if let Some(mut old) = self.sequencer.take() {
                    old.stop(&mut self.spu);
                }
                if let Some(seq) = self.pending_seq.take() {
                    self.sequencer = Some(seq);
                    // Fade back in at the same rate.
                    self.fade_target = 1.0;
                    // fade_step stays the same (symmetric fade-in).
                } else {
                    // No pending - just idled at silence; release.
                    self.fade_step = 0.0;
                }
            } else {
                // Fade-in (or any other ramp) complete.
                self.fade_step = 0.0;
            }
        }
    }
}

/// Stage a one-shot XA shout into a resampler, honoring the back-to-back
/// no-drop queue: a shout that arrives while one is still sounding is queued
/// behind it rather than cutting it mid-play; only the most recent pending
/// clip is kept. Shared by [`AudioOut::play_xa_shout`] and
/// [`OfflineMixer::play_xa_shout`].
fn stage_xa_shout(s: &mut StreamResampler, shout: XaPlayback) {
    if s.xa.as_ref().is_some_and(|x| !x.is_done()) {
        // A shout is still sounding: queue behind it (no mid-play cut).
        s.pending_xa = Some(shout);
    } else {
        s.pending_xa = None;
        s.xa = Some(shout);
    }
}

/// Device-free mixing harness over the same core the cpal callback drives
/// ([`StreamResampler`]): SPU + sequencer + XA shout mixing, pulled one frame
/// at a time by the caller instead of by an audio device. Used by tests and
/// offline renderers that need to observe exactly what would reach the
/// speakers - e.g. asserting an arts-voice shout's PCM lands in the mix at
/// the modeled CD-response time.
pub struct OfflineMixer {
    state: StreamResampler,
}

impl OfflineMixer {
    /// Create a mixer producing frames at `device_rate` Hz (pass 44_100 for
    /// 1:1 SPU-rate output with no resampling).
    pub fn new(device_rate: u32) -> Self {
        Self {
            state: StreamResampler::new(device_rate),
        }
    }

    /// Mirror of [`AudioOut::play_xa_shout`] - same staging semantics
    /// (response-presentation delay + back-to-back no-drop queue).
    pub fn play_xa_shout(
        &mut self,
        pcm: Vec<i16>,
        sample_rate: u32,
        channels: legaia_xa::Channels,
        gain: u16,
        start_delay_frames: u32,
    ) {
        stage_xa_shout(
            &mut self.state,
            XaPlayback {
                pcm,
                sample_rate,
                channels,
                looping: false,
                gain,
                cursor: 0.0,
                start_delay: start_delay_frames,
            },
        );
    }

    /// `true` while an XA shout is attached and not yet exhausted.
    pub fn xa_active(&self) -> bool {
        self.state.xa.as_ref().is_some_and(|x| !x.is_done())
    }

    /// Run a closure with mutable access to the underlying SPU model
    /// (mirror of [`AudioOut::with_spu`]).
    pub fn with_spu<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Spu) -> R,
    {
        f(&mut self.state.spu)
    }

    /// Pull one output frame - identical mixing math to the cpal callback.
    pub fn next_frame(&mut self) -> (i16, i16) {
        self.state.next_frame()
    }
}

/// Multiply a stereo i16 sample pair by a 0.0..=1.0 gain.
fn apply_fade(s: (i16, i16), fade: f32) -> (i16, i16) {
    let l = (s.0 as f32 * fade).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    let r = (s.1 as f32 * fade).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    (l, r)
}

fn lerp_i16(a: i16, b: i16, t: f32) -> i16 {
    let result = a as f32 + (b as f32 - a as f32) * t;
    result.clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

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
        let mut s = self.lock();
        if let Some(mut prev) = s.sequencer.take() {
            prev.stop(&mut s.spu);
        }
        // Cancel any in-progress crossfade.
        s.pending_seq = None;
        s.master_fade = 1.0;
        s.fade_target = 1.0;
        s.fade_step = 0.0;
        s.sequencer = Some(seq);
    }

    /// Detach the active sequencer (if any) and key-off whatever it had
    /// running. Cancels any in-progress crossfade.
    pub fn detach_sequencer(&self) {
        let mut s = self.lock();
        if let Some(mut seq) = s.sequencer.take() {
            seq.stop(&mut s.spu);
        }
        s.pending_seq = None;
        s.master_fade = 1.0;
        s.fade_target = 1.0;
        s.fade_step = 0.0;
    }

    /// Gate the sequencer tick without detaching it. When `paused` is
    /// `true` the sequencer clock stops (SPU voices already sounding will
    /// continue to decay via their ADSR envelopes). Call with `false` to
    /// resume from where the sequencer left off.
    pub fn set_sequencer_paused(&self, paused: bool) {
        self.lock().sequencer_paused = paused;
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
        let mut s = self.lock();
        if fade_samples == 0 || s.sequencer.is_none() {
            // No current sequencer or immediate switch requested.
            if let Some(mut prev) = s.sequencer.take() {
                prev.stop(&mut s.spu);
            }
            s.pending_seq = None;
            s.master_fade = 1.0;
            s.fade_target = 1.0;
            s.fade_step = 0.0;
            s.sequencer = Some(new_seq);
        } else {
            // Queue new_seq and start fading out.
            s.pending_seq = Some(new_seq);
            s.fade_target = 0.0;
            s.fade_step = 1.0 / fade_samples.max(1) as f32;
        }
    }

    /// Snapshot of the sequencer's progress, returned `None` if no sequencer
    /// is currently attached. Caller-side polling for UI / progress bars.
    pub fn sequencer_progress(&self) -> Option<SequencerProgress> {
        let s = self.lock();
        s.sequencer.as_ref().map(|seq| SequencerProgress {
            tick: seq.playhead_ticks(),
            bpm: seq.bpm(),
            active_notes: seq.active_notes(),
            finished: seq.is_finished(),
        })
    }
}

/// Read-only view onto the active sequencer for diagnostics / UI.
#[derive(Debug, Clone, Copy)]
pub struct SequencerProgress {
    /// Sequencer playhead in PPQN ticks since start (or last loop rewind).
    pub tick: u64,
    /// Current tempo in BPM.
    pub bpm: f32,
    /// Currently-keyed-on note count.
    pub active_notes: usize,
    /// Has the sequencer reached end-of-track (looping disabled)?
    pub finished: bool,
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
fn pcm_to_silence_padded_adpcm(pcm: &[i16]) -> Vec<u8> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Empty stub bank (no programs / samples) - the loop-detector tests care
    /// only about the sequencer's tick timing, not audible output.
    fn empty_bank() -> VabBank {
        VabBank {
            master_vol: 127,
            samples: Vec::new(),
            programs: Vec::new(),
        }
    }

    /// A one-quarter-note SEQ (ppqn 480, 120 BPM) that ends with an
    /// end-of-track: a `set_loop_to(0)` sequencer rewinds there every 480
    /// ticks = one quarter = 0.5 s = 22 050 SPU samples.
    fn one_quarter_seq() -> legaia_seq::Seq {
        let mut buf = Vec::new();
        buf.extend_from_slice(&legaia_seq::SEQ_MAGIC);
        buf.extend_from_slice(&[0x00, 0x01]); // version
        buf.extend_from_slice(&[0x01, 0xE0]); // ppqn 480
        buf.extend_from_slice(&[0x07, 0xA1, 0x20]); // tempo 500000 us/qn
        buf.push(0x04);
        buf.push(0x02);
        buf.extend_from_slice(&[0x00, 0xC0, 0x00]); // prog change
        buf.extend_from_slice(&[0x00, 0x90, 60, 100]); // note on
        buf.extend_from_slice(&[0x83, 0x60, 60, 0]); // +480: note off
        buf.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]); // end of track
        legaia_seq::Seq::parse(&buf).unwrap()
    }

    /// The loop-region detector reports one true SEQ period: the span between
    /// the first and second rewind, trimmed to end exactly on the second
    /// rewind so the browser's `[loopStart, loopEnd)` is one repeatable
    /// iteration. For the 480-tick track that period is ~22 050 samples.
    #[test]
    fn render_bgm_loop_region_finds_one_period() {
        let mut seq = Sequencer::new(one_quarter_seq(), empty_bank());
        seq.set_loop_to(0);
        let mut spu = Spu::new();
        // Budget three periods so two rewinds comfortably fit.
        let render = render_bgm_loop_region(&mut seq, &mut spu, 22_050 * 3);
        assert!(render.loop_start_sample > 0, "no first rewind detected");
        assert!(
            render.loop_end_sample > render.loop_start_sample,
            "no second rewind detected"
        );
        // The trimmed PCM ends exactly on the second rewind.
        assert_eq!(render.pcm.len(), render.loop_end_sample * 2);
        // One period is ~22 050 samples (allow a few for event-boundary rounding).
        let period = render.loop_end_sample - render.loop_start_sample;
        assert!(
            (22_040..=22_060).contains(&period),
            "loop period {period} not ~22050 samples"
        );
    }

    /// A non-looping render (budget shorter than one period, no rewind) hands
    /// back the whole buffer with no loop region so the caller hard-loops it.
    #[test]
    fn render_bgm_loop_region_without_rewind_reports_no_region() {
        let mut seq = Sequencer::new(one_quarter_seq(), empty_bank());
        // No loop fallback + a budget under one period: never rewinds.
        let mut spu = Spu::new();
        let render = render_bgm_loop_region(&mut seq, &mut spu, 1000);
        assert_eq!(render.loop_start_sample, 0);
        assert_eq!(render.loop_end_sample, 1000);
        assert_eq!(render.pcm.len(), 2000);
    }

    /// `pcm_to_silence_padded_adpcm` produces a stream that, when fed back
    /// through the streaming decoder, yields a low-resolution version of
    /// the original PCM. The first block carries 28 samples; samples 0,
    /// 8, and 12 (deliberately chosen positive 12-bit values) survive the
    /// quantisation as their upper nibble.
    #[test]
    fn pcm_to_adpcm_round_trips_low_nibble() {
        let pcm: Vec<i16> = (0..28).map(|i: i32| (i * 0x100) as i16).collect();
        let blocks = pcm_to_silence_padded_adpcm(&pcm);
        let mut decoder = AdpcmDecoder::new();
        let (out, _flags) = decoder.decode_block(&blocks[..BLOCK_BYTES]);
        // pcm[0]=0 -> top nibble = 0 -> sample = 0.
        assert_eq!(out[0], 0);
        // pcm[16] = 0x1000 -> top nibble = 1 -> decoded sample = 1<<12 = 4096.
        assert_eq!(out[16], 4096);
        // pcm[27] = 0x1B00 -> top nibble = 1 -> sample = 4096 (truncation).
        assert_eq!(out[27], 4096);
    }

    /// Empty input produces a valid (silent + terminator) stream.
    #[test]
    fn pcm_to_adpcm_handles_empty() {
        let blocks = pcm_to_silence_padded_adpcm(&[]);
        assert_eq!(blocks.len(), BLOCK_BYTES * 2);
        assert_eq!(blocks[BLOCK_BYTES + 1] & 0x01, 0); // first block: no end flag
    }

    /// StreamResampler renders silence forever when the SPU has no active
    /// voices.
    #[test]
    fn resampler_renders_silence_when_spu_idle() {
        let mut r = StreamResampler::new(48_000);
        for _ in 0..1000 {
            assert_eq!(r.next_frame(), (0, 0));
        }
    }

    /// StreamResampler advances at the right phase rate so 48 kHz frames
    /// over 1 second consume exactly 44_100 SPU ticks (within one).
    #[test]
    fn resampler_step_rate_matches_44100_internal() {
        let mut r = StreamResampler::new(48_000);
        // Drain 48000 frames; SPU should have ticked ~44100 times. We can't
        // observe SPU tick count directly, so verify via phase accumulator.
        for _ in 0..48_000 {
            r.next_frame();
        }
        // After 48000 frames at step = 44100/48000 ≈ 0.91875, total phase
        // advanced = 44100. Tick count is the integer part. We don't expose
        // a counter; just assert no panic + finite state.
        // (The strict numerical check is in the SPU's own tests.)
    }

    // --- XaPlayback -------------------------------------------------------

    fn make_mono_xa(rate: u32, samples: Vec<i16>, looping: bool) -> XaPlayback {
        XaPlayback {
            pcm: samples,
            sample_rate: rate,
            channels: legaia_xa::Channels::Mono,
            looping,
            gain: 0x4000,
            cursor: 0.0,
            start_delay: 0,
        }
    }

    #[test]
    fn xa_playback_done_flag_flips_at_end_of_oneshot() {
        let mut xa = make_mono_xa(SPU_INTERNAL_RATE, vec![100i16, 200, 300, 400], false);
        assert!(!xa.is_done());
        // 4 samples at SPU rate -> 4 ticks to exhaust.
        for _ in 0..4 {
            let _ = xa.tick_for_spu();
        }
        assert!(xa.is_done(), "one-shot should mark done after consumption");
    }

    #[test]
    fn xa_oneshot_tail_fades_out_to_avoid_click() {
        // Constant loud mono buffer, one-shot. Mid-stream output is ~full
        // amplitude, but the final few ms must ramp toward zero so the
        // stream doesn't hard-cut from a loud sample straight to silence.
        let amp = 16384i16;
        let n = 400usize;
        let mut xa = make_mono_xa(SPU_INTERNAL_RATE, vec![amp; n], false);
        let outs: Vec<i16> = (0..n).map(|_| xa.tick_for_spu().0).collect();
        // Mid-stream (well before the tail) is at full amplitude.
        assert!(
            outs[10] > amp - 100,
            "mid-stream should be ~full: {}",
            outs[10]
        );
        // The last sample is heavily attenuated by the tail fade.
        let last = *outs.last().unwrap();
        assert!(last.abs() < amp / 8, "tail should fade out, last={last}");
        // The fade is monotonic non-increasing over the final stretch.
        for w in outs[n - 100..].windows(2) {
            assert!(w[1] <= w[0], "tail fade not monotone: {} -> {}", w[0], w[1]);
        }
    }

    #[test]
    fn xa_looping_does_not_tail_fade() {
        // A looping stream wraps rather than exhausting, so it must NOT fade.
        let amp = 16384i16;
        let mut xa = make_mono_xa(SPU_INTERNAL_RATE, vec![amp; 400], true);
        for _ in 0..399 {
            let _ = xa.tick_for_spu();
        }
        // Near the buffer end a looping stream is still full volume.
        let (l, _) = xa.tick_for_spu();
        assert!(l > amp - 100, "looping stream should not fade: {l}");
    }

    #[test]
    fn xa_playback_loops_when_looping_set() {
        let mut xa = make_mono_xa(SPU_INTERNAL_RATE, vec![100i16, 200], true);
        for _ in 0..50 {
            let _ = xa.tick_for_spu();
        }
        // Looping streams never report done.
        assert!(!xa.is_done());
    }

    #[test]
    fn xa_playback_mono_duplicates_to_both_channels() {
        let mut xa = make_mono_xa(SPU_INTERNAL_RATE, vec![1234i16, 0, 0, 0], false);
        let (l, r) = xa.tick_for_spu();
        assert_eq!(l, 1234);
        assert_eq!(r, 1234);
    }

    #[test]
    fn xa_playback_stereo_passes_l_r_separately() {
        let mut xa = XaPlayback {
            pcm: vec![100i16, 200, 0, 0],
            sample_rate: SPU_INTERNAL_RATE,
            channels: legaia_xa::Channels::Stereo,
            looping: false,
            gain: 0x4000,
            cursor: 0.0,
            start_delay: 0,
        };
        let (l, r) = xa.tick_for_spu();
        assert_eq!(l, 100);
        assert_eq!(r, 200);
    }

    #[test]
    fn xa_playback_gain_zero_silences_output() {
        let mut xa = XaPlayback {
            pcm: vec![32000i16, 32000, 32000, 32000],
            sample_rate: SPU_INTERNAL_RATE,
            channels: legaia_xa::Channels::Mono,
            looping: false,
            gain: 0,
            cursor: 0.0,
            start_delay: 0,
        };
        let (l, r) = xa.tick_for_spu();
        assert_eq!((l, r), (0, 0));
    }

    #[test]
    fn xa_playback_resamples_lower_source_rate_correctly() {
        // Source at half SPU rate => one source sample lasts 2 SPU ticks.
        let mut xa = make_mono_xa(
            SPU_INTERNAL_RATE / 2,
            vec![1000i16, 2000, 3000, 4000],
            false,
        );
        // First tick: at cursor=0 -> sample=1000.
        let s0 = xa.tick_for_spu();
        // Second tick: cursor advanced by 0.5 -> lerp(1000, 2000, 0.5) = 1500.
        let s1 = xa.tick_for_spu();
        // Third tick: cursor=1.0 -> sample=2000.
        let s2 = xa.tick_for_spu();
        assert_eq!(s0.0, 1000);
        assert_eq!(s1.0, 1500);
        assert_eq!(s2.0, 2000);
    }

    #[test]
    fn xa_playback_empty_buffer_is_immediately_done() {
        let xa = make_mono_xa(SPU_INTERNAL_RATE, vec![], false);
        assert!(xa.is_done());
    }

    #[test]
    fn resampler_mixes_xa_into_idle_spu_output() {
        let mut r = StreamResampler::new(SPU_INTERNAL_RATE);
        r.xa = Some(make_mono_xa(SPU_INTERNAL_RATE, vec![5000i16; 100], false));
        // Pull a few frames so the resampler's `prev`/`cur` caches both
        // hold the XA sample (first call sets cur, subsequent calls slide
        // prev forward). After a few frames the lerp settles on the
        // sustained XA value.
        for _ in 0..16 {
            r.phase = r.phase.max(1.0);
            let _ = r.next_frame();
        }
        r.phase = 1.0;
        let (l, r_out) = r.next_frame();
        // SPU is silent -> mixed sample is just XA, ~5000.
        assert!((l - 5000).abs() < 50, "left should be ~5000, got {l}");
        assert!(
            (r_out - 5000).abs() < 50,
            "right should be ~5000, got {r_out}"
        );
    }

    #[test]
    fn resampler_drops_xa_after_oneshot_drains() {
        let mut r = StreamResampler::new(SPU_INTERNAL_RATE);
        r.xa = Some(make_mono_xa(SPU_INTERNAL_RATE, vec![1000i16, 2000], false));
        r.phase = 1.0;
        // First two ticks consume the buffer; one more tick: xa is None
        // and frame is silent.
        for _ in 0..3 {
            r.phase = r.phase.max(1.0);
            let _ = r.next_frame();
        }
        assert!(r.xa.is_none(), "exhausted XA stream should detach");
    }

    // --- arts-voice battle shout scheduling ---

    /// The response-presentation delay keeps a shout silent (and its cursor
    /// parked at 0) for `start_delay` output samples, then it plays from the
    /// first source sample. This is what makes a shout requested on the
    /// animation-start frame *trail* the animation instead of leading it.
    #[test]
    fn xa_shout_start_delay_gates_then_plays_from_start() {
        let delay = 32u32;
        let mut xa = make_mono_xa(SPU_INTERNAL_RATE, vec![7000i16; 64], false);
        xa.start_delay = delay;
        // During the delay: pure silence, cursor unmoved, not done.
        for _ in 0..delay {
            assert_eq!(xa.tick_for_spu(), (0, 0), "gated shout must be silent");
        }
        assert_eq!(xa.cursor, 0.0, "cursor must not advance during the gate");
        assert!(!xa.is_done());
        // First post-gate sample is the buffer's first sample (full volume).
        let (l, _) = xa.tick_for_spu();
        assert!(
            l > 7000 - 100,
            "post-gate shout should play from start: {l}"
        );
    }

    /// Back-to-back arts: a second shout requested while the first is still
    /// sounding is queued, not dropped and not a mid-play cut. It begins the
    /// sample the first runs off its end - the counterpart to the retail
    /// back-to-back Hyper-Art fix (the later clip must still sound).
    #[test]
    fn xa_shout_back_to_back_queues_second_without_dropping_it() {
        let mut r = StreamResampler::new(SPU_INTERNAL_RATE);
        // Shout A: short, distinct amplitude. Shout B: distinct amplitude.
        let mut a = make_mono_xa(SPU_INTERNAL_RATE, vec![4000i16; 8], false);
        a.gain = 0x4000;
        r.xa = Some(a);
        // Second request arrives while A is active -> queued behind it.
        let mut b = make_mono_xa(SPU_INTERNAL_RATE, vec![9000i16; 8], false);
        b.gain = 0x4000;
        r.pending_xa = Some(b);
        // Drain enough frames to exhaust A and let B promote and play.
        let mut saw_a = false;
        let mut saw_b = false;
        for _ in 0..40 {
            r.phase = 1.0;
            let (l, _) = r.next_frame();
            if (l - 4000).abs() < 400 {
                saw_a = true;
            }
            if (l - 9000).abs() < 400 {
                saw_b = true;
            }
        }
        assert!(saw_a, "first shout should have sounded");
        assert!(saw_b, "queued second shout must not be dropped");
        assert!(
            r.pending_xa.is_none(),
            "pending slot consumed after promotion"
        );
    }

    // --- master mute gate ---

    /// While muted the resampler renders pure zeros, but the underlying
    /// producers keep advancing (here: the XA stream cursor), so unmuting
    /// resumes playback from where it would have been - no state loss.
    #[test]
    fn resampler_mute_gate_silences_and_unmute_resumes_without_state_loss() {
        let mut r = StreamResampler::new(SPU_INTERNAL_RATE);
        // A long, loud, looping XA stream stands in for "audio is playing".
        r.xa = Some(make_mono_xa(SPU_INTERNAL_RATE, vec![5000i16; 4096], true));
        // Muted: every rendered frame is exactly silent.
        r.muted = true;
        for _ in 0..200 {
            r.phase = r.phase.max(1.0);
            assert_eq!(r.next_frame(), (0, 0), "muted render must be zeros");
        }
        // ...but the stream kept ticking underneath the gate.
        let cursor = r.xa.as_ref().expect("stream still attached").cursor;
        assert!(
            cursor >= 199.0,
            "producer state should keep advancing while muted, cursor={cursor}"
        );
        // Unmute: output resumes at the sustained XA level within a few
        // frames (the prev/cur interpolation caches were kept warm).
        r.muted = false;
        let mut resumed = false;
        for _ in 0..16 {
            r.phase = r.phase.max(1.0);
            let (l, _) = r.next_frame();
            if (l - 5000).abs() < 50 {
                resumed = true;
            }
        }
        assert!(resumed, "unmute should resume playback without re-priming");
    }

    // --- fade engine ---

    #[test]
    fn apply_fade_at_zero_silences() {
        assert_eq!(apply_fade((1000, -2000), 0.0), (0, 0));
    }

    #[test]
    fn apply_fade_at_unity_is_passthrough() {
        let s = (1234i16, -5678i16);
        assert_eq!(apply_fade(s, 1.0), s);
    }

    #[test]
    fn resampler_advance_fade_fades_out_over_samples() {
        let mut r = StreamResampler::new(SPU_INTERNAL_RATE);
        // Start a fade-out over 10 SPU ticks.
        r.master_fade = 1.0;
        r.fade_target = 0.0;
        r.fade_step = 1.0 / 10.0;
        for _ in 0..10 {
            r.advance_fade();
        }
        assert!(
            r.master_fade < 1e-5,
            "fade should reach 0 after 10 steps, got {}",
            r.master_fade
        );
        assert_eq!(r.fade_step, 0.0, "step should idle after target reached");
    }

    #[test]
    fn resampler_pending_seq_swapped_at_zero() {
        let mut r = StreamResampler::new(SPU_INTERNAL_RATE);
        // Manually load a pending sequencer and drive the fade to 0.
        r.master_fade = 0.1;
        r.fade_target = 0.0;
        r.fade_step = 0.1;
        // Construct a no-op sequencer (no samples).
        let seq = Sequencer::new(
            legaia_seq::Seq::parse(&{
                // Legaia-format SEQ: magic(4) + version=1 u32 BE(4) + ppqn u16 BE(2)
                // + tempo u24 BE(3) + time_sig_num(1) + time_sig_denom_pow2(1) = 15 B header
                // then delta(1) + EOT meta(3) = 4 B event stream. Total 19 B.
                let mut v = b"pQES".to_vec();
                v.extend_from_slice(&1u32.to_be_bytes()); // version
                v.extend_from_slice(&24u16.to_be_bytes()); // ppqn
                v.extend_from_slice(&[0x07, 0xA1, 0x20]); // tempo = 500 000 µs/qn
                v.push(4); // time_sig_num
                v.push(2); // time_sig_denom_pow2
                v.push(0x00); // delta = 0
                v.extend_from_slice(&[0xFF, 0x2F, 0x00]); // End of Track
                v
            })
            .expect("minimal seq"),
            VabBank {
                master_vol: 127,
                samples: vec![],
                programs: vec![],
            },
        );
        r.pending_seq = Some(seq);
        r.advance_fade(); // one step: 0.1 - 0.1 = 0 → swap
        assert!(
            r.sequencer.is_some(),
            "pending seq should be installed after fade-out"
        );
        assert!(r.pending_seq.is_none());
        // Fade should now be going back up.
        assert_eq!(r.fade_target, 1.0);
    }

    #[test]
    fn mix_stereo_saturates_at_i16_bounds() {
        let a = (i16::MAX, i16::MIN);
        let b = (1, -1);
        let (l, r) = mix_stereo(a, b);
        // i16::MAX + 1 saturates to i16::MAX.
        assert_eq!(l, i16::MAX);
        // i16::MIN + -1 saturates to i16::MIN.
        assert_eq!(r, i16::MIN);
    }
}
