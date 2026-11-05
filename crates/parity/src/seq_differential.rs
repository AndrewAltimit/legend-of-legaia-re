//! The state's own sequence driven by retail's libsnd and by the port, vsync by vsync.
//!
//! `crates/retail-exec` runs `FUN_80062F98` (`SsSeqCalc`) off the RAM of a library
//! state, with libspu's register base `0x8007AF40` pointed at a block of RAM, so
//! every register write lands where the harness can read it. The port's `Spu` is
//! the hardware model on both sides: retail's writes are applied to one, and that
//! one's envelopes are handed back as `ENVX`, which is what the allocation scan
//! reads. Both sides start the score from the top on an idle voice file, and every
//! sounding voice's registers are compared at every vsync. See
//! `docs/subsystems/audio.md`, "The keying differential".

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use legaia_engine_audio::spu::adsr::{AdsrConfig, Phase};
use legaia_engine_audio::sequencer::flush_key_on_lag;
use legaia_engine_audio::spu::voice::PITCH_UNITY;
use legaia_engine_audio::{Spu, spu::NUM_VOICES};
use legaia_engine_scene_host::scene_host::{BgmDirector, SceneHost};
use legaia_ram_image::ScenarioManifest;
use legaia_retail_exec::{Cpu, GP, Stop};

use crate::audio_sweep::{RetailAudio, TrackBank};
use crate::audio_trace_oracle::TraceBgmDirector;
use crate::pcm_retail::{RETAIL_VSYNC_SAMPLES, spu_resumed_from_retail};
use crate::retail_compare::enumerate_corpus;

/// libspu's register base word (`DAT_8007AF40`).
const SPU_BASE_WORD: u32 = 0x8007_AF40;
/// Where the registers land instead: the kernel's RAM, which nothing here reaches.
const MIRROR: u32 = 0x8000_8000;
const REG_KON: u32 = MIRROR + 0x188;
const REG_KOFF: u32 = MIRROR + 0x18C;
const REG_EON: u32 = MIRROR + 0x198;
/// libspu's transfer-busy word: bit 0 parks a KON in RAM instead of the register.
const SPU_BUSY_WORD: u32 = 0x8007_AF38;
const SEQ_CALC: u32 = 0x8006_2F98;
const SEQ_INIT: u32 = 0x8006_2410;
const SEQ_PLAY: u32 = 0x8006_28F0;
const SEQ_LOCK: u32 = 0x801C_D2B4;
const SEQ_BITMAP: u32 = 0x801C_D2B8;
const SEQ_TABLE: u32 = 0x801C_D2C0;
const VOICE_RECORDS: u32 = 0x801C_DB50;
const VOICE_STRIDE: u32 = 0x36;
const SILENT_RING: u32 = 0x801C_E208;
/// The key-on / key-off accumulators and the sounding mask.
const MASK_WORDS: [u32; 3] = [0x801C_DB48, 0x801C_DB4C, 0x801C_E248];
const VOICE_FLAGS: u32 = 0x801C_E060;
/// libsnd's mono fold (`_DAT_801CE330`): the option a state was captured under.
const MONO_FOLD: u32 = 0x801C_E330;
const SHADOW_REGS: u32 = 0x801C_E080;

/// One sounding voice as the SPU holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Sounding {
    pub pitch: u16,
    pub vol: (i16, i16),
    pub adsr: (u16, u16),
    pub level: u16,
    pub releasing: bool,
    pub reverb: bool,
}

/// What one side did in one vsync.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VsyncView {
    /// Voices keyed this vsync.
    pub keyed: Vec<(u8, Sounding)>,
    /// Voices that went into release this vsync.
    pub released: Vec<u8>,
    /// Every voice with its envelope up, after the vsync's samples.
    pub sounding: Vec<(u8, Sounding)>,
}

fn view_of(spu: &Spu, before: &[(u32, Phase); NUM_VOICES]) -> VsyncView {
    let mut out = VsyncView::default();
    for (i, v) in spu.voices.iter().enumerate() {
        let s = Sounding {
            pitch: v.pitch,
            vol: (v.vol_left, v.vol_right),
            adsr: v.adsr_cfg.raw,
            level: v.adsr.level as u16,
            releasing: v.adsr.phase == Phase::Release,
            reverb: v.reverb_send,
        };
        if v.key_on_count() != before[i].0 {
            out.keyed.push((i as u8, s));
        }
        if !v.is_off() {
            out.sounding.push((i as u8, s));
        }
    }
    out
}

