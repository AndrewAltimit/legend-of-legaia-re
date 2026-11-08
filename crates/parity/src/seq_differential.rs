//! The state's own sequence driven by retail's libsnd and by the port, vsync by vsync.
//!
//! `crates/retail-exec` runs `FUN_80062F98` (`SsSeqCalc`) off the RAM of a library
//! state, with libspu's register base `0x8007AF40` pointed at a block of RAM, so
//! every register write lands where the harness can read it. The port's `Spu` is
//! the hardware model on both sides, holding the samples where the port's bank put
//! them: retail's writes are applied to one, and that one's envelopes are handed
//! back as `ENVX`, which is what the allocation scan reads. Both sides start the
//! score from the top on an idle voice file, and every sounding voice's registers
//! are compared at every vsync. See
//! `docs/subsystems/audio.md`, "The keying differential".

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use legaia_engine_audio::sequencer::flush_key_on_lag;
use legaia_engine_audio::spu::adsr::{AdsrConfig, Phase};
use legaia_engine_audio::spu::voice::PITCH_UNITY;
use legaia_engine_audio::{Spu, spu::NUM_VOICES};
use legaia_engine_scene_host::scene_host::{BgmDirector, SceneHost};
use psx_ram_image::ScenarioManifest;
use psx_retail_exec::{Cpu, GP, Stop};

use crate::audio_sweep::{RetailAudio, TrackBank};
use crate::audio_trace_oracle::TraceBgmDirector;
use crate::pcm_retail::RETAIL_VSYNC_SAMPLES;
use crate::retail_compare::enumerate_corpus;

/// libspu's register base word (`DAT_8007AF40`).
const SPU_BASE_WORD: u32 = 0x8007_AF40;
/// Where the registers land instead: the kernel's RAM, which nothing here reaches.
const MIRROR: u32 = 0x8000_8000;
const REG_KON: u32 = MIRROR + 0x188;
const REG_KOFF: u32 = MIRROR + 0x18C;
const REG_EON: u32 = MIRROR + 0x198;
/// libspu's event-queueing word: bit 0 stages a KON in RAM instead of the
/// register. Only `SpuInit` stores it (zero); the harness clears the bit so a
/// state's RAM cannot divert the calc it runs.
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
/// The silent history's write index (`_DAT_801CE204`).
const SILENT_RING_INDEX: u32 = 0x801C_E204;
/// The key-on / key-off accumulators and the sounding mask.
const MASK_WORDS: [u32; 3] = [0x801C_DB48, 0x801C_DB4C, 0x801C_E248];
const VOICE_FLAGS: u32 = 0x801C_E060;
/// libsnd's mono fold (`_DAT_801CE330`): the option a state was captured under.
const MONO_FOLD: u32 = 0x801C_E330;
const SHADOW_REGS: u32 = 0x801C_E080;
/// Per open bank, the program table (`0x801CE268 + vab * 4`, read by `FUN_80068B98`).
const VAB_PROGRAM_TABLES: u32 = 0x801C_E268;

/// One sounding voice as the SPU holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Sounding {
    pub start: u32,
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
            start: v.start_addr,
            pitch: v.pitch,
            vol: (v.vol_left, v.vol_right),
            adsr: v.adsr_cfg.raw,
            level: v.adsr.level,
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
    slot: u32,
    /// A VAG's SPU word address as the open bank holds it -> where the port's
    /// bank put that sample, in this SPU's RAM.
    vag_addr: BTreeMap<u16, u32>,
}

impl RetailSide {
    /// Stand the state's sequence on `slot` at the top of its score, on an idle
    /// voice file, with `seq_vol` for the sequence volume.
    pub fn new(ram: &[u8], port: &PortSide, slot: u8, seq_vol: u8) -> Result<Self> {
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
        let mut spu = Spu::new();
        spu.set_retail_reverb();
        spu.ram = port.d.spu().ram.clone();
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
        // `FUN_80065978` reads a VAG's address from the bank's program table:
        // two to a sixteen-byte row, at `+0xC` for an odd index, `+0xE` for even.
        let table = cpu.mem.rd32(VAB_PROGRAM_TABLES + vab * 4);
        let vag_addr = port
            .bank_samples()
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let vag = i as u32 + 1;
                let row = table + (vag - 1) / 2 * 16 + if vag & 1 == 0 { 0xE } else { 0xC };
                Some((cpu.mem.rd16(row), (*s)?))
            })
            .collect();
        Ok(Self {
            cpu,
            spu,
            carry: 0.0,
            slot: u32::from(slot),
            vag_addr,
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
                voice.start_addr = self
                    .vag_addr
                    .get(&r[3])
                    .copied()
                    .unwrap_or(u32::from(r[3]) * 8);
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
            mem.wr16(MIRROR + i as u32 * 16 + 0xC, level);
        }
        let mut view = view_of(&self.spu, &before);
        view.released = released;
        Ok(view)
    }
}

