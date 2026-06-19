//! PCM-window oracle plumbing - I2 of Phase I.
//!
//! Builds on top of [`crate::audio_trace_oracle`] by emitting **mixed PCM**
//! windows rather than per-frame voice-activity masks. The voice-mask
//! oracle catches "engine never key-on'd the right voices"; the PCM oracle
//! catches mixer-level regressions (vibrato envelope shape, master volume
//! application, ADPCM decoder drift) that leave the active-voice set
//! identical but the audible output different.
//!
//! ## Shape
//!
//! - [`engine_spu_from_retail`] translates a
//!   [`legaia_mednafen::PsxSpu`] snapshot into a freshly-seeded
//!   [`legaia_engine_audio::Spu`]. The 512 KiB SPU RAM blob is copied
//!   verbatim and the per-voice static config (start_addr / loop_addr /
//!   pitch / vol_left / vol_right) is mirrored. Voices whose retail ADSR
//!   phase is non-zero get a synthetic [`Voice::key_on`] so they start
//!   playing from the beginning of their sample.
//! - [`retail_reference_pcm`] is the convenience wrapper: load a
//!   mednafen save file, translate the snapshot, render N stereo
//!   samples. The resulting buffer is "what the SPU would emit given
//!   this snapshot's voice config" - **not** a bit-identical
//!   resume-from-snapshot, because the engine-audio [`Voice`] doesn't
//!   expose its mid-stream playback state (current ADPCM block
//!   pointer, sample fractional position, ADSR runtime phase). Each
//!   keyed-on voice rewinds to its start address with a fresh Attack
//!   envelope.
//! - [`build_engine_pcm_trace`] mirrors
//!   [`crate::audio_trace_oracle::build_engine_audio_trace`] - it boots a
//!   [`crate::BootSession`], runs a private headless SPU + sequencer in
//!   parallel, routes field-VM BGM events through a
//!   [`crate::audio_trace_oracle::TraceBgmDirector`], and per-frame
//!   accumulates the rendered PCM into a returned buffer. The retail
//!   side renders from the snapshot via [`retail_reference_pcm`]; the
//!   diff axis is "did these two SPUs produce similar audio".
//!
//! ## Tolerance
//!
//! PCM byte-exact convergence is **not** the goal of this oracle. Two
//! sources of expected drift:
//!
//! 1. The retail snapshot is one SPU cycle; the engine starts at boot.
//!    BGM ramp / cross-fade timing alignment between the two is
//!    accidental at best.
//! 2. The engine-audio mixer is not bit-identical to mednafen's PSX SPU.
//!    Different ADPCM implementations, different envelope time bases,
//!    different reverb topology.
//!
//! [`first_pcm_divergence`] therefore takes a per-sample tolerance band
//! and falls back to RMS-of-difference reporting through [`PcmStats`].
//! The disc-gated test treats anything short of "engine produced
//! silence while retail had audible audio" as soft-tolerable drift, the
//! same shape [`crate::audio_trace_oracle`]'s test treats
//! `NoFrameMatched`.

use std::path::Path;

use anyhow::{Context, Result};
use legaia_engine_audio::{Spu, SpuAllocator, VabBank, spu::reverb::ReverbMode};
use legaia_engine_core::scene::BgmDirector;
use legaia_mednafen::{PsxSpu, SaveState};

use crate::audio_trace_oracle::{
    AudioTraceBuildOptions, AudioTraceFrame, TraceBgmDirector, sample_engine_frame_for_pcm,
};
use crate::{BootConfig, BootSession};

/// PSX SPU internal output rate (Hz). [`Spu::tick`] yields one stereo
/// sample at this rate.
pub const SPU_SAMPLE_RATE: u32 = 44_100;

/// Stereo i16 PCM samples = 2 channels per frame.
const CHANNELS: usize = 2;

