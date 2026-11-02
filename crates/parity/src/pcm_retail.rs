//! The engine's mix against real retail PCM.
//!
//! [`crate::pcm_oracle`]'s "retail" window is the engine's own mixer seeded
//! from a snapshot, so it cannot see a defect in that mixer. This oracle's
//! retail side is the emulator's actual output: a WAV mednafen wrote while it
//! played on from a library state (`scripts/ci/audio-pcm.py`).
//!
//! The engine side starts the state's track and runs vsync by vsync until its
//! sequencer stands where the state's libsnd record stands: the event the read
//! cursor `+0x00` waits on, with the same `+0x90` tenths of a tick left. From
//! there the two windows play the same bars of the same score.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use legaia_engine_audio::Spu;
use legaia_engine_audio::spu::adsr::{AdsrConfig, Phase};
use legaia_engine_audio::spu::ram::SpuRam;
use legaia_engine_audio::spu::reverb::{RESAMPLE_IN_LEN, RESAMPLE_OUT_LEN};
use legaia_engine_audio::spu::voice::VoiceResume;
use legaia_engine_scene_host::scene_host::BgmDirector;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_ram_image::ScenarioManifest;

use crate::audio_sweep::{RetailAudio, TrackBank};
use crate::audio_trace_oracle::TraceBgmDirector;
use crate::retail_compare::enumerate_corpus;

/// libsnd's per-slot record pointer table (`_DAT_801CD2C0[16]`).
const SEQ_TABLE: u32 = 0x801C_D2C0;

/// mednafen's vsync in SPU samples (59.826 Hz, fitted: the drift of a track's
/// note onsets against the emulator's output zeroes here). The port's own
/// frame is a flat 60 Hz.
pub const RETAIL_VSYNC_SAMPLES: f64 = 737.13;

#[derive(Debug, clap::Args)]
pub struct PcmRetailArgs {
    /// Scenario label of a mednafen library state.
    #[arg(long, default_value = "")]
    pub label: String,
    /// Resume this mednafen state's SPU sample for sample and render on with
    /// no sequencer at all: the SPU core against the emulator's, on a state
    /// whose game code writes no more voices. `--label` is ignored.
    #[arg(long)]
    pub resume_state: Option<PathBuf>,
    /// Stereo samples of the aligned window to render.
    #[arg(long, default_value_t = 44_100 * 45)]
    pub samples: usize,
    /// Where to write the aligned engine window (WAV).
    #[arg(long)]
    pub engine_wav: Option<PathBuf>,
    /// mednafen's recording of the same window: score the engine against it.
    #[arg(long)]
    pub retail_wav: Option<PathBuf>,
    /// Write the score here.
    #[arg(long)]
    pub json: Option<PathBuf>,
    /// Print the aligned position and the sounding voices.
    #[arg(long)]
    pub verbose: bool,
    /// Samples per retail vsync.
    #[arg(long, default_value_t = RETAIL_VSYNC_SAMPLES)]
    pub vsync_samples: f64,
    /// Use this state file instead of the library state `--label` names.
    #[arg(long)]
    pub state: Option<PathBuf>,
    /// Which time through the score to stop on (a channel's state carries
    /// across the loop, so the second pass need not sound like the first).
    #[arg(long, default_value_t = 1)]
    pub pass: u32,
    #[arg(long)]
    pub library: Option<PathBuf>,
    #[arg(long)]
    pub extracted_root: Option<PathBuf>,
    #[arg(long)]
    pub manifest: Option<PathBuf>,
}

/// Where a retail state's sequence stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailSeqPosition {
    pub track: u16,
    pub seq_vol: u8,
    /// Index of the event the read cursor waits on.
    pub event: usize,
    /// `+0x90`, tenths of a tick left to wait.
    pub wait: i64,
}