fn stamps(spu: &Spu) -> [(u32, Phase); NUM_VOICES] {
    std::array::from_fn(|i| (spu.voices[i].key_on_count(), spu.voices[i].adsr.phase))
}

/// Which voices left a held phase for release between `before` and now. Read
/// ahead of the samples, so a release's first step is not what is being asked.
fn released_since(spu: &Spu, before: &[(u32, Phase); NUM_VOICES]) -> Vec<u8> {
    (0..NUM_VOICES)
        .filter(|&i| {
            spu.voices[i].adsr.phase == Phase::Release
                && !matches!(before[i].1, Phase::Release | Phase::Off)
        })
        .map(|i| i as u8)
        .collect()
}

/// Retail's libsnd over a state's RAM, its register writes applied to a model SPU.
pub struct RetailSide {
    cpu: Cpu,
    pub spu: Spu,
    carry: f64,
}

impl RetailSide {
    /// Stand the state's sequence on `slot` at the top of its score, on an idle
    /// voice file, with `seq_vol` for the sequence volume.
    pub fn new(ram: &[u8], mut spu: Spu, slot: u8, seq_vol: u8) -> Result<Self> {
        let mut cpu = Cpu::new();
        cpu.mem.load(0x8000_0000, ram);
        cpu.regs.r[GP] = 0x8007_B318;
        cpu.step_budget = 20_000_000;
        cpu.hardware_zero_div = true;
        cpu.mem.wr32(SPU_BASE_WORD, MIRROR);
        let busy = cpu.mem.rd32(SPU_BUSY_WORD);
        cpu.mem.wr32(SPU_BUSY_WORD, busy & !1);
        cpu.mem.wr32(SEQ_LOCK, 0);
        cpu.mem.wr16(MONO_FOLD, 0);
        cpu.mem.wr16(SEQ_BITMAP, 1 << slot);
        for w in MASK_WORDS {
            cpu.mem.wr32(w, 0);
        }
        for i in 0..NUM_VOICES as u32 {
            for b in 0..VOICE_STRIDE {
                cpu.mem.wr8(VOICE_RECORDS + i * VOICE_STRIDE + b, 0);
            }
            cpu.mem.wr8(VOICE_FLAGS + i, 0);
            for b in 0..16 {
                cpu.mem.wr8(SHADOW_REGS + i * 16 + b, 0);
            }
        }
        for i in 0..16 {
            cpu.mem.wr32(SILENT_RING + i * 4, 0);
        }
        for v in &mut spu.voices {
            v.adsr.level = 0;
            v.adsr.phase = Phase::Off;
        }
        let rec = cpu.mem.rd32(SEQ_TABLE + u32::from(slot) * 4);
        let start = cpu.mem.rd32(rec + 4);
        // `+0x04` stands past the header and the score's first delta.
        let header = (1..64u32)
            .map(|back| start - back)
            .find(|&a| cpu.mem.read(a, 4) == *b"pQES")
            .context("no pQES ahead of the score start")?;
        let vab = u32::from(cpu.mem.rd8(rec + 0x26));
        let plays = u32::from(cpu.mem.rd8(rec + 0x20));
        cpu.mem.wr32(rec + 0x98, 0);
        expect_return(cpu.call(SEQ_INIT, &[u32::from(slot), vab, header]))?;
        cpu.mem.wr16(rec + 0x58, u16::from(seq_vol));
        cpu.mem.wr16(rec + 0x5A, u16::from(seq_vol));
        expect_return(cpu.call(SEQ_PLAY, &[u32::from(slot), 0, 1, plays]))?;
        Ok(Self {
            cpu,
            spu,
            carry: 0.0,
        })
    }

