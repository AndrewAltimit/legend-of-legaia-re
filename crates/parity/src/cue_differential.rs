//! Every cue keyed both ways in one library state: retail's drainer run off the
//! state's RAM, and the port's director seeded into the state.
//!
//! `FUN_80016B6C` is run in `crates/retail-exec` with one ring slot armed, up to its
//! first call past the ring walk. What it staged is read from libsnd's shadow
//! registers: each keyed voice's level pair, pitch and ADSR, which between them name
//! the tone. The engine's side keys the same ring id through its audio director
//! (`AudioDirector::fire_ring_now`) and reads the SPU. A row that differs is a cue that
//! sounds different: the wrong bank open, the wrong tone, the wrong level. See
//! `docs/subsystems/audio.md`, "The cue differential".

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use legaia_engine_audio::AudioSink;
use legaia_engine_session::session::BootSession;
use legaia_ram_image::ScenarioManifest;
use legaia_retail_exec::{Cpu, GP, RA, RETURN_SENTINEL, Stop};

use crate::audio_sweep::{EngineAudio, excluded_state};
use crate::retail_compare::{StateClass, enumerate_corpus, read_retail, run_engine_listening_with};

const DRAINER: u32 = 0x8001_6B6C;
/// The drainer's first call past the ring walk: here the run stops.
const PAST_THE_RING: u32 = 0x8001_822C;
const ACTOR_TABLE: u32 = 0x801C_9370;
const RING_IDS: u32 = 0x8007_B6D8;
const RING_TIMERS: u32 = 0x8007_C338;
/// The stream gate the walk sits behind: `0x80016BC0..0x80016BD4`.
const STREAM_A: u32 = 0x8007_BABC;
const STREAM_B: u32 = 0x8007_BAA0;
const GP_VALUE: u32 = 0x8007_B318;
const GP_STREAM_BUSY: u32 = 0x550;
const GP_ONE_SHOT: u32 = 0x4BC;
const GP_HELD: u32 = 0x5D0;
const GP_DEBUG: u32 = 0x3B8;
const GP_DEBUG_DRAW: u32 = 0x3B4;
const GP_RUNTIME_BANK: u32 = 0x5B8;
const KEY_ON_LO: u32 = 0x801C_DB48;
const SHADOW_REGS: u32 = 0x801C_E080;
const SEQ_LOCK: u32 = 0x801C_D2B4;
const STATIC_CUES: u16 = 0x64;
const RUNTIME_BASE: u16 = 0x200;
const RUNTIME_ROWS: u16 = 0x40;

/// One voice a cue keyed: levels, pitch, ADSR.
pub type CueVoice = ((i16, i16), u16, (u16, u16));

/// The engine side's ids to key, and what each did.
#[derive(Debug, Clone, Default)]
pub struct CueProbe {
    pub ids: Vec<u16>,
    pub keyed: BTreeMap<u16, Vec<CueVoice>>,
}

/// Key the probe's ids through the settled session's director.
pub fn probe<S: AudioSink>(session: &mut BootSession<S>, audio: &mut EngineAudio) {
    let Some(mut p) = audio.cue_probe.take() else {
        return;
    };
    if let Some(d) = session.bgm.as_mut() {
        for &id in &p.ids {
            let before: Vec<u32> = d
                .audio()
                .with_spu(|s| s.voices.iter().map(|v| v.key_on_count()).collect());
            d.fire_ring_now(id as i16);
            let keyed = d.audio().with_spu(|s| {
                let mut out: Vec<CueVoice> = s
                    .voices
                    .iter()
                    .zip(&before)
                    .filter(|(v, b)| v.key_on_count() != **b)
                    .map(|(v, _)| ((v.vol_left, v.vol_right), v.pitch, v.adsr_cfg.raw))
                    .collect();
                out.sort();
                out
            });
            p.keyed.insert(id, keyed);
        }
    }
    audio.cue_probe = Some(p);
}

/// Retail's drainer over a state's RAM.
pub struct RetailDrainer {
    cpu: Cpu,
    snapshot: legaia_retail_exec::Memory,
}

impl RetailDrainer {
    pub fn new(ram: &[u8]) -> Self {
        let mut cpu = Cpu::new();
        cpu.mem.load(0x8000_0000, ram);
        cpu.regs.r[GP] = GP_VALUE;
        cpu.step_budget = 2_000_000;
        cpu.hook(PAST_THE_RING, |r, _| r.r[RA] = RETURN_SENTINEL);
        let m = &mut cpu.mem;
        let a = m.rd32(STREAM_A);
        m.wr32(STREAM_B, a);
        m.wr32(SEQ_LOCK, 0);
        for off in [
            GP_STREAM_BUSY,
            GP_ONE_SHOT,
            GP_HELD,
            GP_DEBUG,
            GP_DEBUG_DRAW,
        ] {
            m.wr32(GP_VALUE + off, 0);
        }
        let snapshot = cpu.mem.clone();
        Self { cpu, snapshot }
    }