/// A cue as both sides key it.
#[derive(Debug, Clone, Copy)]
pub struct CueKey {
    pub voice: u8,
    pub program: u8,
    pub tone: u8,
    pub note: u8,
    pub vol: i16,
    /// Stop these voices from `7` up first: a new sustained run.
    pub release_from_7: u8,
}

impl RetailSide {
    /// The two calls `FUN_80016B6C` makes per voice: `FUN_800653C8` and then
    /// `FUN_80065034`, whose last four arguments are on the stack.
    pub fn key_cue(&mut self, c: &CueKey) -> Result<()> {
        const STOP_VOICE: u32 = 0x8006_53C8;
        const CUE_KEY: u32 = 0x8006_5034;
        const STACK: u32 = 0x801F_FF00;
        for v in 0..c.release_from_7 {
            expect_return(self.cpu.call(STOP_VOICE, &[7 + u32::from(v)]))?;
        }
        if c.release_from_7 == 0 {
            expect_return(self.cpu.call(STOP_VOICE, &[u32::from(c.voice)]))?;
        }
        let mem = &mut self.cpu.mem;
        mem.wr32(STACK + 0x10, u32::from(c.note));
        mem.wr32(STACK + 0x14, 0x40);
        mem.wr32(STACK + 0x18, c.vol as u32);
        mem.wr32(STACK + 0x1C, c.vol as u32);
        let rec = mem.rd32(SEQ_TABLE + self.slot * 4);
        let vab = u32::from(mem.rd8(rec + 0x26));
        expect_return(self.cpu.call(
            CUE_KEY,
            &[
                u32::from(c.voice),
                vab,
                u32::from(c.program),
                u32::from(c.tone),
            ],
        ))
    }
}