/// Translate a retail [`PsxSpu`] snapshot into a freshly-seeded engine
/// [`Spu`].
///
/// Returns `None` when the save state doesn't carry an `SPURAM` entry
/// (some non-PSX mednafen modules).
///
/// **What gets mirrored:**
///   - 512 KiB SPU RAM (verbatim copy via
///     [`legaia_engine_audio::spu::ram::SpuRam::write_at`]).
///   - Master volume left/right (mednafen's accumulated
///     `(GlobalSweep[0/1]).Current`).
///   - Reverb mode (low byte of the raw u32 register, mapped via
///     [`ReverbMode::from_byte`]).
///   - Per-voice: `start_addr`, `loop_addr`, `pitch`, `vol_left`,
///     `vol_right`. A synthetic [`legaia_engine_audio::spu::voice::Voice::key_on`]
///     is issued for voices whose retail ADSR phase is non-zero.
///
/// **What does NOT get mirrored** (engine-audio doesn't expose the
/// state):
///   - Voice mid-stream playback position (current ADPCM block, sample
///     fractional offset, current envelope level).
///   - ADSR runtime phase (every active voice starts in Attack).
///   - Voice-on/-off pending masks (snapshot is a freeze frame; the
///     translator emits a key_on for everything retail had audible).
///   - SPU control register (`SPUCNT`).
///   - Reverb buffer state.
pub fn engine_spu_from_retail(psx_spu: &PsxSpu<'_>) -> Option<Spu> {
    let ram_bytes = psx_spu.spu_ram_bytes()?;

    let mut spu = Spu::new();
    spu.ram.write_at(0, ram_bytes);

    if let Some((l, r)) = psx_spu.master_volume() {
        spu.master_left = l;
        spu.master_right = r;
    }
    // Identify the reverb preset from the captured coefficient registers
    // rather than mednafen's `Reverb_Mode` sub-entry - that field is actually
    // the per-voice reverb-enable mask (EON), not a libspu mode byte, so
    // `from_byte(reverb_mode())` previously mis-mapped the busy EON value to
    // `Off` and the retail-side PCM never had reverb. Retail runs Studio C
    // globally (see docs/subsystems/audio.md).
    if let Some(mode) = psx_spu
        .reverb_registers()
        .and_then(|r| ReverbMode::identify(&r))
    {
        spu.reverb_mode_raw = psx_spu.reverb_mode().unwrap_or(0);
        if psx_spu.reverb_master_enabled().unwrap_or(true) {
            spu.set_reverb_mode(mode);
        }
    }
    let reverb_mask = psx_spu.voice_reverb_mask().unwrap_or(0);

    let retail_voices = psx_spu.voices();
    let mut key_on_mask = 0u32;
    for (i, rv) in retail_voices.iter().enumerate() {
        let v = &mut spu.voices[i];
        v.set_reverb_send(reverb_mask & (1u32 << i) != 0);
        if let Some(addr) = rv.start_addr {
            v.start_addr = addr;
        }
        if let Some(addr) = rv.loop_addr {
            v.loop_addr = Some(addr);
        }
        if let Some(pitch) = rv.pitch {
            v.pitch = pitch;
        }
        if let Some(vl) = rv.vol_left {
            v.vol_left = vl;
        }
        if let Some(vr) = rv.vol_right {
            v.vol_right = vr;
        }
        if rv.is_active() {
            key_on_mask |= 1u32 << i;
        }
    }
    spu.key_on_mask(key_on_mask);

    Some(spu)
}

/// Render `samples_per_channel` stereo samples from `spu` by repeatedly
/// ticking the mixer. Returns interleaved L,R `Vec<i16>` of length
/// `2 * samples_per_channel`.
pub fn render_pcm_window(spu: &mut Spu, samples_per_channel: usize) -> Vec<i16> {
    let mut out = vec![0i16; samples_per_channel * CHANNELS];
    spu.render_into(&mut out);
    out
}

/// Build the retail-side reference PCM for a save state.
///
/// Translates the save's `SPU` section via [`engine_spu_from_retail`]
/// and renders `samples_per_channel` stereo samples through the engine
/// mixer. Returns an empty PCM when the save state has no SPU section
/// (the audio-trace oracle's documented "tolerable drift" mode), so
/// downstream code can keep the same skip-pass shape as
/// `load_runtime_audio_trace_from_save`.
pub fn retail_reference_pcm(save_path: &Path, samples_per_channel: usize) -> Result<Vec<i16>> {
    let state = SaveState::from_path(save_path)
        .with_context(|| format!("load mednafen save {}", save_path.display()))?;
    let psx_spu = PsxSpu::new(&state);
    let Some(mut spu) = engine_spu_from_retail(&psx_spu) else {
        return Ok(Vec::new());
    };
    Ok(render_pcm_window(&mut spu, samples_per_channel))
}