/// Read the playing sequence's position off a state's main RAM.
pub fn retail_position(ram: &[u8], bank: &TrackBank, entry: &[u8]) -> Result<RetailSeqPosition> {
    let audio = RetailAudio::from_ram(ram, bank, None);
    let slot = audio.music_slot().context("no open sequence")?;
    let track = slot.track.context("slot holds no known track")?;
    let at = |va: u32| (va & 0x1F_FFFF) as usize;
    let rd32 = |va: u32| {
        let i = at(va);
        ram.get(i..i + 4)
            .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let rec = rd32(SEQ_TABLE + u32::from(slot.slot) * 4);
    let start = rd32(rec + 4);
    let cursor = rd32(rec);
    let wait = i64::from(rd32(rec + 0x90) as i32);
    // `+0x04` holds a window of the score that the sweep matches against the
    // entry's bytes; the cursor counts from there.
    let start_off = at(start);
    let needle = ram
        .get(start_off..start_off + 24)
        .context("score start out of RAM")?;
    let split = legaia_engine_system::chunk_install::owned_bank_offsets(entry)
        .context("track entry does not split")?;
    let score = &entry[split.seq..];
    let base = score
        .windows(24)
        .position(|w| w == needle)
        .context("score start not in the track's entry")?;
    let off = base + (cursor.wrapping_sub(start)) as usize;
    let offsets = legaia_seq::event_stream_offsets(score)?;
    let event = offsets
        .iter()
        .position(|&o| o == off)
        .with_context(|| format!("cursor +{off:#x} is on no event (base {base:#x})"))?;
    Ok(RetailSeqPosition {
        track,
        seq_vol: slot.vol.0 as u8,
        event,
        wait,
    })
}

/// Run `track` vsync by vsync until the sequencer reaches `pos`, then render
/// `samples` stereo samples, one vsync every `vsync_samples`.
pub fn render_aligned(
    entry: &[u8],
    pos: &RetailSeqPosition,
    samples: usize,
    vsync_samples: f64,
    pass: u32,
    verbose: bool,
) -> Result<Vec<i16>> {
    let mut d = TraceBgmDirector::new();
    d.master_vol = pos.seq_vol;
    d.start_owned_vab(pos.track, entry);
    let step = |d: &mut TraceBgmDirector, carry: &mut f64, out: Option<&mut Vec<i16>>| {
        let spu: *mut Spu = d.spu_mut();
        if let Some(s) = d.sequencer_mut() {
            // SAFETY: the director's SPU and sequencer are disjoint fields.
            s.step_vsync(unsafe { &mut *spu });
        }
        *carry += vsync_samples;
        let n = carry.floor() as usize;
        *carry -= n as f64;
        let mut buf = vec![0i16; n * 2];
        d.spu_mut().render_into(&mut buf);
        if let Some(o) = out {
            o.extend_from_slice(&buf);
        }
    };
    let mut carry = 0.0;
    let mut found = false;
    let mut left = pass;
    let mut prev_next = 0;
    for _ in 0..60 * 1800 {
        let (next, wait) = d
            .sequencer()
            .map_or((0, None), |s| (s.next_event(), s.pump_wait()));
        // One vsync can fire past the event outright, and a later pass waits
        // it out with its own carry, so the wait need not recur exactly.
        if prev_next < pos.event && next >= pos.event {
            left = left.saturating_sub(1);
        }
        if left == 0 && (next != pos.event || wait.is_some_and(|w| w <= pos.wait)) {
            found = true;
            break;
        }
        prev_next = next;
        step(&mut d, &mut carry, None);
    }
    if !found {
        let seq = d.sequencer().map(|s| (s.next_event(), s.loop_count()));
        bail!("the sequencer never reached {pos:?}; it stands at {seq:?}");
    }
    if verbose {
        for (i, v) in d.spu().voices.iter().enumerate() {
            if !v.is_off() {
                println!(
                    "engine v{i}: {:?} env {:x} ctl {:04x}{:04x} pitch {:x} start {:x} vol {} {} rev {}",
                    v.adsr.phase,
                    v.adsr.level,
                    v.adsr_cfg.raw.1,
                    v.adsr_cfg.raw.0,
                    v.pitch,
                    v.start_addr,
                    v.vol_left,
                    v.vol_right,
                    v.reverb_send
                );
            }
        }
    }
    let mut out = Vec::with_capacity(samples * 2 + 4096);
    while out.len() < samples * 2 {
        step(&mut d, &mut carry, Some(&mut out));
    }
    out.truncate(samples * 2);
    Ok(out)
}

/// An engine [`Spu`] standing exactly where a mednafen state's SPU stands.
pub fn spu_resumed_from_retail(st: &legaia_mednafen::SaveState) -> Result<Spu> {
    let psx = legaia_mednafen::PsxSpu::new(st);
    let ram = psx.spu_ram_bytes().context("state carries no SPU RAM")?;
    let mut spu = crate::pcm_oracle::engine_spu_from_retail(&psx).context("seed")?;
    let halfword = |a: u32| {
        let i = (a as usize & 0x3_FFFF) * 2;
        u16::from_le_bytes([ram[i], ram[i + 1]])
    };
    let ram_copy: SpuRam = spu.ram.clone();
    for i in 0..legaia_mednafen::spu::NUM_VOICES {
        let (Some(v), Some(d)) = (psx.voice_state(i), psx.voice_decoder(i)) else {
            continue;
        };
        let voice = &mut spu.voices[i];
        let level = v.adsr_env_level.unwrap_or(0);
        voice.adsr_cfg = AdsrConfig::from_words(
            v.adsr_control.unwrap_or(0) as u16,
            (v.adsr_control.unwrap_or(0) >> 16) as u16,
        );
        voice.adsr.level = level;
        voice.adsr.phase = match v.adsr_phase {
            _ if level == 0 => Phase::Off,
            Some(0) => Phase::Attack,
            Some(1) => Phase::Decay,
            Some(2) => Phase::Sustain,
            _ => Phase::Release,
        };
        if level == 0 {
            continue;
        }
        let ring = |back: u32| d.buffer[(d.write_pos.wrapping_sub(back) & 31) as usize];
        let cur = v.cur_addr.unwrap_or(0);
        // Samples of the block in hand already in the ring: a just-finished
        // block is 28 with the cursor on the next block's header.
        let (block, decoded) = if cur & 7 == 0 {
            (cur.wrapping_sub(8) & 0x3_FFF8, 28)
        } else {
            (cur & 0x3_FFF8, ((cur & 7) - 1) * 4)
        };
        // The newest tap sits `avail - 4` samples behind the newest decoded.
        let newest = decoded as i32 - 1 - (d.avail as i32 - 4);
        let hist: [i16; 4] =
            std::array::from_fn(|k| d.buffer[((d.read_pos + k as u32) & 31) as usize]);
        let before_block = (newest < 0).then(|| {
            let n = (-newest) as u32;
            (0..n).map(|k| ring(decoded + n - k)).collect::<Vec<_>>()
        });
        voice.resume(
            &ram_copy,
            VoiceResume {
                block_addr: block * 2,
                sample_idx: newest.max(0) as u8,
                before_block,
                sample_frac: v.cur_phase.unwrap_or(0),
                hist,
                prev: (ring(decoded + 1) as i32, ring(decoded + 2) as i32),
            },
        );
    }
    if let Some(r) = psx.reverb_state() {
        let size = spu.reverb.work_area_base_bytes();
        let base = r.work_area;
        if size != 0 {
            let tank: Vec<i16> = (base..0x4_0000).map(|a| halfword(a) as i16).collect();
            let p = r.res_pos;
            let send = |lr: usize| -> [i16; RESAMPLE_IN_LEN] {
                std::array::from_fn(|k| {
                    r.down[lr][(p
                        .wrapping_sub(RESAMPLE_IN_LEN as u32)
                        .wrapping_add(k as u32)
                        & 0x3F) as usize]
                })
            };
            let wet = |lr: usize| -> [i16; RESAMPLE_OUT_LEN] {
                let last = (p >> 1).wrapping_sub(1);
                std::array::from_fn(|k| {
                    r.up[lr][(last
                        .wrapping_sub(RESAMPLE_OUT_LEN as u32 - 1)
                        .wrapping_add(k as u32)
                        & 0x1F) as usize]
                })
            };
            let (s0, s1, w0, w1) = (send(0), send(1), wet(0), wet(1));
            spu.reverb.resume(
                &tank,
                r.cur as i32 - base as i32,
                p & 1 == 1,
                [&s0, &s1],
                [&w0, &w1],
            );
        }
    }
    Ok(spu)
}

pub fn run(args: PcmRetailArgs) -> Result<()> {
    let (pcm, label) = if let Some(state) = &args.resume_state {
        let st = legaia_mednafen::SaveState::from_path(state)?;
        let mut spu = spu_resumed_from_retail(&st)?;
        let mut pcm = vec![0i16; args.samples * 2];
        spu.render_into(&mut pcm);
        (pcm, state.display().to_string())
    } else {
        let (m, l, e) = crate::retail_compare::resolve_dirs();
        let manifest_path = args.manifest.or(m).context("no scripts/scenarios.toml")?;
        let library = args.library.or(l).context("no save library")?;
        let extracted = args.extracted_root.or(e).context("no extracted disc")?;
        let manifest = ScenarioManifest::from_path(&manifest_path)?;
        let path = match &args.state {
            Some(p) => p.clone(),
            None => {
                enumerate_corpus(&manifest, &library)
                    .into_iter()
                    .find(|c| c.label == args.label && c.emulator == "mednafen")
                    .with_context(|| format!("no mednafen library state {:?}", args.label))?
                    .path
            }
        };
        let (pos, track_bytes) = read_position(&path, &extracted)?;
        if args.verbose {
            println!("{}: {pos:?}", args.label);
        }
        let pcm = render_aligned(
            &track_bytes,
            &pos,
            args.samples,
            args.vsync_samples,
            args.pass,
            args.verbose,
        )?;
        (pcm, args.label.clone())
    };
    if let Some(p) = &args.engine_wav {
        crate::pcm_oracle::write_wav(p, &pcm)?;
    }
    if let Some(p) = &args.retail_wav {
        let score = score_window(&read_wav(p)?, &pcm);
        println!(
            "{label}: corr {:.4} (p10 {:.4}) level {:+.2} dB residual {:.1} dB lag {} over {} windows",
            score.corr_median,
            score.corr_p10,
            score.level_db,
            score.residual_db,
            score.lag,
            score.windows
        );
        if let Some(out) = &args.json {
            std::fs::write(out, serde_json::to_string_pretty(&score)?)?;
        }
    }
    Ok(())
}

/// mednafen's WAV recording is the emulated output at `3/4`: a resumed SPU
/// matches it at correlation `1.0000`, level `0.750`, in every band, over every
/// state tried. A property of the recorder, not of the console: every level
/// here is scored after dividing it out.
pub const MEDNAFEN_RECORD_GAIN: f64 = 0.75;

/// One window of engine PCM against one of retail PCM.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PcmScore {
    /// Median per-half-second correlation at the best lag.
    pub corr_median: f64,
    /// Its tenth percentile, which the bars a cue or a phase collision cost.
    pub corr_p10: f64,
    /// Retail level over the engine's, whole window, dB.
    pub level_db: f64,
    /// Median residual energy over retail's, dB.
    pub residual_db: f64,
    /// Median best lag, samples.
    pub lag: i64,
    /// Half-second windows scored (retail silent ones are not).
    pub windows: usize,
}

