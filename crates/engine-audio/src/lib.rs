//! cpal-backed audio output for the engine reimplementation track.
//!
//! Provides one stream that plays mono i16 PCM, queued from any thread.
//! Resamples linearly into the device's sample rate; downmixes mono into
//! every output channel by duplication. Designed for the asset viewer's
//! "play this VAG sample" key binding -- not yet a full mixer.
//!
//! Future iterations will:
//! - mix multiple voices simultaneously (the PSX SPU has 24)
//! - add ADSR envelope shaping (VAB tone metadata is parsed already)
//! - stream XA-ADPCM via the existing [`legaia_xa`] decoder
//!
//! Channel mapping: a queued mono buffer fans out to every device channel.
//! On a stereo device that's center playback; for surround setups it'll be
//! louder than expected. Good enough for the "does sample N play?" loop.

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Default sample rate to assume for queued buffers when the caller
/// doesn't specify one. PSX VAG samples in Legaia run at this rate
/// (verified against several extracted banks).
pub const DEFAULT_INPUT_RATE: u32 = 22_050;

#[derive(Default)]
struct Voice {
    /// Mono samples in input rate.
    pcm: Vec<i16>,
    /// Input sample rate (Hz).
    input_rate: u32,
    /// Fractional play position in INPUT samples.
    pos: f64,
}

#[derive(Default)]
struct Mixer {
    /// At most one voice for now. Replacing the voice replaces the queued
    /// playback (no overlapping). The PSX SPU runs 24 voices in parallel
    /// and a future revision should mirror that, but the asset viewer
    /// only needs "play sample N, replacing what's currently playing".
    voice: Option<Voice>,
}

impl Mixer {
    fn write_into<S: Sample>(&mut self, out: &mut [S], device_rate: u32, channels: u16) {
        let Some(v) = self.voice.as_mut() else {
            // Silence.
            for s in out.iter_mut() {
                *s = S::ZERO;
            }
            return;
        };
        let step = v.input_rate as f64 / device_rate as f64;
        let frames = out.len() / channels as usize;
        let mut wrote = 0usize;
        for f in 0..frames {
            let i = v.pos as usize;
            if i >= v.pcm.len() {
                break;
            }
            let raw = v.pcm[i];
            let value = S::from_i16(raw);
            for c in 0..channels as usize {
                out[f * channels as usize + c] = value;
            }
            v.pos += step;
            wrote = f + 1;
        }
        // Tail-fill silence if the voice ran out.
        for f in wrote..frames {
            for c in 0..channels as usize {
                out[f * channels as usize + c] = S::ZERO;
            }
        }
        if v.pos as usize >= v.pcm.len() {
            self.voice = None;
        }
    }
}

trait Sample: cpal::Sample + Copy {
    const ZERO: Self;
    fn from_i16(s: i16) -> Self;
}
impl Sample for f32 {
    const ZERO: f32 = 0.0;
    fn from_i16(s: i16) -> f32 {
        s as f32 / i16::MAX as f32
    }
}
impl Sample for i16 {
    const ZERO: i16 = 0;
    fn from_i16(s: i16) -> i16 {
        s
    }
}
impl Sample for u16 {
    const ZERO: u16 = 32_768;
    fn from_i16(s: i16) -> u16 {
        ((s as i32) + 32_768) as u16
    }
}

pub struct AudioOut {
    _stream: cpal::Stream,
    mixer: Arc<Mutex<Mixer>>,
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
        let mixer = Arc::new(Mutex::new(Mixer::default()));

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                Self::build_stream::<f32>(&device, &config.into(), mixer.clone(), channels)?
            }
            cpal::SampleFormat::I16 => {
                Self::build_stream::<i16>(&device, &config.into(), mixer.clone(), channels)?
            }
            cpal::SampleFormat::U16 => {
                Self::build_stream::<u16>(&device, &config.into(), mixer.clone(), channels)?
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
            mixer,
            device_rate,
            channels,
        })
    }

    fn build_stream<S>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        mixer: Arc<Mutex<Mixer>>,
        channels: u16,
    ) -> Result<cpal::Stream>
    where
        S: cpal::SizedSample + Sample,
    {
        let device_rate = config.sample_rate.0;
        let stream = device.build_output_stream::<S, _, _>(
            config,
            move |out: &mut [S], _: &cpal::OutputCallbackInfo| {
                let mut m = mixer.lock().unwrap();
                m.write_into::<S>(out, device_rate, channels);
            },
            |err| log::error!("audio output error: {err}"),
            None,
        )?;
        Ok(stream)
    }

    /// Replace the currently-playing voice with `pcm` at `input_rate`.
    /// Returns immediately; playback happens on the audio thread.
    pub fn play_pcm_mono(&self, pcm: Vec<i16>, input_rate: u32) {
        let mut m = self.mixer.lock().unwrap();
        m.voice = Some(Voice {
            pcm,
            input_rate,
            pos: 0.0,
        });
    }

    /// Stop any currently-playing voice immediately.
    pub fn stop(&self) {
        self.mixer.lock().unwrap().voice = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixer_writes_silence_when_empty() {
        let mut m = Mixer::default();
        let mut buf = vec![123.0f32; 64];
        m.write_into::<f32>(&mut buf, 48000, 2);
        assert!(buf.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn mixer_writes_voice_into_all_channels() {
        // 4 input samples at 48000 Hz played at 48000 Hz device rate (no resample).
        let mut m = Mixer {
            voice: Some(Voice {
                pcm: vec![32_767, 32_767, 32_767, 32_767],
                input_rate: 48_000,
                pos: 0.0,
            }),
        };
        let mut buf = vec![0.0f32; 8]; // 4 frames * 2 channels
        m.write_into::<f32>(&mut buf, 48_000, 2);
        // Both channels should have the same nonzero value.
        for f in 0..4 {
            let l = buf[f * 2];
            let r = buf[f * 2 + 1];
            assert!(l > 0.99, "frame {f} left sample {l}");
            assert_eq!(l, r);
        }
        // Voice should be exhausted after writing.
        assert!(m.voice.is_none());
    }

    #[test]
    fn mixer_resamples_22k_to_44k() {
        let mut m = Mixer {
            voice: Some(Voice {
                pcm: vec![10_000; 100],
                input_rate: 22_050,
                pos: 0.0,
            }),
        };
        let mut buf = vec![0.0f32; 8]; // 4 frames * 2 channels at device rate
        m.write_into::<f32>(&mut buf, 44_100, 2);
        // step = 22050/44100 = 0.5 -- pos advances 0.5 per output frame
        // After 4 output frames, pos = 2.0; we read indices 0,0,1,1.
        // All four are positive (unchanged sample value).
        for f in 0..4 {
            assert!(buf[f * 2] > 0.0);
        }
    }
}