/// Combined result of a PCM-trace boot: per-frame voice-activity (same
/// shape as the audio-trace oracle's [`AudioTraceFrame`]) plus the
/// rendered PCM accumulated over the trace window.
#[derive(Debug, Clone)]
pub struct EnginePcmTrace {
    /// Per-frame voice-activity snapshots. `frames + 1` entries (one
    /// pre-tick, one per tick) for parity with
    /// [`crate::audio_trace_oracle::build_engine_audio_trace`].
    pub frames: Vec<AudioTraceFrame>,
    /// Interleaved L,R i16 stereo at 44.1 kHz. Length =
    /// `2 * samples_per_frame * opts.frames` where `samples_per_frame`
    /// is `44_100 * us_per_frame / 1_000_000`.
    pub pcm: Vec<i16>,
    /// Stereo samples emitted per engine frame. Useful for slicing
    /// `pcm` into per-frame windows after the fact.
    pub samples_per_frame: usize,
}

/// Boot the scene, run a private headless SPU + sequencer, and emit
/// both the per-frame voice trace AND the per-frame rendered PCM. The
/// retail comparand for PCM is [`retail_reference_pcm`].
///
/// Sibling of
/// [`crate::audio_trace_oracle::build_engine_audio_trace`]. The trace
/// loops are structurally identical - both call [`BootSession::tick`],
/// route BGM events through a [`TraceBgmDirector`], tick the
/// sequencer, and advance the SPU by one frame of samples. The
/// difference is that this function keeps the rendered samples
/// instead of discarding the sink.
pub fn build_engine_pcm_trace(
    extracted_root: &Path,
    disc: Option<&Path>,
    opts: &AudioTraceBuildOptions,
) -> Result<EnginePcmTrace> {
    let cfg = BootConfig {
        scene: opts.scene.clone(),
        enable_audio: false,
    };
    let mut session = match disc {
        Some(p) => BootSession::open_disc(p, &cfg)?,
        None => BootSession::open(extracted_root, &cfg)?,
    };

    let mut spu = Spu::new();
    let mut director = TraceBgmDirector::new();
    if let Some(vab_bytes) = session
        .host
        .scene_vab_bytes()
        .context("resolve scene VAB bytes")?
    {
        let report =
            legaia_vab::parse(&vab_bytes, 0).context("parse scene VAB header for PCM trace")?;
        const SPU_RAM_BYTES: u32 = 512 * 1024;
        const SPU_RESERVED_BYTES: u32 = 0x1000;
        let mut alloc = SpuAllocator::new(SPU_RESERVED_BYTES, SPU_RAM_BYTES - SPU_RESERVED_BYTES);
        let bank = VabBank::upload(&mut spu, &mut alloc, &report, &vab_bytes);
        director.set_bank(bank);
    }

    if let Some(id) = opts.bgm_id {
        if let Some(seq_bytes) = session.host.bgm_seq_bytes(id)? {
            director.start(id, &seq_bytes);
        } else {
            log::warn!("pcm-trace: bgm_id {id} did not resolve to a SEQ entry");
        }
    }

    let samples_per_frame = (SPU_SAMPLE_RATE as f64 * (opts.us_per_frame / 1_000_000.0)) as usize;
    let total_samples = samples_per_frame * (opts.frames as usize);
    let mut frames = Vec::with_capacity((opts.frames as usize).saturating_add(1));
    let mut pcm = Vec::with_capacity(total_samples * CHANNELS);

    frames.push(sample_engine_frame_for_pcm(
        &session,
        &spu,
        director.sequencer(),
    ));
    let mut sink = vec![0i16; samples_per_frame * CHANNELS];
    for _ in 0..opts.frames {
        let _ = session.tick()?;
        let _ = session.host.route_bgm_events(&mut director)?;
        if !director.is_paused()
            && let Some(seq) = director.sequencer_mut()
        {
            seq.tick_us(&mut spu, opts.us_per_frame);
        }
        spu.render_into(&mut sink);
        pcm.extend_from_slice(&sink);
        frames.push(sample_engine_frame_for_pcm(
            &session,
            &spu,
            director.sequencer(),
        ));
    }
    Ok(EnginePcmTrace {
        frames,
        pcm,
        samples_per_frame,
    })
}