    /// What ring id `id` stages from a clean slate.
    pub fn key(&mut self, id: u16) -> Result<Vec<CueVoice>> {
        self.cpu.mem = self.snapshot.clone();
        let m = &mut self.cpu.mem;
        for i in 0..4 {
            m.wr16(RING_IDS + i * 2, 0xFFFF);
            m.wr32(RING_TIMERS + i * 4, 1);
        }
        m.wr16(RING_IDS, id);
        m.wr32(RING_TIMERS, 0);
        m.wr32(KEY_ON_LO, 0);
        match self.cpu.call(DRAINER, &[]) {
            Stop::Returned => {}
            other => anyhow::bail!("drainer stopped on {id:#x}: {other:x?}"),
        }
        let m = &mut self.cpu.mem;
        let mask = m.rd32(KEY_ON_LO) & 0xFF_FFFF;
        let mut out: Vec<CueVoice> = (0..24u32)
            .filter(|v| mask & (1 << v) != 0)
            .map(|v| {
                let r = SHADOW_REGS + v * 16;
                (
                    (m.rd16(r) as i16, m.rd16(r + 2) as i16),
                    // The chip's own cap on the step, which the port applies
                    // at the register.
                    m.rd16(r + 4).min(0x4000),
                    (m.rd16(r + 8), m.rd16(r + 0xA)),
                )
            })
            .collect();
        out.sort();
        Ok(out)
    }

    /// Whether the state has a runtime descriptor bank at all.
    pub fn has_runtime_bank(&mut self) -> bool {
        self.cpu.mem.rd32(GP_VALUE + GP_RUNTIME_BANK) != 0
    }
}

#[derive(Debug, clap::Args)]
pub struct CueDifferentialArgs {
    /// Only labels containing this (comma = any of several). Without it, the
    /// first state of each scene.
    #[arg(long)]
    pub filter: Option<String>,
    /// Lines to print per state.
    #[arg(long, default_value_t = 8)]
    pub keep: usize,
    /// Every matching state, not one per scene.
    #[arg(long)]
    pub all_states: bool,
    /// Battle states, one per formation: the class-2 bank, the formation's two
    /// `monster.snd` banks and `bse.dat` with the categories the state's RAM holds.
    #[arg(long)]
    pub battle: bool,
}

pub fn run(args: CueDifferentialArgs) -> Result<()> {
    let (m, l, e) = crate::retail_compare::resolve_dirs();
    let manifest = ScenarioManifest::from_path(&m.context("no scripts/scenarios.toml")?)?;
    let library = l.context("no save library")?;
    let extracted = e.context("no extracted disc")?;
    let scus = std::fs::read(extracted.join("SCUS_942.54")).context("read SCUS_942.54")?;
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT"))?;
    let mut scenes = BTreeSet::new();
    let (mut total, mut same) = (0usize, 0usize);
    let mut bad_categories = 0usize;
    let mut entries = enumerate_corpus(&manifest, &library);
    entries.sort_by(|a, b| a.label.cmp(&b.label));
    for entry in entries {
        if excluded_state(&entry)
            || args
                .filter
                .as_deref()
                .is_some_and(|f| !f.split(',').any(|p| entry.label.contains(p)))
        {
            continue;
        }
        let Ok(mut obs) = read_retail(&entry, &scus) else {
            continue;
        };
        obs.settle_on_loaded_scene(&cdname);
        let battle = obs.class == StateClass::Battle;
        if args.battle != battle
            || !(battle || matches!(obs.class, StateClass::Field | StateClass::WorldMap))
        {
            continue;
        }
        let ram = if entry.emulator == "mednafen" {
            legaia_mednafen::SaveState::from_path(&entry.path)?
                .main_ram()?
                .to_vec()
        } else {
            let (st, _) = legaia_pcsx_redux::gpu::load_with_scus(&entry.path, &scus)?;
            st.main_ram().to_vec()
        };
        let formation: Vec<u8> = ram[0x7B_D0C..0x7B_D10].to_vec();
        let scene_key = if battle {
            format!("{formation:02x?}")
        } else {
            obs.scene.clone()
        };
        if !args.all_states && !scenes.insert(scene_key) {
            continue;
        }
        if battle {
            categories(&ram, &formation, &entry.label, &mut bad_categories);
        }
        let mut retail = RetailDrainer::new(&ram);
        let mut ids: Vec<u16> = (0..STATIC_CUES).collect();
        if retail.has_runtime_bank() {
            ids.extend(RUNTIME_BASE..RUNTIME_BASE + RUNTIME_ROWS);
        }
        let probe = CueProbe {
            ids: ids.clone(),
            ..Default::default()
        };
        // The listening run hands the probe back filled.
        let heard = if battle {
            battle_probe(&extracted, &ram, &formation, probe)
        } else {
            run_engine_listening_with(&extracted, &obs, Some(probe))
                .map(|(_, heard)| heard.cue_probe.map(|p| p.keyed).unwrap_or_default())
        };
        let keyed = match heard {
            Ok(k) => k,
            Err(err) => {
                println!("{} ({}): [seed failed] {err:#}", entry.label, obs.scene);
                continue;
            }
        };
        let mut lines = Vec::new();
        let (mut row_total, mut row_same) = (0, 0);
        for id in ids {
            let audible = |v: &Vec<CueVoice>| {
                v.iter()
                    .filter(|c| c.0 != (0, 0))
                    .copied()
                    .collect::<Vec<_>>()
            };
            let r = audible(&retail.key(id)?);
            let p = audible(&keyed.get(&id).cloned().unwrap_or_default());
            // Past the end of a runtime bank retail keys whatever follows.
            if id >= RUNTIME_BASE && p.is_empty() {
                continue;
            }
            row_total += 1;
            if r == p {
                row_same += 1;
            } else {
                lines.push(format!("    {id:#05x}: retail {r:x?} port {p:x?}"));
            }
        }
        total += row_total;
        same += row_same;
        println!(
            "{} ({}): {row_same}/{row_total} cues alike{}",
            entry.label,
            obs.scene,
            match ram[0x7B_A88] {
                0 => String::new(),
                _ => " [forced channel latch up]".to_string(),
            }
        );
        for l in lines.iter().take(args.keep) {
            println!("{l}");
        }
    }
    println!("cue-differential: {same}/{total}");
    if args.battle {
        println!("loader category disagreements: {bad_categories}");
    }
    Ok(())
}