/// A tone the port's bank can sound, on the drainer's voices.
fn pick_cue(port: &PortSide, r: u32, sustained: &mut bool) -> Option<CueKey> {
    let bank = port.d.bank()?;
    let keyed: Vec<(usize, usize)> = bank
        .programs
        .iter()
        .enumerate()
        .flat_map(|(p, pr)| (0..pr.tones.len()).map(move |t| (p, t)))
        .filter(|&(p, t)| bank.can_play_tone(p, t))
        .collect();
    let &(program, tone) = keyed.get((r >> 8) as usize % keyed.len().max(1))?;
    let long = r & 0x70 == 0;
    let voice = if long { 7 } else { 20 + (r & 3) as u8 };
    let release_from_7 = u8::from(long && *sustained);
    *sustained |= long;
    Some(CueKey {
        voice,
        program: program as u8,
        tone: tone as u8,
        note: 48 + ((r >> 16) & 0x1F) as u8,
        vol: 0x7F,
        release_from_7,
    })
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

    /// The flush's silent-history index as the state's libsnd holds it: both
    /// sides run their flushes from there.
    fn seat_silent_ring_phase(&mut self, retail: &mut RetailSide) {
        let at = retail.cpu.mem.rd32(SILENT_RING_INDEX) as usize;
        if let Some(s) = self.d.sequencer_mut() {
            s.set_silent_ring_phase(at);
        }
    }

    fn bank_samples(&self) -> Vec<Option<u32>> {
        self.d
            .bank_samples()
            .iter()
            .map(|s| s.as_ref().map(|u| u.addr))
            .collect()
    }

    /// One vsync. A cue keys after the flush and ahead of the samples: retail's
    /// main loop runs it between two calcs, and its KON goes out with the flush
    /// the port has just run.
    pub fn step_with_cue(&mut self, samples: f64, cue: Option<CueKey>) -> VsyncView {
        let before = stamps(self.d.spu());
        let spu: *mut Spu = self.d.spu_mut();
        if let Some(s) = self.d.sequencer_mut() {
            // SAFETY: the director's SPU and sequencer are disjoint fields.
            s.step_vsync(unsafe { &mut *spu });
        }
        if let (Some(c), Some(bank)) = (cue, self.d.bank().cloned()) {
            let spu = self.d.spu_mut();
            for v in 0..c.release_from_7 {
                spu.voices[7 + usize::from(v)].key_off();
            }
            bank.play_tone_pair(
                spu,
                usize::from(c.voice),
                usize::from(c.program),
                usize::from(c.tone),
                c.note,
                c.vol,
                c.vol,
            );
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

fn note(s: &Sounding) -> (u32, u16, (i16, i16), (u16, u16), bool) {
    (s.start, s.pitch, s.vol, s.adsr, s.reverb)
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
        if out
            .first
            .iter()
            .filter(|f| f.split_whitespace().nth(1) == Some(&kind))
            .count()
            < keep
        {
            out.first.push(s);
        }
    };
    for (i, r) in retail.iter().enumerate() {
        let Some(p) = usize::try_from(i as i64 - lag)
            .ok()
            .and_then(|j| port.get(j))
        else {
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
                format!(
                    "v{i} voice: retail {:?} port {:?}",
                    ids(&r.keyed),
                    ids(&p.keyed)
                ),
            );
        }
        if r.released.len() != p.released.len() {
            out.release_diffs += 1;
            note_it(
                &mut out,
                format!(
                    "v{i} release: retail {:?} port {:?}",
                    r.released, p.released
                ),
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
                let mut n: Vec<_> = v
                    .iter()
                    .map(|(_, s)| (note(s), s.level, s.releasing))
                    .collect();
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
    /// Key cues beside the score on both sides, a seeded run of them: one-shots on
    /// the drainer's voices `20..=23`, sustained ones from voice `7`.
    #[arg(long)]
    pub cues: bool,
    /// Print retail's voice records and both sides' voices at this vsync.
    #[arg(long)]
    pub dump_at: Option<usize>,
}

/// The differential of the mednafen or PCSX-Redux state `label`, `None` when the
/// library does not hold it or it has no sequence open.
pub fn run_label(label: &str, vsyncs: usize, cues: bool) -> Result<Option<Differential>> {
    let (Some(m), Some(l), Some(e)) = crate::retail_compare::resolve_dirs() else {
        return Ok(None);
    };
    let manifest = ScenarioManifest::from_path(&m)?;
    let host = SceneHost::open_extracted(&e)?;
    let bank = TrackBank::load(&host);
    let Some(c) = enumerate_corpus(&manifest, &l)
        .into_iter()
        .find(|c| c.label == label)
    else {
        return Ok(None);
    };
    let ram = state_ram(c.emulator, &c.path)?;
    let audio = RetailAudio::from_ram(&ram, &bank, None);
    let Some(music) = audio.music_slot() else {
        return Ok(None);
    };
    let Some(track) = music.track else {
        return Ok(None);
    };
    let Some(entry) = host.music_bank_entry_bytes(track)? else {
        return Ok(None);
    };
    let args = SeqDifferentialArgs {
        filter: None,
        vsyncs,
        keep: 4,
        all_states: false,
        cues,
        dump_at: None,
    };
    run_one(&ram, music.slot, track, music.vol.0 as u8, &entry, &args).map(Some)
}

pub fn run(args: SeqDifferentialArgs) -> Result<()> {
    let (m, l, e) = crate::retail_compare::resolve_dirs();
    let manifest = ScenarioManifest::from_path(&m.context("no scripts/scenarios.toml")?)?;
    let library = l.context("no save library")?;
    let extracted = e.context("no extracted disc")?;
    let host = SceneHost::open_extracted(&extracted)?;
    let bank = TrackBank::load(&host);
    let mut seen: BTreeMap<u16, String> = BTreeMap::new();
    let mut corpus: Vec<(String, &'static str, PathBuf)> = enumerate_corpus(&manifest, &library)
        .into_iter()
        .filter(|c| c.resident_patch.is_none() && !excluded(&c.label))
        .map(|c| (c.label, c.emulator, c.path))
        .collect();
    corpus.sort();
    for (label, emulator, path) in corpus {
        if args
            .filter
            .as_deref()
            .is_some_and(|f| !f.split(',').any(|p| label.contains(p)))
        {
            continue;
        }
        let Ok(ram) = state_ram(emulator, &path) else {
            continue;
        };
        let audio = RetailAudio::from_ram(&ram, &bank, None);
        let Some(music) = audio.music_slot() else {
            continue;
        };
        let (slot, vol) = (music.slot, music.vol.0 as u8);
        // Two ids can carry one score's opening and one bank: the track is the
        // one the state's RAM holds the most of.
        let Some((_, track, entry)) = music
            .bank_tracks
            .iter()
            .chain(&music.score_tracks)
            .filter_map(|&id| {
                let entry = host.music_bank_entry_bytes(id).ok()??;
                Some((score_in_ram(&ram, slot, &entry), id, entry))
            })
            .max_by_key(|c| c.0)
        else {
            continue;
        };
        if !args.all_states && seen.contains_key(&track) {
            continue;
        }
        seen.insert(track, label.clone());
        match run_one(&ram, slot, track, vol, &entry, &args) {
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

/// How many bytes of `entry`'s score the sequence on `slot` holds from its header.
fn score_in_ram(ram: &[u8], slot: u8, entry: &[u8]) -> usize {
    let at = |va: u32| (va & 0x1F_FFFF) as usize;
    let rd32 = |va: u32| {
        ram.get(at(va)..at(va) + 4)
            .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let Some(split) = legaia_engine_system::chunk_install::owned_bank_offsets(entry) else {
        return 0;
    };
    let score = &entry[split.seq..];
    let start = at(rd32(rd32(SEQ_TABLE + u32::from(slot) * 4) + 4));
    let Some(head) = (1..64usize).find(|b| ram.get(start - b..start - b + 4) == Some(b"pQES"))
    else {
        return 0;
    };
    let Some(there) = score.windows(4).position(|w| w == b"pQES") else {
        return 0;
    };
    ram[start - head..]
        .iter()
        .zip(&score[there..])
        .take_while(|(a, b)| a == b)
        .count()
}

/// A state made on a patched disc replays that disc's code.
fn excluded(label: &str) -> bool {
    ["delilas_", "shiny_refactor_", "minigame_dance_pcsx"]
        .iter()
        .any(|p| label.contains(p))
}

fn state_ram(emulator: &str, path: &std::path::Path) -> Result<Vec<u8>> {
    Ok(if emulator == "mednafen" {
        psx_mednafen::SaveState::from_path(path)?
            .main_ram()?
            .to_vec()
    } else {
        psx_pcsx_redux::SaveState::from_path(path)?
            .main_ram()
            .to_vec()
    })
}

fn run_one(
    ram: &[u8],
    slot: u8,
    track: u16,
    vol: u8,
    entry: &[u8],
    args: &SeqDifferentialArgs,
) -> Result<Differential> {
    let (vsyncs, keep) = (args.vsyncs, args.keep);
    let mut port = PortSide::new(track, entry, vol);
    let mut retail = RetailSide::new(ram, &port, slot, vol)?;
    port.seat_silent_ring_phase(&mut retail);
    let mut rv = Vec::with_capacity(vsyncs);
    let mut pv = Vec::with_capacity(vsyncs);
    let mut rng = 0x5EED_u32 ^ u32::from(track);
    let mut next_cue = 30usize;
    let mut sustained = false;
    // A cue keyed ahead of retail's calc `i` goes out in that calc's flush, which
    // is the port's flush of step `i - 1`.
    let mut retail_cue = None;
    for i in 0..vsyncs {
        if let Some(c) = retail_cue.take() {
            retail.key_cue(&c)?;
        }
        rv.push(retail.step(RETAIL_VSYNC_SAMPLES)?);
        let cue = if args.cues && i + 1 == next_cue {
            rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            next_cue += 8 + (rng >> 26) as usize;
            pick_cue(&port, rng, &mut sustained)
        } else {
            None
        };
        retail_cue = cue;
        pv.push(port.step_with_cue(RETAIL_VSYNC_SAMPLES, cue));
        if args.dump_at == Some(i) {
            let m = &mut retail.cpu.mem;
            let rec = m.rd32(SEQ_TABLE + u32::from(slot) * 4);
            println!(
                "pans {:02x?}\nprogs {:02x?}\nvols {:x?}",
                m.read(rec + 0x27, 16),
                m.read(rec + 0x37, 16),
                (0..16)
                    .map(|k| m.rd16(rec + 0x60 + k * 2))
                    .collect::<Vec<_>>()
            );
            for v in 0..NUM_VOICES as u32 {
                let b = m.read(VOICE_RECORDS + v * VOICE_STRIDE, 0x20);
                println!("record {v} {b:02x?}");
            }
            println!("retail {:x?}", rv[i].sounding);
            if i > 0 {
                println!("port {:x?}", pv[i - 1].sounding);
            }
        }
    }
    Ok(compare(&rv, &pv, keep))
}