const WINDOW: usize = 22_050;
/// Lag search on either side of the running estimate, samples: past one vsync
/// on the first window, then a few samples for what wanders.
const FIRST_SEARCH: i64 = 3_000;
const NEXT_SEARCH: i64 = 24;
/// Below this correlation a window's lag is noise: the next searches in full.
const CENTER_MIN_CORR: f64 = 0.6;
/// A window below this retail RMS is the state's own silence.
const SILENT_RMS: f64 = 30.0;

fn read_wav(path: &Path) -> Result<Vec<i16>> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let data = bytes
        .windows(4)
        .position(|w| w == b"data")
        .context("no data chunk")?
        + 8;
    Ok(bytes[data..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect())
}

/// Score interleaved stereo `engine` against mednafen's `retail` recording.
pub fn score_window(retail: &[i16], engine: &[i16]) -> PcmScore {
    let side = |pcm: &[i16], gain: f64| -> Vec<f64> {
        pcm.as_chunks::<2>()
            .0
            .iter()
            .map(|s| (f64::from(s[0]) + f64::from(s[1])) * 0.5 / gain)
            .collect()
    };
    let r = side(retail, MEDNAFEN_RECORD_GAIN);
    let e = side(engine, 1.0);
    let mut corrs = Vec::new();
    let mut residuals = Vec::new();
    let mut lags = Vec::new();
    let (mut re, mut ee) = (0.0, 0.0);
    let mut center: Option<i64> = None;
    let span = r.len().min(e.len());
    // The first window is skipped: a negative lag cannot reach before sample 0,
    // and the recording's own rate converter is still settling there.
    for start in (WINDOW..span.saturating_sub(WINDOW)).step_by(WINDOW) {
        let a = &r[start..start + WINDOW];
        let energy: f64 = a.iter().map(|x| x * x).sum();
        if (energy / WINDOW as f64).sqrt() < SILENT_RMS {
            continue;
        }
        let (mid, reach) = center.map_or((0, FIRST_SEARCH), |c| (c, NEXT_SEARCH));
        // The first window's search is subsampled: a track's low end carries
        // the lag, and it is re-read in full around the winner next window.
        let stride = if center.is_some() { 1 } else { 2 };
        let at = |lag: i64| -> Option<&[f64]> {
            let s = start as i64 + lag;
            (s >= 0).then(|| e.get(s as usize..s as usize + WINDOW))?
        };
        let mut best: Option<(f64, i64)> = None;
        for lag in (mid - reach..=mid + reach).step_by(stride) {
            let Some(b) = at(lag) else { continue };
            let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
            let eb: f64 = b.iter().map(|y| y * y).sum();
            let c = dot / (energy * eb + 1e-9).sqrt();
            if best.is_none_or(|(k, _)| c > k) {
                best = Some((c, lag));
            }
        }
        let Some((c, lag)) = best else { break };
        let b = at(lag).expect("searched above");
        let resid: f64 = a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum();
        corrs.push(c);
        residuals.push(10.0 * (resid / energy + 1e-9).log10());
        lags.push(lag);
        re += energy;
        ee += b.iter().map(|y| y * y).sum::<f64>();
        // Only a window that found the music steers the next one's search.
        center = (c > CENTER_MIN_CORR).then_some(lag);
    }
    if corrs.is_empty() {
        return PcmScore::default();
    }
    let windows = corrs.len();
    let pick = |v: &mut Vec<f64>, q: f64| {
        v.sort_by(f64::total_cmp);
        v[((v.len() - 1) as f64 * q) as usize]
    };
    lags.sort_unstable();
    PcmScore {
        corr_p10: pick(&mut corrs, 0.1),
        corr_median: pick(&mut corrs, 0.5),
        level_db: 10.0 * (re / ee.max(1.0)).log10(),
        residual_db: pick(&mut residuals, 0.5),
        lag: lags[lags.len() / 2],
        windows,
    }
}

