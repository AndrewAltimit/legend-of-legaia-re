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
use legaia_engine_audio::note_trace::NoteTrace;
use legaia_engine_audio::spu::adsr::{AdsrConfig, Phase};
use legaia_engine_audio::spu::ram::SpuRam;
use legaia_engine_audio::spu::reverb::{RESAMPLE_IN_LEN, RESAMPLE_OUT_LEN};
use legaia_engine_audio::spu::voice::SPU_INTERNAL_RATE;
use legaia_engine_audio::spu::voice::VoiceResume;
use legaia_engine_audio::{OfflineMixer, Spu};
use legaia_engine_scene_host::scene_host::BgmDirector;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_ram_image::ScenarioManifest;

use crate::audio_sweep::{RetailAudio, TrackBank};
use crate::audio_trace_oracle::TraceBgmDirector;
use crate::retail_compare::enumerate_corpus;

/// libsnd's per-slot record pointer table (`_DAT_801CD2C0[16]`).
const SEQ_TABLE: u32 = 0x801C_D2C0;

/// mednafen's vsync in SPU samples: an NTSC frame, 263 lines of 3413 video clocks
/// at 53.693175 MHz (59.8261 Hz), against 44.1 kHz. The drift of a track's note
/// onsets against the emulator's output zeroes here. The port's own frame is a
/// flat 60 Hz.
pub const RETAIL_VSYNC_SAMPLES: f64 = 737.1365;

/// How many arrivals at a state's event `--pass 0` looks at. An hour of vsyncs
/// bounds it for a score whose event comes round less often than that.
pub const EXACT_PASS_SEARCH: u32 = 16;

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
    /// With `--resume-state`: a decoded XA channel (mono, as `legaia-extract`
    /// writes under `XA_WAV/`) the state's CD input is reading. It plays through
    /// the mix core at the cue gain beside the resumed SPU.
    #[arg(long, requires = "resume_state")]
    pub xa_wav: Option<PathBuf>,
    /// Where in that channel the state stands, source samples.
    #[arg(long, default_value_t = 0)]
    pub xa_start: usize,
    /// Stereo samples of the aligned window to render.
    #[arg(long, default_value_t = 44_100 * 45)]
    pub samples: usize,
    /// Where to write the aligned engine window (WAV).
    #[arg(long)]
    pub engine_wav: Option<PathBuf>,
    /// Write the render's split for `scripts/ci/audio-note-fit.py`: `PREFIX.stems`
    /// (every voice and the wet return, per sample), `PREFIX.notes` (the
    /// key-ons), `PREFIX.flush` (how many voices each flush staged).
    #[arg(long)]
    pub stems: Option<PathBuf>,
    /// Mix in the state's own cue voices, resumed from its SPU, on the track's
    /// lag: a state with a looping emitter holds it for the whole recording.
    #[arg(long, requires = "retail_wav")]
    pub cue_voices: bool,
    /// Mix in the engine's own cues from the state on, on the track's lag: a
    /// `play-window` capture child's `LEGAIA_DIAG_AUDIO_WAV` recording (the
    /// fight seeded into the state, its music bus silent), which carries what
    /// the engine's cast keys after the state's instant.
    #[arg(long, requires = "retail_wav")]
    pub cast_wav: Option<PathBuf>,
    /// Print the level and correlation second by second.
    #[arg(long)]
    pub per_second: bool,
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
    /// Which time through the score to stop on. The tick stream's phase against
    /// the vsync differs pass by pass, so which vsync a note lands on does too.
    /// `0` stops where the state's wait matches to the tenth of a tick: the
    /// state's own phase, searched over [`EXACT_PASS_SEARCH`] passes.
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
    /// libsnd's monaural flag `_DAT_801CE330 == 1` (`SsSetMono`, the options
    /// screen's Sound row): every voice volume the key-on and the re-volume
    /// walk build is folded to the larger side (`0x800682B8..0x800682E8` in
    /// `FUN_80067E9C`). A state saved in Monaural is rendered that way.
    pub mono: bool,
}

/// libsnd's monaural flag (`SsSetMono`), read as a halfword against `1` by
/// the voice-volume chain (`lh v1,-0x1cd0(v1)` at `0x800682BC`).
pub const LIBSND_MONO: u32 = 0x801C_E330;

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
    let mono = ram
        .get(at(LIBSND_MONO)..at(LIBSND_MONO) + 2)
        .is_some_and(|b| i16::from_le_bytes([b[0], b[1]]) == 1);
    Ok(RetailSeqPosition {
        track,
        seq_vol: slot.vol.0 as u8,
        event,
        wait,
        mono,
    })
}