    /// One vsync: the calc, its writes on the SPU, the samples, `ENVX` back.
    pub fn step(&mut self, samples: f64) -> Result<VsyncView> {
        // What the flush is about to write: the port's lag on the KON write.
        let staged = (0..NUM_VOICES as u32)
            .filter(|&i| self.cpu.mem.rd8(VOICE_FLAGS + i) != 0)
            .count() as u32;
        expect_return(self.cpu.call(SEQ_CALC, &[]))?;
        let before = stamps(&self.spu);
        let mem = &mut self.cpu.mem;
        let kon = mem.rd32(REG_KON);
        let koff = mem.rd32(REG_KOFF);
        let eon = mem.rd32(REG_EON);
        if std::env::var_os("SEQDIFF_TRACE").is_some() {
            eprintln!("kon {kon:06x} koff {koff:06x} accum {:08x} sounding {:08x} steps {} calls {:x?}", mem.rd32(MASK_WORDS[0]), mem.rd32(MASK_WORDS[2]), self.cpu.steps, &self.cpu.calls[..self.cpu.calls.len().min(8)]);
        }
        mem.wr32(REG_KON, 0);
        mem.wr32(REG_KOFF, 0);
        let mut regs = [[0u16; 8]; NUM_VOICES];
        for (i, r) in regs.iter_mut().enumerate() {
            for (k, w) in r.iter_mut().enumerate() {
                *w = mem.rd16(MIRROR + i as u32 * 16 + k as u32 * 2);
            }
        }
        for (i, voice) in self.spu.voices.iter_mut().enumerate() {
            let r = &regs[i];
            voice.vol_left = r[0] as i16;
            voice.vol_right = r[1] as i16;
            voice.pitch = r[2].min(PITCH_UNITY * 4);
            voice.reverb_send = eon & (1 << i) != 0;
            if koff & (1 << i) != 0 {
                voice.key_off();
            }
        }
        let ram = self.spu.ram.clone();
        for (i, voice) in self.spu.voices.iter_mut().enumerate() {
            let r = &regs[i];
            if kon & (1 << i) != 0 {
                voice.start_addr = u32::from(r[3]) * 8;
                voice.adsr_cfg = AdsrConfig::from_words(r[4], r[5]);
                voice.loop_addr = None;
                voice.key_on(&ram);
                voice.delay_start(flush_key_on_lag(staged));
            }
        }
        let released = released_since(&self.spu, &before);
        self.carry += samples;
        let n = self.carry.floor() as usize;
        self.carry -= n as f64;
        let mut buf = vec![0i16; n * 2];
        self.spu.render_into(&mut buf);
        let mem = &mut self.cpu.mem;
        for (i, voice) in self.spu.voices.iter().enumerate() {
            let level = if voice.is_off() { 0 } else { voice.adsr.level };
            mem.wr16(MIRROR + i as u32 * 16 + 0xC, level as u16);
        }
        let mut view = view_of(&self.spu, &before);
        view.released = released;
        Ok(view)
    }
}

fn expect_return(s: Stop) -> Result<()> {
    match s {
        Stop::Returned => Ok(()),
        other => bail!("retail call stopped: {other:x?}"),
    }
}

/// The port's sequencer over its own SPU.
pub struct PortSide {
    d: TraceBgmDirector,
    carry: f64,
}

impl PortSide {
    pub fn new(track: u16, entry: &[u8], seq_vol: u8) -> Self {
        let mut d = TraceBgmDirector::new();
        d.master_vol = seq_vol;
        d.start_owned_vab(track, entry);
        Self { d, carry: 0.0 }
    }

    pub fn step(&mut self, samples: f64) -> VsyncView {
        let before = stamps(self.d.spu());
        let spu: *mut Spu = self.d.spu_mut();
        if let Some(s) = self.d.sequencer_mut() {
            // SAFETY: the director's SPU and sequencer are disjoint fields.
            s.step_vsync(unsafe { &mut *spu });
        }
        let released = released_since(self.d.spu(), &before);
        self.carry += samples;
        let n = self.carry.floor() as usize;
        self.carry -= n as f64;
        let mut buf = vec![0i16; n * 2];
        self.d.spu_mut().render_into(&mut buf);
        let mut view = view_of(self.d.spu(), &before);
        view.released = released;
        view
    }
}