fn read_position(state: &Path, extracted: &Path) -> Result<(RetailSeqPosition, Vec<u8>)> {
    let host = SceneHost::open_extracted(extracted)?;
    let bank = TrackBank::load(&host);
    let st = legaia_mednafen::SaveState::from_path(state)?;
    let ram = st.main_ram()?.to_vec();
    let audio = RetailAudio::from_ram(&ram, &bank, None);
    let slot = audio.music_slot().context("no open sequence")?;
    let track = slot.track.context("slot holds no known track")?;
    let bytes = host
        .music_bank_entry_bytes(track)?
        .context("track has no bank entry")?;
    let pos = retail_position(&ram, &bank, &bytes)?;
    Ok((pos, bytes.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(n: usize, gain: f64, shift: usize) -> Vec<i16> {
        (0..n)
            .flat_map(|i| {
                let t = (i + shift) as f64;
                // Two inharmonic tones: one lag wins outright.
                let v = (t * 0.031).sin() * 2000.0 + (t * 0.0471).sin() * 1500.0;
                let s = (v * gain) as i16;
                [s, s]
            })
            .collect()
    }

    #[test]
    fn the_score_finds_the_lag_and_divides_the_recorders_gain_out() {
        let retail = tone(88_200, MEDNAFEN_RECORD_GAIN, 0);
        // `engine[k - 1000] == retail[k]`: the engine sounds 1000 samples early.
        let s = score_window(&retail, &tone(88_200, 1.0, 1000));
        assert!(s.corr_median > 0.999, "{s:?}");
        assert!(s.level_db.abs() < 0.05, "{s:?}");
        assert_eq!(s.lag, -1000, "{s:?}");
    }

    #[test]
    fn a_louder_engine_reads_as_a_negative_level() {
        let retail = tone(88_200, MEDNAFEN_RECORD_GAIN, 0);
        let engine = tone(88_200, 1.25, 0);
        let s = score_window(&retail, &engine);
        assert!((s.level_db + 1.94).abs() < 0.05, "{s:?}");
    }

    #[test]
    fn a_silent_retail_window_scores_nothing() {
        let s = score_window(&vec![0; 176_400], &tone(88_200, 1.0, 0));
        assert_eq!(s.windows, 0);
    }
}