/// Run `track` vsync by vsync until the sequencer reaches `pos`, then render
/// `samples` stereo samples, one vsync every `vsync_samples`. `seq_vols`, when
/// given, is the sequence volume of each rendered vsync ([`duck_schedule`]).
#[allow(clippy::too_many_arguments)]
pub fn render_aligned(
    entry: &[u8],
    pos: &RetailSeqPosition,
    samples: usize,
    vsync_samples: f64,
    pass: u32,
    verbose: bool,
    stems: Option<&Path>,
    seq_vols: Option<&[u8]>,
) -> Result<Vec<i16>> {
    let mut d = TraceBgmDirector::new();
    d.master_vol = pos.seq_vol;
    d.start_owned_vab(pos.track, entry);
    d.spu_mut().mono = pos.mono;
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
    let exact = pass == 0;
    let mut left = if exact { EXACT_PASS_SEARCH } else { pass };
    let mut prev_next = 0;
    let budget = if exact { 60 * 3600 } else { 60 * 1800 };
    for _ in 0..budget {
        let (next, wait) = d
            .sequencer()
            .map_or((0, None), |s| (s.next_event(), s.pump_wait()));
        if exact && next == pos.event && wait == Some(pos.wait) {
            found = true;
            break;
        }
        // One vsync can fire past the event outright, and a later pass waits
        // it out with its own carry, so the wait need not recur exactly.
        if prev_next < pos.event && next >= pos.event {
            left = left.saturating_sub(1);
            if exact && left == 0 {
                break;
            }
        }
        if !exact && left == 0 && (next != pos.event || wait.is_some_and(|w| w <= pos.wait)) {
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
        let at = d.sequencer().map(|s| (s.next_event(), s.pump_wait()));
        println!("engine stands at {at:?}");
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
    if let Some(prefix) = stems {
        render_stems(&mut d, &mut carry, vsync_samples, samples, &mut out, prefix)?;
    }
    let mut vsync = 0usize;
    while out.len() < samples * 2 {
        // The sequence volume of this vsync of the window, when the caller
        // hands a schedule: the duck a cast ramps the score under.
        if let Some(v) = seq_vols.and_then(|v| v.get(vsync).or(v.last()))
            && let Some(s) = d.sequencer_mut()
        {
            s.set_master_vol(*v);
        }
        step(&mut d, &mut carry, Some(&mut out));
        vsync += 1;
    }
    out.truncate(samples * 2);
    Ok(out)
}

/// The sequence volume a cast's score plays at, one entry per output vsync of
/// the track render. `levels` is the engine child's applied audio level per tick
/// (`<cast>.duck`); `start` is the state's own level `_DAT_8007B910`, which the
/// child's seed does not carry (a state taken mid-ramp, `zenoir_summon_mid_cast`
/// at `192`, seeds at `215`). The level walks from `start` one unit a vsync
/// toward where the engine's level is heading - the next value it rests on -
/// as retail's ramp does (`legaia_engine_audio::duck::step_duck`), and plays
/// halved, as every level-to-volume site halves it. Vsync `k` of the render
/// stands on recording sample `k * vsync - lag`.
pub fn duck_schedule(levels: &[u8], start: u8, lag: i64, vsync_samples: f64) -> Vec<u8> {
    if levels.is_empty() {
        return Vec::new();
    }
    // Where the engine's level next rests, tick by tick.
    let mut target = levels.to_vec();
    for k in (0..levels.len().saturating_sub(1)).rev() {
        if levels[k] != levels[k + 1] {
            target[k] = target[k + 1];
        }
    }
    let mut level = start;
    let walked: Vec<u8> = target
        .iter()
        .map(|&t| {
            let at = level;
            legaia_engine_audio::duck::step_duck(&mut level, t);
            at
        })
        .collect();
    let shift = (-lag as f64 / vsync_samples).round() as i64;
    (0..walked.len())
        .map(|k| {
            let at = (k as i64 + shift).clamp(0, walked.len() as i64 - 1) as usize;
            walked[at] >> 1
        })
        .collect()
}

/// [`render_aligned`]'s window with the mix's parts kept apart: one tick of
/// [`Spu`] spelled out, its voices and its wet return written beside the sum.
/// The key-ons and the flushes' staged voices go beside them.
fn render_stems(
    d: &mut TraceBgmDirector,
    carry: &mut f64,
    vsync_samples: f64,
    samples: usize,
    out: &mut Vec<i16>,
    prefix: &Path,
) -> Result<()> {
    let with = |ext: &str| prefix.with_extension(ext);
    let mut stems = Vec::with_capacity(samples * 100);
    let mut flushes = String::new();
    d.spu_mut().note_trace = Some(NoteTrace::new());
    while out.len() < samples * 2 {
        let at = (out.len() / 2) as u64;
        let spu: *mut Spu = d.spu_mut();
        if let Some(s) = d.sequencer_mut() {
            // SAFETY: the director's SPU and sequencer are disjoint fields.
            let spu = unsafe { &mut *spu };
            if let Some(t) = spu.note_trace.as_mut() {
                t.sample = at;
            }
            s.step_vsync(spu);
            flushes.push_str(&format!("{at} {}\n", s.last_flush_staged()));
        }
        *carry += vsync_samples;
        let n = carry.floor() as usize;
        *carry -= n as f64;
        let spu = d.spu_mut();
        for _ in 0..n {
            let (mut acc, mut send) = ([0i64; 2], [0i64; 2]);
            let Spu {
                voices,
                ram,
                reverb,
                ..
            } = spu;
            for v in voices.iter_mut() {
                let (l, r) = v.tick(ram);
                for (i, x) in [l, r].into_iter().enumerate() {
                    acc[i] += i64::from(x);
                    if v.reverb_send {
                        send[i] += i64::from(x);
                    }
                    stems.push(x.clamp(-32768, 32767) as i16);
                }
            }
            let narrow = |x: i64| x.clamp(-32768, 32767) as i16;
            let (wl, wr) = reverb.tick(narrow(send[0]), narrow(send[1]));
            stems.extend([wl, wr]);
            for (x, gain, wet) in [
                (acc[0], spu.master_left, wl),
                (acc[1], spu.master_right, wr),
            ] {
                out.push(narrow(((x + i64::from(wet)) * i64::from(gain)) >> 14));
            }
        }
    }
    let trace = d.spu_mut().note_trace.take().unwrap_or_default();
    std::fs::write(with("notes"), trace.to_jsonl("{}"))?;
    std::fs::write(with("flush"), flushes)?;
    stems.truncate(samples * 50);
    let bytes: Vec<u8> = stems.iter().flat_map(|x| x.to_le_bytes()).collect();
    std::fs::write(with("stems"), bytes)?;
    Ok(())
}

/// A CD-XA channel alone through the output's own mix core, as a one-shot cue
/// plays: the stream's rate converter, the cue gain.
fn render_xa(xa: &Path, start: usize, samples: usize) -> Result<Vec<i16>> {
    let bytes = std::fs::read(xa).with_context(|| format!("read {}", xa.display()))?;
    let rate = u32::from_le_bytes(bytes.get(24..28).context("short WAV")?.try_into()?);
    let source = read_wav(xa)?;
    let mut mix = OfflineMixer::new(SPU_INTERNAL_RATE);
    mix.play_xa_shout(
        source.get(start..).unwrap_or_default().to_vec(),
        rate,
        legaia_xa::Channels::Mono,
        legaia_engine_session::audio_director::XA_CUE_GAIN,
        0,
    );
    let mut pcm = vec![0i16; samples * 2];
    for f in pcm.as_chunks_mut::<2>().0 {
        let (l, r) = mix.next_frame();
        *f = [l, r];
    }
    Ok(pcm)
}

/// How far the CD input lies from the XA channel. `retail` is a silenced state's
/// recording; the resumed SPU, which fits that recording to `-44` dB on a state
/// with no CD input, takes the voices out, and what is left is the CD input
/// (and any cue the game keys in the window, so the window is the caller's to pick).
fn score_xa(
    state: &legaia_mednafen::SaveState,
    retail: &[i16],
    xa: &Path,
    start: usize,
    samples: usize,
) -> Result<PcmScore> {
    let mut core = vec![0i16; samples * 2];
    spu_resumed_from_retail(state)?.render_into(&mut core);
    let cd: Vec<f64> = retail
        .as_chunks::<2>()
        .0
        .iter()
        .zip(core.as_chunks::<2>().0)
        .map(|(r, c)| {
            (f64::from(r[0]) + f64::from(r[1])) * 0.5 / MEDNAFEN_RECORD_GAIN
                - (f64::from(c[0]) + f64::from(c[1])) * 0.5
        })
        .collect();
    let ours: Vec<f64> = render_xa(xa, start, samples)?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|s| (f64::from(s[0]) + f64::from(s[1])) * 0.5)
        .collect();
    let (mut corrs, mut residuals, mut lags) = (Vec::new(), Vec::new(), Vec::new());
    let (mut re, mut ee) = (0.0, 0.0);
    for at in (XA_WINDOW..cd.len().saturating_sub(XA_WINDOW)).step_by(XA_WINDOW) {
        let a = &cd[at..at + XA_WINDOW];
        let energy: f64 = a.iter().map(|x| x * x).sum();
        let mut best: Option<(f64, i64, f64)> = None;
        for lag in -XA_LAG..=XA_LAG {
            let Some(b) =
                ours.get((at as i64 + lag) as usize..(at as i64 + lag) as usize + XA_WINDOW)
            else {
                continue;
            };
            let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
            let eb: f64 = b.iter().map(|y| y * y).sum();
            let c = dot / (energy * eb + 1e-9).sqrt();
            if best.is_none_or(|(k, ..)| c > k) {
                best = Some((c, lag, eb));
            }
        }
        let Some((c, lag, eb)) = best else { continue };
        let b = &ours[(at as i64 + lag) as usize..][..XA_WINDOW];
        let resid: f64 = a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum();
        corrs.push(c);
        residuals.push(10.0 * (resid / energy + 1e-9).log10());
        lags.push(lag);
        re += energy;
        ee += eb;
    }
    if corrs.is_empty() {
        bail!("the window holds no XA");
    }
    let windows = corrs.len();
    let pick = |v: &mut Vec<f64>, q: f64| {
        v.sort_by(f64::total_cmp);
        v[((v.len() - 1) as f64 * q) as usize]
    };
    lags.sort_unstable();
    Ok(PcmScore {
        corr_p10: pick(&mut corrs, 0.1),
        corr_median: pick(&mut corrs, 0.5),
        level_db: 10.0 * (re / ee.max(1.0)).log10(),
        residual_db: pick(&mut residuals, 0.5),
        lag: lags[lags.len() / 2],
        windows,
    })
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

/// libsnd's per-voice driver records (24 x `0x36`), `+0x10` the note owner.
const VOICE_RECORDS: usize = 0x1C_DB50;
const VOICE_STRIDE: usize = 0x36;
/// The note owner key of the cue slot (`audio_sweep::CUE_OWNER`).
const CUE_OWNER: u16 = 0x21;

/// The state's cue voices alone, from its first sample on: every other voice
/// is stood down, while the reverb tank keeps the tails already in it. The
/// track render starts on an empty tank, so the two tails sum to the state's.
fn render_cue_voices(st: &legaia_mednafen::SaveState, samples: usize) -> Result<Vec<i16>> {
    let ram = st.main_ram()?;
    let mut spu = spu_resumed_from_retail(st)?;
    for (v, voice) in spu.voices.iter_mut().enumerate() {
        let at = VOICE_RECORDS + v * VOICE_STRIDE + 0x10;
        if u16::from_le_bytes([ram[at], ram[at + 1]]) != CUE_OWNER {
            voice.adsr.level = 0;
            voice.adsr.phase = Phase::Off;
        }
    }
    let mut pcm = vec![0i16; samples * 2];
    spu.render_into(&mut pcm);
    Ok(pcm)
}

/// How far the engine's cast recording `cast` (sample `0` its seed's gate)
/// sits from the state's instant (the retail recording's sample `0`): the shift
/// within `reach_vsyncs` either way of `center` whose cast best fits what is left of
/// `retail` once the aligned track `track` (`lag` as [`score_window`] reads it)
/// is taken out. Positive delays the cast.
///
/// The reach is the state's battle frame step
/// ([`crate::retail_compare_battle::frame_step`]), not one vsync: the gate is
/// a game-frame boundary, and on a step-`3` fight the state's instant can lie
/// up to three vsyncs from the nearest one. Measured on the recordings,
/// `cort_guilty_cross_mid_cast` (step `3`) fits its cast `1.89` vsyncs before the gate puts it,
/// `cort_mystic_shield_mid_cast` and `gilium_summon_mid_cast` (step `2`) `1.45`
/// and `1.86`: all past the one vsync this window used to reach, and each a
/// single shift that holds for every second of the cast.
pub fn fit_cast_offset(
    retail: &[i16],
    track: &[i16],
    cast: &[i16],
    lag: i64,
    center: i64,
    reach_vsyncs: u8,
) -> i64 {
    let mono = |pcm: &[i16], t: i64, gain: f64| -> f64 {
        usize::try_from(t)
            .ok()
            .and_then(|t| pcm.get(t * 2..t * 2 + 2))
            .map_or(0.0, |s| (f64::from(s[0]) + f64::from(s[1])) * 0.5 / gain)
    };
    let n = (retail.len() / 2).min(cast.len() / 2);
    let left: Vec<f64> = (0..n as i64)
        .map(|t| mono(retail, t, MEDNAFEN_RECORD_GAIN) - mono(track, t + lag, 1.0))
        .collect();
    let c: Vec<f64> = (0..n as i64).map(|t| mono(cast, t, 1.0)).collect();
    let fit = |d: i64| -> f64 {
        let (mut dot, mut ee) = (0.0, 0.0);
        for (t, l) in left.iter().enumerate() {
            let k = t as i64 - d;
            if let Some(v) = usize::try_from(k).ok().and_then(|k| c.get(k)) {
                dot += l * v;
                ee += v * v;
            }
        }
        dot / ee.max(1.0).sqrt()
    };
    let reach = (f64::from(reach_vsyncs.max(1)) * RETAIL_VSYNC_SAMPLES) as i64;
    let coarse = (center - reach..=center + reach)
        .step_by(8)
        .max_by(|a, b| fit(*a).total_cmp(&fit(*b)))
        .unwrap_or(0);
    (coarse - 8..=coarse + 8)
        .max_by(|a, b| fit(*a).total_cmp(&fit(*b)))
        .unwrap_or(0)
}

/// The tick of the engine's cast recording that stands where the state does,
/// read off the summon duck: the one tick whose applied level (`<cast>.duck`)
/// is the state's applied level `DAT_8007B6EC` while the ramp moves through it.
/// A level the ramp rests on, or reaches more than once, names no tick.
/// `zenoir_summon_mid_cast` stands at `195`, which the engine's ramp passes on
/// tick `5`: its sounds fit `5.5` vsyncs early, past any frame step.
pub fn duck_anchor_tick(levels: &[u8], applied: u8) -> Option<usize> {
    let moving = |k: usize| {
        levels[k] == applied
            && levels.get(k + 1).is_some_and(|&n| n != applied)
            && (k == 0 || levels[k - 1] != applied)
    };
    let mut hits = (0..levels.len()).filter(|&k| levels[k] == applied);
    let k = hits.next()?;
    (hits.next().is_none() && moving(k)).then_some(k)
}

/// A cast's CD-seek wait in the engine's cast recording, read off its
/// `.drive` sidecar (one `0` / `1` line per recorded tick, `1` where the
/// module's arm waited on the drive): `(first held tick, ticks held)` of the
/// last such run. `None` when nothing in the recording waited.
pub fn cast_drive_wait(drive: &str) -> Option<(usize, usize)> {
    let held: Vec<bool> = drive.lines().map(|l| l.trim() == "1").collect();
    let end = held.iter().rposition(|&h| h)? + 1;
    let start = held[..end].iter().rposition(|&h| !h).map_or(0, |i| i + 1);
    Some((start, end - start))
}

/// The voice after a cast's CD-seek wait, aligned within the seek window
/// (`docs/subsystems/audio.md`, "The cast channel's seek window"): retail's
/// wait may run anywhere from no extra vsync to the engine's `held` plus
/// [`crate::cast_sequence::drive_window_margin`], so the recording from the
/// wait's end on (`split`, a cast sample) is fitted to what is left of
/// `retail` over that span of shifts around `offset`, the cast's own fit.
/// Returns the shift for the part past `split` and the window searched, in
/// samples either side of `offset` (`(-early, late)`).
pub fn fit_cast_offset_after_wait(
    retail: &[i16],
    track: &[i16],
    cast: &[i16],
    lag: i64,
    offset: i64,
    split: usize,
    held: usize,
) -> (i64, (i64, i64)) {
    let mono = |pcm: &[i16], t: i64, gain: f64| -> f64 {
        usize::try_from(t)
            .ok()
            .and_then(|t| pcm.get(t * 2..t * 2 + 2))
            .map_or(0.0, |s| (f64::from(s[0]) + f64::from(s[1])) * 0.5 / gain)
    };
    let vs = RETAIL_VSYNC_SAMPLES;
    let margin = crate::cast_sequence::drive_window_margin(held as u64) as f64;
    // Retail's wait in [0, held + margin] vsyncs: its voice lands up to `held`
    // vsyncs earlier than the engine's, or `margin` later.
    let window = (-((held as f64 * vs) as i64), (margin * vs) as i64);
    // The voice's opening seconds past the wait carry the fit.
    let span = (3 * SPU_INTERNAL_RATE as usize).min((cast.len() / 2).saturating_sub(split));
    let fit = |d: i64| -> f64 {
        let (mut dot, mut ee) = (0.0, 0.0);
        for k in split..split + span {
            let v = mono(cast, k as i64, 1.0);
            let t = k as i64 + d;
            let l = mono(retail, t, MEDNAFEN_RECORD_GAIN) - mono(track, t + lag, 1.0);
            dot += l * v;
            ee += v * v;
        }
        dot / ee.max(1.0).sqrt()
    };
    if span == 0 {
        return (offset, window);
    }
    let best = |from: i64, to: i64, step: usize| {
        (from..=to)
            .step_by(step)
            .max_by(|a, b| fit(*a).total_cmp(&fit(*b)))
            .unwrap_or(offset)
    };
    let coarse = best(offset + window.0, offset + window.1, 64);
    let mid = best(coarse - 64, coarse + 64, 8);
    (best(mid - 8, mid + 8, 1), window)
}

/// `add` into `pcm`, shifted the way [`score_window`] reads a lag: retail's
/// frame `t` stands on the engine's frame `t + lag`, and a voice resumed from
/// the state is on retail's clock.
fn mix_at_lag(pcm: &mut [i16], add: &[i16], lag: i64) {
    for (t, frame) in add.as_chunks::<2>().0.iter().enumerate() {
        let Ok(at) = usize::try_from(t as i64 + lag) else {
            continue;
        };
        let Some(out) = pcm.get_mut(at * 2..at * 2 + 2) else {
            break;
        };
        out[0] = out[0].saturating_add(frame[0]);
        out[1] = out[1].saturating_add(frame[1]);
    }
}

pub fn run(args: PcmRetailArgs) -> Result<()> {
    let mut cast_drive: Option<serde_json::Value> = None;
    let (pcm, label) = if let Some(state) = &args.resume_state {
        let st = legaia_mednafen::SaveState::from_path(state)?;
        let mut spu = spu_resumed_from_retail(&st)?;
        let mut pcm = vec![0i16; args.samples * 2];
        if let Some(xa) = &args.xa_wav {
            let retail = read_wav(
                args.retail_wav
                    .as_deref()
                    .context("--xa-wav needs --retail-wav")?,
            )?;
            let score = score_xa(&st, &retail, xa, args.xa_start, args.samples)?;
            println!(
                "{}: xa corr {:.4} (p10 {:.4}) level {:+.2} dB residual {:.1} dB lag {} over {} windows",
                state.display(),
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
            return Ok(());
        }
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
            let st = legaia_mednafen::SaveState::from_path(&path)?;
            let psx = legaia_mednafen::PsxSpu::new(&st);
            for (i, v) in psx
                .voices()
                .iter()
                .enumerate()
                .filter(|(_, v)| v.is_active())
            {
                println!(
                    "retail v{i}: phase {:?} env {:x?} ctl {:x?} pitch {:x?} start {:x?} vol {:?} {:?}",
                    v.adsr_phase,
                    v.adsr_env_level,
                    v.adsr_control,
                    v.pitch,
                    v.start_addr,
                    v.vol_left,
                    v.vol_right
                );
            }
        }
        let mut pcm = render_aligned(
            &track_bytes,
            &pos,
            args.samples,
            args.vsync_samples,
            args.pass,
            args.verbose,
            args.stems.as_deref(),
            None,
        )?;
        if args.cue_voices {
            let retail = read_wav(args.retail_wav.as_deref().context("--retail-wav")?)?;
            let lag = score_window(&retail, &pcm).lag;
            let st = legaia_mednafen::SaveState::from_path(&path)?;
            let cues = render_cue_voices(&st, args.samples)?;
            mix_at_lag(&mut pcm, &cues, lag);
        }
        if let Some(cast_path) = &args.cast_wav {
            let retail = read_wav(args.retail_wav.as_deref().context("--retail-wav")?)?;
            let lag = score_window(&retail, &pcm).lag;
            // The score under the cast's duck, as the engine child applied it
            // tick by tick: the state's own volume holds for none of a summon.
            if let Ok(text) = std::fs::read_to_string(cast_path.with_extension("duck")) {
                let levels: Vec<u8> = text.lines().filter_map(|l| l.trim().parse().ok()).collect();
                let st = legaia_mednafen::SaveState::from_path(&path)?;
                let ram = st.main_ram()?;
                let start =
                    i32::from_le_bytes(ram[0x7_B910..0x7_B914].try_into()?).clamp(0, 255) as u8;
                let vols = duck_schedule(&levels, start, lag, args.vsync_samples);
                if !vols.is_empty() {
                    pcm = render_aligned(
                        &track_bytes,
                        &pos,
                        args.samples,
                        args.vsync_samples,
                        args.pass,
                        false,
                        None,
                        Some(&vols),
                    )?;
                }
            }
            let lag = score_window(&retail, &pcm).lag;
            let cast = read_wav(cast_path)?;
            // A CD-seek wait in the cast: the voice past it may start anywhere
            // in the seek window, and is aligned there on its own.
            let wait = std::fs::read_to_string(cast_path.with_extension("drive"))
                .ok()
                .and_then(|t| cast_drive_wait(&t));
            let split = wait.map(|(start, held)| {
                (((start + held) as f64 * args.vsync_samples) as usize).min(cast.len() / 2)
            });
            // The child's gate tick stands near the state's instant, not on its
            // sample: the cast's own offset is fitted against what the track
            // leaves of the recording, and reported. The gate is a game-frame
            // boundary, so the state's instant can lie up to a frame step off
            // it ([`fit_cast_offset`]); a fit that reaches that far can also
            // lock a misplaced voice onto a louder stretch, so the frame-step
            // reach and the one-vsync reach are both mixed and the better
            // scoring kept - one alignment parameter, chosen on the score.
            let step = {
                let st = legaia_mednafen::SaveState::from_path(&path)?;
                crate::retail_compare_battle::frame_step(st.main_ram()?)
            };
            let track = pcm;
            let mut best: Option<(f64, Vec<i16>, Option<serde_json::Value>)> = None;
            let mut reaches = vec![(0i64, 1u8)];
            if step > 1 {
                reaches.push((0, step));
            }
            // A state taken mid-ramp names its own instant on the engine's
            // recording: the tick the engine's applied level passes the
            // state's ([`duck_anchor_tick`]).
            let applied = {
                let st = legaia_mednafen::SaveState::from_path(&path)?;
                let ram = st.main_ram()?;
                i32::from_le_bytes(ram[0x7_B6EC..0x7_B6F0].try_into()?).clamp(0, 255) as u8
            };
            let levels: Vec<u8> = std::fs::read_to_string(cast_path.with_extension("duck"))
                .map(|t| t.lines().filter_map(|l| l.trim().parse().ok()).collect())
                .unwrap_or_default();
            if let Some(k) = duck_anchor_tick(&levels, applied) {
                reaches.push((-((k as f64 * args.vsync_samples) as i64), 1));
            }
            for (center, reach) in reaches {
                let offset = fit_cast_offset(&retail, &track, &cast, lag, center, reach);
                let mut mixed = track.clone();
                let mut drive = None;
                let mut notes = vec![format!(
                    "{}: cast offset {offset:+} samples on the track's lag {lag} (reach {reach} vsyncs of {center:+})",
                    args.label
                )];
                match wait.zip(split) {
                    Some(((start, held), split)) => {
                        let (after, window) = fit_cast_offset_after_wait(
                            &retail, &track, &cast, lag, offset, split, held,
                        );
                        notes.push(format!(
                            "{}: cast CD-seek wait {held} ticks from tick {start}: voice offset {after:+} \
                             samples ({:+.1} vsyncs past the cast's), window {:+}..={:+} samples",
                            args.label,
                            (after - offset) as f64 / args.vsync_samples,
                            window.0,
                            window.1
                        ));
                        mix_at_lag(&mut mixed, &cast[..split * 2], lag + offset);
                        mix_at_lag(&mut mixed, &cast[split * 2..], lag + after + split as i64);
                        drive = Some(serde_json::json!({
                            "wait_start_tick": start,
                            "held_ticks": held,
                            "offset": offset,
                            "voice_offset": after,
                            "window": [window.0, window.1],
                        }));
                    }
                    None => mix_at_lag(&mut mixed, &cast, lag + offset),
                }
                let corr = score_window(&retail, &mixed).corr_median;
                for n in &notes {
                    println!("{n} -> corr {corr:.4}");
                }
                if best.as_ref().is_none_or(|(c, _, _)| corr > *c) {
                    best = Some((corr, mixed, drive));
                }
            }
            let (_, mixed, drive) = best.expect("one reach is always tried");
            pcm = mixed;
            cast_drive = drive;
        }
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
        if args.per_second {
            let retail = read_wav(p)?;
            for (i, s) in per_second_scores(&retail, &pcm, score.lag)
                .iter()
                .enumerate()
            {
                println!(
                    "  {i:>3}s: corr {:.4} level {:+.2} dB residual {:.1} dB",
                    s.corr_median, s.level_db, s.residual_db
                );
            }
        }
        if let Some(out) = &args.json {
            let mut v = serde_json::to_value(&score)?;
            if let (Some(d), Some(m)) = (cast_drive.take(), v.as_object_mut()) {
                m.insert("cast_drive".into(), d);
            }
            std::fs::write(out, serde_json::to_string_pretty(&v)?)?;
        }
    }
    Ok(())
}

/// Level and correlation one second at a time at the fixed lag `lag` (as
/// [`score_window`] reads one: retail's frame `t` on the engine's `t + lag`):
/// where in a recording the two part. A second retail holds silent reads
/// nothing.
pub fn per_second_scores(retail: &[i16], engine: &[i16], lag: i64) -> Vec<PcmScore> {
    let mono = |pcm: &[i16], t: i64, gain: f64| -> f64 {
        usize::try_from(t)
            .ok()
            .and_then(|t| pcm.get(t * 2..t * 2 + 2))
            .map_or(0.0, |s| (f64::from(s[0]) + f64::from(s[1])) * 0.5 / gain)
    };
    let second = SPU_INTERNAL_RATE as usize;
    let span = (retail.len() / 2).min(engine.len() / 2);
    (0..span / second)
        .map(|i| {
            let (mut rr, mut ee, mut re, mut d) = (0.0, 0.0, 0.0, 0.0);
            for t in i * second..(i + 1) * second {
                let r = mono(retail, t as i64, MEDNAFEN_RECORD_GAIN);
                let e = mono(engine, t as i64 + lag, 1.0);
                rr += r * r;
                ee += e * e;
                re += r * e;
                d += (r - e) * (r - e);
            }
            if (rr / second as f64).sqrt() < SILENT_RMS {
                return PcmScore::default();
            }
            PcmScore {
                corr_median: re / (rr * ee + 1e-9).sqrt(),
                corr_p10: 0.0,
                level_db: 10.0 * (rr / ee.max(1.0)).log10(),
                residual_db: 10.0 * (d / rr + 1e-9).log10(),
                lag,
                windows: 1,
            }
        })
        .collect()
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
/// [`score_xa`]'s window, 50 ms, and how far either way it looks.
const XA_WINDOW: usize = 2_205;
const XA_LAG: i64 = 64;
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

    /// The `.drive` sidecar's last run of waiting ticks is the wait the voice
    /// follows; a recording that never waited has none.
    #[test]
    fn the_drive_sidecar_names_the_last_wait() {
        assert_eq!(cast_drive_wait("0\n0\n1\n1\n1\n0\n"), Some((2, 3)));
        assert_eq!(cast_drive_wait("1\n1\n0\n0\n1\n0\n"), Some((4, 1)));
        assert_eq!(cast_drive_wait("1\n1\n"), Some((0, 2)));
        assert_eq!(cast_drive_wait("0\n0\n"), None);
        assert_eq!(cast_drive_wait(""), None);
    }

    /// The voice past the wait is found wherever the window puts it: a cast
    /// whose voice retail started `held` vsyncs sooner fits at that shift.
    #[test]
    fn the_voice_after_a_wait_fits_inside_the_window() {
        let n = 4 * SPU_INTERNAL_RATE as usize;
        let tone = |t: usize| ((t as f64 * 0.05).sin() * (t as f64 * 0.0007).cos() * 8000.0) as i16;
        let (split, held, early) = (20_000usize, 20usize, 7_000usize);
        // Engine: the voice from `split + early`; retail: from `split`.
        let mut cast = vec![0i16; n * 2];
        let mut retail = vec![0i16; n * 2];
        for t in 0..n - split - early {
            let v = tone(t);
            cast[(split + early + t) * 2] = v;
            cast[(split + early + t) * 2 + 1] = v;
            let r = (f64::from(v) * MEDNAFEN_RECORD_GAIN) as i16;
            retail[(split + t) * 2] = r;
            retail[(split + t) * 2 + 1] = r;
        }
        let track = vec![0i16; n * 2];
        let (got, window) = fit_cast_offset_after_wait(&retail, &track, &cast, 0, 0, split, held);
        assert_eq!(got, -(early as i64));
        assert!(window.0 <= got && got <= window.1);
    }

    #[test]
    fn the_duck_ramps_from_the_states_own_level() {
        // The engine ramps 215 -> 161 a unit a vsync; a state taken at 192
        // reaches 161 in 31 vsyncs, as `zenoir_summon_mid_cast` does.
        let engine: Vec<u8> = (161..=215)
            .rev()
            .chain(std::iter::repeat_n(161, 20))
            .collect();
        let v = duck_schedule(&engine, 192, 0, RETAIL_VSYNC_SAMPLES);
        assert_eq!(v[0], 96);
        assert_eq!(v[31], 80);
        assert_eq!(*v.last().unwrap(), 80);
        // At the engine's own start the schedule is the engine's level halved.
        let same = duck_schedule(&engine, 215, 0, RETAIL_VSYNC_SAMPLES);
        assert!(same.iter().zip(&engine).all(|(v, l)| *v == l >> 1));
    }

    #[test]
    fn a_resumed_voice_lands_on_the_tracks_lag() {
        let mut pcm = vec![0i16; 12];
        mix_at_lag(&mut pcm, &[1, 2, 3, 4, 5, 6], -1);
        assert_eq!(
            &pcm[..6],
            &[3, 4, 5, 6, 0, 0],
            "frame 0 falls before sample 0"
        );
        let mut pcm = vec![0i16; 6];
        mix_at_lag(&mut pcm, &[1, 2, 3, 4, 5, 6], 2);
        assert_eq!(pcm, [0, 0, 0, 0, 1, 2], "the rest runs off the end");
    }

    /// A cast whose sounds land two vsyncs ahead of the state's instant is out
    /// of a one-vsync reach and inside a step-`3` frame's.
    #[test]
    fn the_duck_names_the_states_tick_only_mid_ramp() {
        let ramp = [200, 199, 198, 197, 196, 195, 194, 193];
        assert_eq!(duck_anchor_tick(&ramp, 195), Some(5));
        assert_eq!(duck_anchor_tick(&ramp, 215), None, "not on the ramp");
        let rest = [213, 212, 211, 210, 210, 210];
        assert_eq!(
            duck_anchor_tick(&rest, 210),
            None,
            "a level the ramp rests on"
        );
        assert_eq!(duck_anchor_tick(&rest, 212), Some(1));
        assert_eq!(duck_anchor_tick(&[215; 8], 215), None, "no ramp");
    }

    #[test]
    fn the_cast_offset_reaches_a_frame_step() {
        let n = 20_000;
        let early = 1400usize;
        // A click train no shorter shift can mimic: sparse, irregular bursts.
        let burst = |at: usize, len: usize| -> Vec<i16> {
            let mut v = vec![0i16; len * 2];
            for (k, &p) in [300usize, 2100, 4700, 7300, 9900, 12_800, 15_100]
                .iter()
                .enumerate()
            {
                for j in 0..400 {
                    let t = p + j + at;
                    if t < len {
                        let x = ((j as f64) * (0.05 + k as f64 * 0.013)).sin() * 6000.0;
                        v[t * 2] = x as i16;
                        v[t * 2 + 1] = x as i16;
                    }
                }
            }
            v
        };
        // Retail hears each burst at `p`, the engine's recording has it at
        // `p + early`: the cast sits `early` samples late, offset `-early`.
        let retail: Vec<i16> = burst(0, n)
            .iter()
            .map(|&x| (f64::from(x) * MEDNAFEN_RECORD_GAIN) as i16)
            .collect();
        let cast = burst(early, n);
        let track = vec![0i16; n * 2];
        let near = fit_cast_offset(&retail, &track, &cast, 0, 0, 1);
        let far = fit_cast_offset(&retail, &track, &cast, 0, 0, 3);
        assert!(
            (far + early as i64).abs() <= 1,
            "step reach finds it: {far}"
        );
        assert!(
            near.abs() <= RETAIL_VSYNC_SAMPLES as i64,
            "one vsync stays inside: {near}"
        );
        assert!((near + early as i64).abs() > 100, "and misses it: {near}");
    }
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