/// The comparison of one track's run.
#[derive(Debug, Default)]
pub struct Differential {
    pub vsyncs: usize,
    pub lag: i64,
    pub retail_keyons: usize,
    pub port_keyons: usize,
    /// Vsyncs whose keyed notes differ, voice numbers aside.
    pub key_diffs: usize,
    /// Vsyncs whose keyed voice numbers differ with the notes alike.
    pub voice_diffs: usize,
    /// Vsyncs whose released voice count differs.
    pub release_diffs: usize,
    /// Vsyncs whose sounding set differs, voice numbers and levels aside.
    pub sound_diffs: usize,
    /// Vsyncs whose envelope levels alone differ.
    pub level_diffs: usize,
    pub first: Vec<String>,
}

fn note(s: &Sounding) -> (u16, (i16, i16), (u16, u16), bool) {
    (s.pitch, s.vol, s.adsr, s.reverb)
}

/// Compare two runs, the port's shifted by the lag of the first key-on.
pub fn compare(retail: &[VsyncView], port: &[VsyncView], keep: usize) -> Differential {
    let first = |r: &[VsyncView]| r.iter().position(|v| !v.keyed.is_empty()).unwrap_or(0) as i64;
    let lag = first(retail) - first(port);
    let mut out = Differential {
        lag,
        retail_keyons: retail.iter().map(|v| v.keyed.len()).sum(),
        port_keyons: port.iter().map(|v| v.keyed.len()).sum(),
        ..Default::default()
    };
    let note_it = |out: &mut Differential, s: String| {
        let kind = s.split_whitespace().nth(1).unwrap_or("").to_string();
        if out.first.iter().filter(|f| f.split_whitespace().nth(1) == Some(&kind)).count() < keep {
            out.first.push(s);
        }
    };
    for (i, r) in retail.iter().enumerate() {
        let Some(p) = usize::try_from(i as i64 - lag).ok().and_then(|j| port.get(j)) else {
            continue;
        };
        out.vsyncs += 1;
        let sort = |v: &[(u8, Sounding)]| {
            let mut n: Vec<_> = v.iter().map(|(_, s)| note(s)).collect();
            n.sort();
            n
        };
        let (rk, pk) = (sort(&r.keyed), sort(&p.keyed));
        if rk != pk {
            out.key_diffs += 1;
            note_it(
                &mut out,
                format!("v{i} key: retail {:x?} port {:x?}", r.keyed, p.keyed),
            );
        } else if r.keyed.iter().map(|k| k.0).collect::<Vec<_>>()
            != p.keyed.iter().map(|k| k.0).collect::<Vec<_>>()
        {
            out.voice_diffs += 1;
            let ids = |v: &[(u8, Sounding)]| v.iter().map(|k| k.0).collect::<Vec<_>>();
            note_it(
                &mut out,
                format!("v{i} voice: retail {:?} port {:?}", ids(&r.keyed), ids(&p.keyed)),
            );
        }
        if r.released.len() != p.released.len() {
            out.release_diffs += 1;
            note_it(
                &mut out,
                format!("v{i} release: retail {:?} port {:?}", r.released, p.released),
            );
        }
        // A voice's release tail stops on its own envelope, which the flush lag
        // moves by a few samples: a tail a vsync apart is not a difference.
        let held = |v: &[(u8, Sounding)]| {
            v.iter()
                .filter(|(_, s)| !s.releasing)
                .copied()
                .collect::<Vec<_>>()
        };
        let (rs, ps) = (sort(&held(&r.sounding)), sort(&held(&p.sounding)));
        if rs != ps {
            out.sound_diffs += 1;
            note_it(
                &mut out,
                format!(
                    "v{i} sounding: retail-only {:x?} port-only {:x?}",
                    rs.iter().filter(|s| !ps.contains(s)).collect::<Vec<_>>(),
                    ps.iter().filter(|s| !rs.contains(s)).collect::<Vec<_>>()
                ),
            );
        } else {
            let levels = |v: &[(u8, Sounding)]| {
                let mut n: Vec<_> = v.iter().map(|(_, s)| (note(s), s.level, s.releasing)).collect();
                n.sort();
                n
            };
            if levels(&held(&r.sounding)) != levels(&held(&p.sounding)) {
                out.level_diffs += 1;
            }
        }
    }
    out
}

#[derive(Debug, clap::Args)]
pub struct SeqDifferentialArgs {
    /// Only labels containing this (comma = any of several). Without it, the
    /// first mednafen state of every track the library holds.
    #[arg(long)]
    pub filter: Option<String>,
    /// Vsyncs to run from the top of each score.
    #[arg(long, default_value_t = 60 * 240)]
    pub vsyncs: usize,
    /// Divergences to print per track.
    #[arg(long, default_value_t = 6)]
    pub keep: usize,
    /// Every matching state, not one per track.
    #[arg(long)]
    pub all_states: bool,
}