/// Summary statistics for a PCM buffer. Computed in i64 to avoid
/// overflow on large windows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PcmStats {
    /// Number of stereo sample pairs (i.e. `buf.len() / 2`).
    pub sample_pairs: usize,
    /// Peak absolute amplitude across both channels (0..=32767).
    pub peak_abs: i32,
    /// Root-mean-square amplitude across both channels (i32 to keep the
    /// caller's arithmetic mixed-precision-friendly; 0..=32767).
    pub rms: i32,
    /// Number of i16 samples whose absolute value is `>= silence_eps`.
    /// Cheap "is this buffer audibly non-empty" predicate when read
    /// alongside [`pcm_stats`]'s default `silence_eps`.
    pub non_silent_samples: usize,
}

/// Threshold below which a sample is treated as inaudible for the
/// `non_silent_samples` counter. ~0.6% of the i16 range; a quiet pure
/// tone at -60 dBFS is well above this.
pub const PCM_SILENCE_EPS: i16 = 64;

/// Compute [`PcmStats`] over an interleaved L,R i16 buffer. `eps` is
/// the threshold used for the `non_silent_samples` counter (set to
/// [`PCM_SILENCE_EPS`] by default).
pub fn pcm_stats_with_eps(pcm: &[i16], eps: i16) -> PcmStats {
    let sample_pairs = pcm.len() / CHANNELS;
    let mut peak_abs: i32 = 0;
    let mut sum_sq: i64 = 0;
    let mut non_silent: usize = 0;
    for &s in pcm {
        let a = (s as i32).abs();
        if a > peak_abs {
            peak_abs = a;
        }
        sum_sq += (s as i64) * (s as i64);
        if a >= eps as i32 {
            non_silent += 1;
        }
    }
    let rms = if !pcm.is_empty() {
        ((sum_sq / pcm.len() as i64) as f64).sqrt() as i32
    } else {
        0
    };
    PcmStats {
        sample_pairs,
        peak_abs,
        rms,
        non_silent_samples: non_silent,
    }
}

/// Default-eps wrapper around [`pcm_stats_with_eps`].
pub fn pcm_stats(pcm: &[i16]) -> PcmStats {
    pcm_stats_with_eps(pcm, PCM_SILENCE_EPS)
}

/// One sample-pair index where engine and retail PCM disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcmDivergence {
    /// Stereo sample-pair index (frames `0..buf.len()/2`).
    pub sample_pair: usize,
    /// `0` for left, `1` for right.
    pub channel: u8,
    /// Engine sample at the divergence point.
    pub engine: i16,
    /// Retail sample at the divergence point.
    pub retail: i16,
    /// Signed difference `engine - retail`.
    pub delta: i32,
}

/// First sample-pair index where `engine` and `retail` differ by more
/// than `tolerance` on either channel.
///
/// `tolerance` is per-channel absolute amplitude. `0` is byte-exact;
/// `256` allows ~0.8% drift; the audio-trace test uses `4096` (~12.5%)
/// to absorb mixer-implementation drift.
///
/// Returns `None` when the buffers match within tolerance for their full
/// length. Buffers of different length compare only the shorter prefix;
/// the caller should ensure they're sized consistently for an
/// interpretable result.
pub fn first_pcm_divergence(
    engine: &[i16],
    retail: &[i16],
    tolerance: i16,
) -> Option<PcmDivergence> {
    let n = engine.len().min(retail.len());
    let pairs = n / CHANNELS;
    let tol = tolerance as i32;
    for pair in 0..pairs {
        for ch in 0..CHANNELS {
            let idx = pair * CHANNELS + ch;
            let e = engine[idx];
            let r = retail[idx];
            let delta = e as i32 - r as i32;
            if delta.abs() > tol {
                return Some(PcmDivergence {
                    sample_pair: pair,
                    channel: ch as u8,
                    engine: e,
                    retail: r,
                    delta,
                });
            }
        }
    }
    None
}

