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

    /// Convenience: play `pcm` (mono i16, at `input_rate`) as a one-shot.
    /// Synthesises a single SPU-ADPCM-shaped pseudo-block by injecting the
    /// PCM as a "raw" voice that bypasses the ADPCM stage.
    ///
    /// NOTE: this path is for the asset viewer's "preview decoded VAG WAV
    /// without re-encoding" use case. The full SPU mixer is the production
    /// path; see [`AudioSink::with_spu`] for that.
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
}

impl AudioSink for AudioOut {
    fn with_core<R>(&self, f: impl FnOnce(&mut StreamResampler) -> R) -> R {
        f(&mut self.lock())
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