pub fn run(args: SeqDifferentialArgs) -> Result<()> {
    let (m, l, e) = crate::retail_compare::resolve_dirs();
    let manifest = ScenarioManifest::from_path(&m.context("no scripts/scenarios.toml")?)?;
    let library = l.context("no save library")?;
    let extracted = e.context("no extracted disc")?;
    let host = SceneHost::open_extracted(&extracted)?;
    let bank = TrackBank::load(&host);
    let mut seen: BTreeMap<u16, String> = BTreeMap::new();
    let mut corpus: Vec<(String, PathBuf)> = enumerate_corpus(&manifest, &library)
        .into_iter()
        .filter(|c| c.emulator == "mednafen")
        .map(|c| (c.label, c.path))
        .collect();
    corpus.sort();
    for (label, path) in corpus {
        if args
            .filter
            .as_deref()
            .is_some_and(|f| !f.split(',').any(|p| label.contains(p)))
        {
            continue;
        }
        let Ok(st) = legaia_mednafen::SaveState::from_path(&path) else {
            continue;
        };
        let Ok(ram) = st.main_ram().map(<[u8]>::to_vec) else {
            continue;
        };
        let audio = RetailAudio::from_ram(&ram, &bank, None);
        let Some((slot, track, vol)) = audio
            .music_slot()
            .and_then(|s| Some((s.slot, s.track?, s.vol.0 as u8)))
        else {
            continue;
        };
        if !args.all_states && seen.contains_key(&track) {
            continue;
        }
        let Ok(Some(entry)) = host.music_bank_entry_bytes(track) else {
            continue;
        };
        seen.insert(track, label.clone());
        match run_one(&st, &ram, slot, track, vol, &entry, args.vsyncs, args.keep) {
            Ok(d) => {
                println!(
                    "{track} {label}: {} vsyncs lag {} keyons {}/{} key {} voice {} release {} sound {} level {}",
                    d.vsyncs,
                    d.lag,
                    d.retail_keyons,
                    d.port_keyons,
                    d.key_diffs,
                    d.voice_diffs,
                    d.release_diffs,
                    d.sound_diffs,
                    d.level_diffs
                );
                for f in &d.first {
                    println!("    {f}");
                }
            }
            Err(err) => println!("{track} {label}: [fail] {err:#}"),
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_one(
    st: &legaia_mednafen::SaveState,
    ram: &[u8],
    slot: u8,
    track: u16,
    vol: u8,
    entry: &[u8],
    vsyncs: usize,
    keep: usize,
) -> Result<Differential> {
    let spu = spu_resumed_from_retail(st)?;
    let mut retail = RetailSide::new(ram, spu, slot, vol)?;
    let mut port = PortSide::new(track, entry, vol);
    let mut rv = Vec::with_capacity(vsyncs);
    let mut pv = Vec::with_capacity(vsyncs);
    let dump = std::env::var("SEQDIFF_DUMP").ok().and_then(|v| v.parse::<usize>().ok());
    for i in 0..vsyncs {
        rv.push(retail.step(RETAIL_VSYNC_SAMPLES)?);
        pv.push(port.step(RETAIL_VSYNC_SAMPLES));
        if dump == Some(i) {
            let m = &mut retail.cpu.mem;
            let rec = m.rd32(SEQ_TABLE + u32::from(slot) * 4);
            println!("rec {rec:08x} pans {:02x?} progs {:02x?} vols {:04x?}", m.read(rec + 0x27, 16), m.read(rec + 0x37, 16), (0..16).map(|k| m.rd16(rec + 0x60 + k * 2)).collect::<Vec<_>>());
            for v in 0..24u32 {
                let r = VOICE_RECORDS + v * VOICE_STRIDE;
                let b = m.read(r, VOICE_STRIDE as usize);
                println!("v{v} {b:02x?}");
            }
            println!("retail {:x?}", rv[i].sounding);
            println!("port {:x?}", pv[i].sounding);
        }
    }
    Ok(compare(&rv, &pv, keep))
}