/// Write a 16-bit stereo PCM buffer as a minimal RIFF WAVE file at
/// 44.1 kHz. Self-contained so the CLI doesn't pull in a WAV crate.
pub fn write_wav(path: &Path, pcm: &[i16]) -> Result<()> {
    let channels: u16 = CHANNELS as u16;
    let bits_per_sample: u16 = 16;
    let sample_rate = SPU_SAMPLE_RATE;
    let byte_rate = sample_rate * channels as u32 * (bits_per_sample as u32 / 8);
    let block_align = channels * (bits_per_sample / 8);
    let data_bytes = (pcm.len() * 2) as u32;
    let riff_size = 36 + data_bytes;

    let mut out = Vec::with_capacity(44 + pcm.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // audio format = PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, &out).with_context(|| format!("write WAV {}", path.display()))?;
    Ok(())
}

// Re-export the existing private frame sampler so this module doesn't
// duplicate its body. The audio-trace oracle keeps the sampler private;
// this re-export makes it `pub(crate)` for cross-module use.
//
// (Implementation: a `pub(crate) use` in `audio_trace_oracle.rs` plus a
// thin wrapper named `sample_engine_frame_for_pcm` so the two callers
// can have distinct doc strings.)

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_audio::spu::adsr::AdsrConfig;

    #[test]
    fn render_pcm_window_silence_zero() {
        let mut spu = Spu::new();
        let pcm = render_pcm_window(&mut spu, 256);
        assert_eq!(pcm.len(), 512);
        assert!(pcm.iter().all(|&s| s == 0));
    }

    #[test]
    fn pcm_stats_silence_reports_zero() {
        let pcm = vec![0i16; 1024];
        let stats = pcm_stats(&pcm);
        assert_eq!(stats.sample_pairs, 512);
        assert_eq!(stats.peak_abs, 0);
        assert_eq!(stats.rms, 0);
        assert_eq!(stats.non_silent_samples, 0);
    }

    #[test]
    fn pcm_stats_full_scale_sine_reports_peak() {
        // 8 stereo samples at +/- 0x7000.
        let pcm: Vec<i16> = [0x7000_i16, -0x7000, 0x7000, -0x7000]
            .iter()
            .flat_map(|&s| [s, s])
            .collect();
        let stats = pcm_stats(&pcm);
        assert_eq!(stats.sample_pairs, 4);
        assert_eq!(stats.peak_abs, 0x7000);
        // Constant amplitude → RMS == peak.
        assert_eq!(stats.rms, 0x7000);
        assert_eq!(stats.non_silent_samples, 8);
    }

    #[test]
    fn pcm_stats_eps_filters_quiet_samples() {
        // 4 stereo samples at +-16 (below default eps=64).
        let pcm: Vec<i16> = vec![16, -16, 16, -16, 16, -16, 16, -16];
        let stats_default = pcm_stats(&pcm);
        assert_eq!(stats_default.non_silent_samples, 0);
        let stats_strict = pcm_stats_with_eps(&pcm, 8);
        assert_eq!(stats_strict.non_silent_samples, 8);
    }

    #[test]
    fn first_pcm_divergence_byte_exact_match_returns_none() {
        let a = vec![100i16, -100, 200, -200, 300, -300];
        assert!(first_pcm_divergence(&a, &a, 0).is_none());
    }

    #[test]
    fn first_pcm_divergence_within_tolerance_returns_none() {
        let engine = vec![100i16, -100, 200, -200];
        let retail = vec![110i16, -90, 210, -210];
        // Per-channel delta is at most 10; tolerance 32 covers it.
        assert!(first_pcm_divergence(&engine, &retail, 32).is_none());
    }

    #[test]
    fn first_pcm_divergence_finds_first_channel() {
        // Engine and retail agree on pair 0 (within tolerance) but disagree
        // by a large amount on pair 1, left channel.
        let engine = vec![100i16, -100, 30_000, 0];
        let retail = vec![100i16, -100, 0, 0];
        let d = first_pcm_divergence(&engine, &retail, 64).unwrap();
        assert_eq!(d.sample_pair, 1);
        assert_eq!(d.channel, 0);
        assert_eq!(d.engine, 30_000);
        assert_eq!(d.retail, 0);
        assert_eq!(d.delta, 30_000);
    }

    #[test]
    fn first_pcm_divergence_right_channel_when_left_matches() {
        let engine = vec![0i16, 30_000];
        let retail = vec![0i16, 0];
        let d = first_pcm_divergence(&engine, &retail, 64).unwrap();
        assert_eq!(d.sample_pair, 0);
        assert_eq!(d.channel, 1);
    }

    #[test]
    fn first_pcm_divergence_compares_shorter_prefix() {
        // engine is longer; we only look at the first 4 samples.
        let engine = vec![0i16, 0, 0, 0, 30_000, 30_000];
        let retail = vec![0i16, 0, 0, 0];
        assert!(first_pcm_divergence(&engine, &retail, 64).is_none());
    }

    #[test]
    fn engine_spu_from_retail_returns_none_when_save_has_no_spu_ram() {
        use legaia_mednafen::container::{MDFN_HEADER_LEN, MDFN_MAGIC, SECTION_NAME_LEN};
        // Build a SaveState with an SPU section that doesn't carry SPURAM
        // bytes - the translator should refuse rather than ship a
        // zero-initialised mixer.
        let mut name_buf = [0u8; SECTION_NAME_LEN];
        name_buf[..3].copy_from_slice(b"SPU");
        let mut payload = Vec::new();
        payload.extend_from_slice(MDFN_MAGIC);
        payload.extend_from_slice(&[0u8; MDFN_HEADER_LEN - MDFN_MAGIC.len()]);
        payload.extend_from_slice(&name_buf);
        payload.extend_from_slice(&0u32.to_le_bytes()); // empty body
        let state = SaveState::from_decompressed(payload).unwrap();
        let spu = PsxSpu::new(&state);
        assert!(engine_spu_from_retail(&spu).is_none());
    }

    #[test]
    fn engine_spu_from_retail_translates_voice_state_and_key_on() {
        // Hand-roll a small mednafen save with the SPU section populated
        // for voice 3: start_addr, pitch, ADSR phase non-zero (active).
        use legaia_mednafen::container::{MDFN_HEADER_LEN, MDFN_MAGIC, SECTION_NAME_LEN};
        // Studio C reverb register block (public PSX hardware-reference
        // preset; what retail actually installs) laid into the `Regs` shadow
        // at the reverb-register window (SPU 0x1F801DC0 = Regs[224]), with the
        // EON voice-reverb-enable register (Regs[204]) routing voice 3.
        let studio_c: [u16; 32] = [
            0x00E3, 0x00A9, 0x6F60, 0x4FA8, 0xBCE0, 0x4510, 0xBEF0, 0xA680, 0x5680, 0x52C0, 0x0DFB,
            0x0B58, 0x0D09, 0x0A3C, 0x0BD9, 0x0973, 0x0B59, 0x08DA, 0x08D9, 0x05E9, 0x07EC, 0x04B0,
            0x06EF, 0x03D2, 0x05EA, 0x031D, 0x031C, 0x0238, 0x0154, 0x00AA, 0x8000, 0x8000,
        ];
        let mut regs = vec![0u8; 512];
        for (i, w) in studio_c.iter().enumerate() {
            let off = (224 + i) * 2;
            regs[off..off + 2].copy_from_slice(&w.to_le_bytes());
        }
        regs[204 * 2..204 * 2 + 2].copy_from_slice(&(1u16 << 3).to_le_bytes()); // EON: voice 3
        let entries: &[(&str, Vec<u8>)] = &[
            ("SPURAM", vec![0u8; 512 * 1024]),
            ("Voices[3].StartAddr", 0x1000u32.to_le_bytes().to_vec()),
            ("Voices[3].Pitch", 0x1234u16.to_le_bytes().to_vec()),
            ("Voices[3].ADSR.Phase", 1u32.to_le_bytes().to_vec()),
            (
                "(Voices[3].Sweep[0]).Current",
                0x3FFFi16.to_le_bytes().to_vec(),
            ),
            (
                "(Voices[3].Sweep[1]).Current",
                0x3FFFi16.to_le_bytes().to_vec(),
            ),
            ("(GlobalSweep[0]).Current", 0x3F00i16.to_le_bytes().to_vec()),
            ("(GlobalSweep[1]).Current", 0x3F00i16.to_le_bytes().to_vec()),
            ("SPUControl", 0xC080u16.to_le_bytes().to_vec()), // SPU + reverb on
            ("Regs", regs),
        ];
        let mut body = Vec::new();
        for (name, value) in entries {
            body.push(name.len() as u8);
            body.extend_from_slice(name.as_bytes());
            body.extend_from_slice(&(value.len() as u32).to_le_bytes());
            body.extend_from_slice(value);
        }
        let mut name_buf = [0u8; SECTION_NAME_LEN];
        name_buf[..3].copy_from_slice(b"SPU");
        let mut payload = Vec::new();
        payload.extend_from_slice(MDFN_MAGIC);
        payload.extend_from_slice(&[0u8; MDFN_HEADER_LEN - MDFN_MAGIC.len()]);
        payload.extend_from_slice(&name_buf);
        payload.extend_from_slice(&(body.len() as u32).to_le_bytes());
        payload.extend_from_slice(&body);

        let state = SaveState::from_decompressed(payload).unwrap();
        let psx_spu = PsxSpu::new(&state);
        let spu = engine_spu_from_retail(&psx_spu).expect("SPURAM is present");

        assert_eq!(spu.master_left, 0x3F00);
        assert_eq!(spu.master_right, 0x3F00);
        // Reverb mode is identified from the captured coefficient registers
        // (Studio C), not mednafen's `Reverb_Mode` (which is the EON mask).
        assert_eq!(spu.reverb.mode, ReverbMode::StudioC);
        assert_eq!(spu.voices[3].start_addr, 0x1000);
        assert_eq!(spu.voices[3].pitch, 0x1234);
        // Voice 3 is in the EON mask → reverb-routed.
        assert!(spu.voices[3].reverb_send);
        // Voice 4 isn't in the EON mask → dry.
        assert!(!spu.voices[4].reverb_send);
        // Voice 3 retail-active → key_on'd.
        assert!(!spu.voices[3].is_off());
        // Voice 4 wasn't programmed → off.
        assert!(spu.voices[4].is_off());
    }

    #[test]
    fn render_pcm_window_with_seeded_voice_produces_audio() {
        // Smoke: plant a tiny non-silence ADPCM block at 0x1000 (header
        // byte 0 = filter/shift = 0, byte 1 = end-no-repeat) with non-zero
        // body bytes, key one voice on, render. The decoder produces some
        // amplitude (not byte-exact testing, just "not all zero").
        let mut spu = Spu::new();
        let mut block = [0u8; 16];
        block[1] = 0x01; // end flag, no repeat
        for (i, b) in block.iter_mut().enumerate().skip(2) {
            *b = (i as u8) * 0x10;
        }
        spu.ram.write_at(0x1000, &block);
        spu.voices[0].start_addr = 0x1000;
        spu.voices[0].vol_left = 0x3FFF;
        spu.voices[0].vol_right = 0x3FFF;
        // Default ADSR has instant attack → first samples are audible.
        // (Decay can pull the envelope below the threshold quickly, so we
        // only check the leading edge.)
        spu.voices[0].adsr_cfg = AdsrConfig::default();
        spu.key_on_mask(0b1);
        let pcm = render_pcm_window(&mut spu, 32);
        let stats = pcm_stats(&pcm);
        assert!(
            stats.peak_abs > 0,
            "expected non-zero peak amplitude, got stats={stats:?}",
        );
    }
}