/// Each seated monster's category byte at render node `+0x80` against the port's
/// rule over the formation cells, and each dispatcher write it could have left in
/// the `bse.dat` rows: retail's `+4` column holds an authored default or a value the
/// router wrote, so anything else names a writer the port does not have.
fn categories(ram: &[u8], formation: &[u8], label: &str, bad: &mut usize) {
    let at = |va: u32| (va & 0x1F_FFFF) as usize;
    let rd32 = |va: u32| {
        ram.get(at(va)..at(va) + 4)
            .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let ids: Vec<u16> = formation
        .iter()
        .filter(|&&b| b != 0)
        .map(|&b| u16::from(b))
        .collect();
    let mut seen = Vec::new();
    for (i, &cell) in formation.iter().enumerate() {
        let actor = rd32(ACTOR_TABLE + (3 + i as u32) * 4);
        let node = if actor >= 0x8000_0000 {
            rd32(actor + 0x22C)
        } else {
            0
        };
        if cell == 0 || node < 0x8000_0000 {
            continue;
        }
        let byte = ram[at(node + 0x80)];
        let want = legaia_engine_core::world::monster_sound_category(&ids, u16::from(cell));
        seen.push((cell, byte));
        if byte != want {
            *bad += 1;
            println!("  category {label}: seat {i} id {cell:#x} retail {byte} port {want}");
        }
    }
    println!("  categories {label} {formation:02x?}: {seen:02x?}");
}

/// The engine's banks for a battle, staged directly: retail's `+4` writes to the
/// `bse.dat` rows come with the state's RAM.
fn battle_probe(
    extracted: &std::path::Path,
    ram: &[u8],
    formation: &[u8],
    mut probe: CueProbe,
) -> Result<BTreeMap<u16, Vec<CueVoice>>> {
    use legaia_engine_mode::sfx_residency::SharedRegionBank;
    let cfg = legaia_engine_session::session::BootConfig {
        scene: "town01".into(),
        enable_audio: true,
    };
    let mut session = legaia_engine_session::native::NativeSession::open_extracted_detached_audio(
        extracted, &cfg,
    )
    .context("open boot session")?;
    let index = session.host.index.clone();
    let at = |va: u32| (va & 0x1F_FFFF) as usize;
    let rd32 = |va: u32| u32::from_le_bytes(ram[at(va)..at(va) + 4].try_into().unwrap_or([0; 4]));
    let bundle = rd32(GP_VALUE + GP_RUNTIME_BANK);
    let rows = ram
        .get(at(bundle)..at(bundle) + 0x1000)
        .context("no runtime bank")?;
    let want = legaia_engine_core::world::monster_sound_banks(
        formation.iter().map(|&b| u16::from(b)).filter(|&b| b != 0),
    );
    let d = session.bgm.as_mut().context("no director")?;
    d.sync_shared_region(Some(SharedRegionBank::CLASS2), |e| {
        index.entry_bytes_extended(e).ok()
    });
    d.sync_field_sfx(rows, false, None, |_| None);
    d.sync_battle_monster_banks(&want, || {
        index
            .entry_bytes(legaia_container_formats::vab_multi_bank::MONSTER_SND_PROT_INDEX as u32)
            .ok()
    });
    let mut audio = EngineAudio::default();
    probe.keyed.clear();
    audio.cue_probe = Some(probe);
    self::probe(&mut session, &mut audio);
    Ok(audio.cue_probe.map(|p| p.keyed).unwrap_or_default())
}
