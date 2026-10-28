//! Audio state sweep: what the music and the sound effects are doing in each
//! retail library state, against the engine seeded into the same state.
//!
//! The retail comparison corpus scores the track-select word `0x8007BAC8`
//! ([`crate::retail_compare`]'s `bgm` channel). That word is what the scripts
//! *asked for*; this sweep reads what libsnd is *playing*: every open
//! sequence slot (`_DAT_801CD2B8` bitmap, record pointers at `0x801CD2C0`),
//! which `music_01` track each one holds (the score bytes at the record's
//! `+0x04` and the VAB header its `+0x26` bank id installed), whether it is
//! playing (`+0x98` bit `0x1`), its sequence volume (`+0x58`) and its tempo
//! (`+0x94`, the BPM `FUN_80061954` / `FUN_80062410` store), plus the cue
//! voices sounding beside it (libsnd voice records at `0x801CDB50`, owner
//! `0x21`, live envelope `+0x06`). Everything is read off main RAM, so both
//! emulators' states take part.
//!
//! The engine side is seeded the way the corpus seeds a walkable state - the
//! card-load path over the state's live game-state window, the player seated
//! on the state's `(X, Z)` - but over a device-free audio output
//! ([`legaia_engine_audio::TestAudioSink`]) with the director attached, and
//! each settle tick runs the play hosts' audio frame
//! ([`legaia_engine_session::BootSession::step_audio_frame`]) and pulls one
//! video frame of samples. The sweep then reads the director: the attached
//! track, the pause gate, the sequencer's volume and tempo, and the SPU's
//! sounding voices by bus.
//!
//! Channels, each `1.0` / `0.0` per state:
//!
//! - `track` - the track retail's field slot (else any open slot) holds is
//!   the one the engine has attached;
//! - `playing` - retail's slot is playing exactly when the engine's gate is
//!   open with a sequencer attached;
//! - `seq_vol` - the record's `+0x58` against the engine sequencer's volume
//!   (both playing the same track);
//! - `tempo` - the record's `+0x94` is one of the tempos the track's score
//!   writes (rounded as `FUN_80061954` rounds), which a tempo slide would
//!   break;
//! - `ambient` - where retail has a cue voice sounding, the engine sounds at
//!   least one SFX-bus voice over the settle window's second half.
//!
//! `docs/subsystems/audio.md` ("The audio state sweep") records what it
//! measures; `scripts/ci/audio-sweep.py` drives it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use legaia_engine_audio::spu::VoiceBus;
use legaia_engine_audio::{AudioSink, SPU_INTERNAL_RATE};
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_session::BootSession;
use legaia_ram_image::ScenarioManifest;
use serde::{Deserialize, Serialize};

use crate::retail_compare::{CorpusEntry, StateClass, enumerate_corpus};

/// libsnd's open-sequence bitmap (`_DAT_801CD2B8`, 16 slots).
const SEQ_BITMAP: u32 = 0x801C_D2B8;
/// libsnd's per-slot record pointer table (`_DAT_801CD2C0[16]`).
const SEQ_TABLE: u32 = 0x801C_D2C0;
/// libsnd's per-voice driver records (24 x `0x36`).
const VOICE_RECORDS: u32 = 0x801C_DB50;
const VOICE_STRIDE: u32 = 0x36;
/// The VAB header pointer table (`0x80091508 + slot * 12`).
const VAB_HEADERS: u32 = 0x8009_1508;
/// The note owner key of the cue slot.
const CUE_OWNER: u16 = 0x21;
/// The sound-source records (`0x8007051C + slot * 0x10`): `+0x0` the score,
/// `+0x8` open, `+0xA` the libsnd sequence id `SsSeqOpen` returned, `+0xC`
/// the VAB id (`FUN_80026410`, `0x8002643C..0x80026464`).
const SOUND_SOURCES: u32 = 0x8007_051C;
/// The field BGM's sound source (`0x8007052C`).
const FIELD_BGM_SOURCE: u8 = 1;
/// The ending theme's sound source (`0x800705BC`, `FUN_801D6704`).
const ENDING_SOURCE: u8 = 10;
/// The `music_01` id range searched for a state's track.
const TRACK_IDS: std::ops::RangeInclusive<u16> = 2000..=2120;
/// The settle window, as the retail comparison corpus settles a seed.
const SETTLE_TICKS: u64 = legaia_engine_capture::SETTLE_TICKS;

/// Every channel the sweep scores, in report order.
pub const AUDIO_CHANNELS: &[&str] = &[
    "track",
    "playing",
    "seq_vol",
    "tempo",
    "ambient",
    "battle_track",
];
/// The battle track's sound source (`0x8007056C`, the record the poller's
/// commit closes at `0x8002474C`).
const BATTLE_SOURCE: u8 = 5;
/// The first monster of the formation `0x8007BD0C`: the battle intro zeroes
/// the sound set after a fight that opens on [`GAZA_FORMATION_HEAD`], so a
/// capture inside it no longer holds the set the intro read.
const FORMATION: u32 = 0x8007_BD0C;
const GAZA_FORMATION_HEAD: u8 = 0xA6;

/// One open libsnd sequence slot in a retail state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetailSeqSlot {
    pub slot: u8,
    /// The open sound-source record whose `+0xA` names this sequence.
    pub source: Option<u8>,
    /// The `music_01` track whose score and bank the slot holds.
    pub track: Option<u16>,
    /// Every track whose entry carries the slot's score.
    pub score_tracks: Vec<u16>,
    /// Those of [`Self::score_tracks`] whose VAB header is the installed
    /// one: every id here plays the same bytes.
    pub bank_tracks: Vec<u16>,
    /// The record's `+0x98` flag word.
    pub flags: u16,
    /// `+0x98` bit `0x1`.
    pub playing: bool,
    /// `+0x58` / `+0x5A`.
    pub vol: (u16, u16),
    /// `+0x94`, BPM.
    pub tempo_bpm: u32,
    /// `+0x26`, the VAB slot the sequence keys.
    pub vab_id: u8,
}

/// What libsnd is doing in one retail state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RetailAudio {
    pub track_word: u16,
    /// `_DAT_8007BAB8` / `_DAT_8007BA9C`: the requested and the loaded bank
    /// index; they differ while a track swap is in flight (`FUN_800243F0`).
    pub swap: (u16, u16),
    /// The sound flag word `_DAT_8007B750` (bit 0 script-owned swap pending,
    /// bit 1 paused, bit 3 load settled).
    pub sound_flags: u16,
    /// The battle sound set `_DAT_8007B880` (op-`0x35` sub-op `7`).
    pub battle_sound_set: i32,
    /// `DAT_8007B64B`, which moves a set-`0` fight onto bundle `0x370`.
    pub battle_alt: u8,
    /// The formation's first monster id (`0x8007BD0C`).
    pub formation_head: u8,
    pub seqs: Vec<RetailSeqSlot>,
    /// Sounding cue voices (owner `0x21`, live envelope): `(voice, vab slot)`.
    pub cue_voices: Vec<(u8, u16)>,
    /// Sounding sequence-owned voices.
    pub seq_voices: u32,
}

/// What the engine's director is doing after the seed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EngineAudio {
    pub track_word: Option<u16>,
    /// The attached track (`AudioDirector::last_started` while attached).
    pub track: Option<u16>,
    pub playing: bool,
    pub seq_vol: Option<u8>,
    pub tempo_bpm: Option<f32>,
    /// Most SFX-bus voices sounding on one frame of the window's second half.
    pub sfx_voices_peak: u32,
    /// Most BGM-bus voices sounding on one frame of the same span.
    pub bgm_voices_peak: u32,
    /// Every cue id that keyed a voice during the settle window.
    pub cues_fired: BTreeSet<u16>,
    /// `(sfx, bgm)` sounding-voice counts of the last [`LISTEN_SPAN`] ticks.
    #[serde(skip)]
    recent: Vec<(u32, u32)>,
}

/// One state of the sweep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSweepRow {
    pub label: String,
    pub emulator: String,
    pub scene: String,
    pub class: String,
    pub retail: RetailAudio,
    pub engine: Option<EngineAudio>,
    /// Why the engine was not seeded (non-walkable class, unreadable state).
    pub unseeded: Option<String>,
    pub channels: BTreeMap<String, f64>,
    pub detail: BTreeMap<String, String>,
}

/// The states the sweep never reads: made on a patched disc, or named by the
/// project's retail-only evidence rule (their executables are not retail's).
pub fn excluded_state(e: &CorpusEntry) -> bool {
    let l = e.label.as_str();
    e.resident_patch.is_some()
        || e.resident_disc.is_some()
        || l.starts_with("delilas_")
        || l.starts_with("shiny_refactor_")
        || l.contains("minigame_dance_pcsx")
        || l.contains("dolk2")
        || l.contains("worldmap")
        || l.contains("baka_entry")
        || (e.emulator != "mednafen"
            && ((l.contains("gaza2") && !l.contains("gaza2_card"))
                || (l.contains("cort_evolved") && !l.contains("cort_evolved_card"))))
}

/// Every `music_01` track's entry bytes, read once.
pub struct TrackBank {
    tracks: Vec<(u16, Arc<Vec<u8>>)>,
}

impl TrackBank {
    pub fn load(host: &SceneHost) -> Self {
        Self {
            tracks: TRACK_IDS
                .filter_map(|id| Some((id, host.music_bank_entry_bytes(id).ok()??)))
                .collect(),
        }
    }

    fn entry(&self, id: u16) -> Option<&[u8]> {
        self.tracks
            .iter()
            .find(|(t, _)| *t == id)
            .map(|(_, b)| b.as_slice())
    }

    /// Every track whose entry carries `score`, and the ones among them
    /// whose VAB header also matches `vab_size` (two tracks can share one
    /// score; two that share both play the same thing).
    fn identify(&self, score: &[u8], vab_size: Option<&[u8]>) -> (Vec<u16>, Vec<u16>) {
        let all: Vec<_> = self
            .tracks
            .iter()
            .filter(|(_, b)| score.len() == 24 && b.windows(24).any(|w| w == score))
            .collect();
        let bank = all
            .iter()
            .filter(|(_, b)| {
                vab_size.is_some()
                    && b.windows(4)
                        .position(|w| w == b"pBAV")
                        .and_then(|p| b.get(p + 12..p + 0x20))
                        == vab_size
            })
            .map(|(id, _)| *id)
            .collect();
        (all.iter().map(|(id, _)| *id).collect(), bank)
    }

    /// The BPMs the track's score writes - its header tempo and every
    /// tempo meta - rounded as libsnd rounds them.
    pub fn score_tempos(&self, id: u16) -> BTreeSet<u32> {
        let mut out = BTreeSet::new();
        let Some(entry) = self.entry(id) else {
            return out;
        };
        let Some(split) = legaia_engine_system::chunk_install::owned_bank_offsets(entry) else {
            return out;
        };
        let Some(seq) = entry
            .get(split.seq..)
            .and_then(|b| legaia_seq::Seq::parse(b).ok())
        else {
            return out;
        };
        out.insert(libsnd_bpm(seq.header.tempo_us_per_qn));
        for e in &seq.events {
            if let legaia_seq::EventBody::Meta(legaia_seq::MetaMessage::SetTempo { us_per_qn }) =
                e.body
            {
                out.insert(libsnd_bpm(us_per_qn));
            }
        }
        out
    }
}

/// `60000000 / us_per_qn`, rounded up only past the half
/// (`FUN_80062410` `0x80062598..0x800625E4`).
pub fn libsnd_bpm(us_per_qn: u32) -> u32 {
    if us_per_qn == 0 {
        return 0;
    }
    let q = 60_000_000 / us_per_qn;
    let r = 60_000_000 % us_per_qn;
    if (us_per_qn >> 1) < r { q + 1 } else { q }
}

impl RetailAudio {
    /// Read libsnd's state off a retail main-RAM image. `env` overrides the
    /// voice records' envelope copies with the SPU's own (a mednafen state's
    /// SPU section), when the caller has it.
    pub fn from_ram(ram: &[u8], bank: &TrackBank, env: Option<[u16; 24]>) -> Self {
        let at = |va: u32| (va & 0x1F_FFFF) as usize;
        let rd8 = |va: u32| ram.get(at(va)).copied().unwrap_or(0);
        let rd16 = |va: u32| u16::from_le_bytes([rd8(va), rd8(va + 1)]);
        let rd32 = |va: u32| u32::from(rd16(va)) | (u32::from(rd16(va + 2)) << 16);
        let bitmap = rd16(SEQ_BITMAP);
        let source_of = |seq: u8| {
            (0..16u8).find(|&k| {
                let r = SOUND_SOURCES + u32::from(k) * 0x10;
                rd16(r + 8) != 0 && rd16(r + 0xA) == u16::from(seq)
            })
        };
        let mut seqs = Vec::new();
        for slot in 0..16u8 {
            if bitmap & (1 << slot) == 0 {
                continue;
            }
            let rec = rd32(SEQ_TABLE + u32::from(slot) * 4);
            if rec & 0xFF00_0000 != 0x8000_0000 {
                continue;
            }
            let start = at(rd32(rec + 4));
            let score = ram.get(start..start + 24).unwrap_or(&[]);
            let vab_id = rd8(rec + 0x26);
            let hdr = at(rd32(VAB_HEADERS + u32::from(vab_id) * 12));
            let vab_size = ram.get(hdr + 12..hdr + 0x20);
            let flags = rd16(rec + 0x98);
            let (score_tracks, bank_tracks) = bank.identify(score, vab_size);
            let track = bank_tracks.first().or(score_tracks.first()).copied();
            seqs.push(RetailSeqSlot {
                slot,
                source: source_of(slot),
                track,
                score_tracks,
                bank_tracks,
                flags,
                playing: flags & 1 != 0,
                vol: (rd16(rec + 0x58), rd16(rec + 0x5A)),
                tempo_bpm: rd32(rec + 0x94),
                vab_id,
            });
        }
        let mut cue_voices = Vec::new();
        let mut seq_voices = 0;
        for v in 0..24u32 {
            let r = VOICE_RECORDS + v * VOICE_STRIDE;
            let level = env.map_or(rd16(r + 6), |e| e[v as usize]);
            if level == 0 {
                continue;
            }
            match rd16(r + 0x10) {
                CUE_OWNER => cue_voices.push((v as u8, rd16(r + 0x18))),
                0xFFFF => {}
                _ => seq_voices += 1,
            }
        }
        Self {
            track_word: rd16(0x8007_BAC8),
            swap: (rd16(0x8007_BAB8), rd16(0x8007_BA9C)),
            sound_flags: rd16(0x8007_B750),
            battle_sound_set: rd32(0x8007_B880) as i32,
            battle_alt: rd8(0x8007_B64B),
            formation_head: rd8(FORMATION),
            seqs,
            cue_voices,
            seq_voices,
        }
    }

    /// The sequence a listener hears as the scene's music: the one a
    /// playing track sounds through - the field BGM source's, the ending
    /// source's, any other track - else the field BGM source's held one.
    pub fn music_slot(&self) -> Option<&RetailSeqSlot> {
        let src = |k: u8| move |s: &&RetailSeqSlot| s.source == Some(k);
        let seqs = || self.seqs.iter();
        seqs()
            .filter(|s| s.playing)
            .find(src(FIELD_BGM_SOURCE))
            .or_else(|| seqs().filter(|s| s.playing).find(src(ENDING_SOURCE)))
            .or_else(|| seqs().find(|s| s.playing && s.track.is_some()))
            .or_else(|| seqs().find(src(FIELD_BGM_SOURCE)))
    }
}

/// Frames of one 60 Hz video frame at the SPU rate.
const FRAMES_PER_TICK: usize = (SPU_INTERNAL_RATE / 60) as usize;
/// The span, in ticks before the sample, the sounding-voice peaks cover.
const LISTEN_SPAN: usize = (SETTLE_TICKS / 2) as usize;

/// One settle tick of the listener: the play hosts' audio frame
/// ([`legaia_engine_session::BootSession::step_audio_frame`], which both
/// hosts run once per sim tick), one video frame of samples pulled through
/// the output's mix core, then the SPU's sounding voices by bus.
pub fn listen_frame<S: AudioSink>(session: &mut BootSession<S>, obs: &mut EngineAudio) {
    if let Some(r) = session.step_audio_frame() {
        obs.cues_fired.extend(r.fired.iter().map(|(id, _)| *id));
    }
    let Some(d) = session.bgm.as_ref() else {
        return;
    };
    d.audio().with_core(|core| {
        for _ in 0..FRAMES_PER_TICK {
            core.next_frame();
        }
    });
    let counts = d.audio().with_spu(|spu| {
        spu.voices
            .iter()
            .filter(|v| !v.is_off() && v.adsr.level > 0)
            .fold((0, 0), |(s, b), v| match v.bus {
                VoiceBus::Sfx => (s + 1, b),
                VoiceBus::Bgm => (s, b + 1),
            })
    });
    obs.recent.push(counts);
    if obs.recent.len() > LISTEN_SPAN {
        obs.recent.remove(0);
    }
}

/// Read the director at the sample.
pub fn finish_listen<S: AudioSink>(session: &BootSession<S>, obs: &mut EngineAudio) {
    obs.sfx_voices_peak = obs.recent.iter().map(|c| c.0).max().unwrap_or(0);
    obs.bgm_voices_peak = obs.recent.iter().map(|c| c.1).max().unwrap_or(0);
    obs.track_word = session.host.bgm_track_word;
    let Some(d) = session.bgm.as_ref() else {
        return;
    };
    let prog = d.audio().sequencer_progress();
    obs.track = prog.is_some().then_some(d.last_started).flatten();
    obs.playing = d.is_playing();
    obs.seq_vol = prog.map(|p| p.master_vol);
    obs.tempo_bpm = prog.map(|p| p.bpm);
}

/// Score one state's channels.
pub fn score(
    retail: &RetailAudio,
    engine: &EngineAudio,
    bank: &TrackBank,
) -> (BTreeMap<String, f64>, BTreeMap<String, String>) {
    let mut ch = BTreeMap::new();
    let mut det = BTreeMap::new();
    let mut put = |c: &str, ok: bool, d: String| {
        ch.insert(c.to_string(), f64::from(u8::from(ok)));
        det.insert(c.to_string(), d);
    };
    let music = retail.music_slot();
    let r_track = music.and_then(|s| s.track);
    let same_track = match music {
        Some(m) if !m.bank_tracks.is_empty() => {
            engine.track.is_some_and(|t| m.bank_tracks.contains(&t))
        }
        _ => r_track == engine.track,
    };
    put(
        "track",
        same_track,
        format!(
            "retail slot {:?} track {r_track:?} {:?} (word {}, swap {:?}, flags {:#06x}) \
             engine {:?} (word {:?})",
            music.map(|s| s.slot),
            music.map(|s| &s.bank_tracks),
            retail.track_word,
            retail.swap,
            retail.sound_flags,
            engine.track,
            engine.track_word
        ),
    );
    let r_playing = music.is_some_and(|s| s.playing);
    put(
        "playing",
        r_playing == engine.playing,
        format!(
            "retail {} (flags {:#06x}) engine {}",
            r_playing,
            music.map_or(0, |s| s.flags),
            engine.playing
        ),
    );
    if let (Some(m), Some(ev)) = (music, engine.seq_vol)
        && m.playing
        && engine.playing
        && same_track
    {
        put(
            "seq_vol",
            m.vol.0.min(127) as u8 == ev,
            format!("retail {:?} engine {ev}", m.vol),
        );
    }
    if let Some(m) = music
        && let Some(t) = m.track
        && m.playing
    {
        let tempos = bank.score_tempos(t);
        put(
            "tempo",
            tempos.contains(&m.tempo_bpm),
            format!(
                "retail {} BPM, score writes {tempos:?}, engine {:?}",
                m.tempo_bpm, engine.tempo_bpm
            ),
        );
    }
    if !retail.cue_voices.is_empty() {
        put(
            "ambient",
            engine.sfx_voices_peak as usize >= retail.cue_voices.len(),
            format!(
                "retail cue voices {:?} engine sfx peak {} cues {:?}",
                retail.cue_voices, engine.sfx_voices_peak, engine.cues_fired
            ),
        );
    }
    (ch, det)
}

/// Score a battle state's track against the engine's selection rule
/// ([`legaia_engine_system::music_labels::battle_track_for_sound_set`], the
/// rule `World::swap_to_battle_bgm` runs) fed retail's own inputs: the sound
/// set and `DAT_8007B64B`. No engine is seeded - the battle half of the
/// corpus seeds through `retail_compare_battle`, which this sweep leaves
/// alone.
pub fn score_battle(retail: &RetailAudio) -> (BTreeMap<String, f64>, BTreeMap<String, String>) {
    use legaia_engine_system::music_labels::{BATTLE_THEME_1_BGM_ID, battle_track_for_sound_set};
    let mut ch = BTreeMap::new();
    let mut det = BTreeMap::new();
    let battle = retail
        .seqs
        .iter()
        .find(|s| s.playing && s.source == Some(BATTLE_SOURCE));
    let field = retail
        .seqs
        .iter()
        .find(|s| s.playing && s.source == Some(FIELD_BGM_SOURCE));
    let set = retail.battle_sound_set;
    let expected = battle_track_for_sound_set(set, retail.battle_alt != 0, BATTLE_THEME_1_BGM_ID);
    let names = |s: Option<&RetailSeqSlot>| s.map(|s| s.bank_tracks.clone());
    let d = format!(
        "set {set} alt {} head {:#04x}: rule {expected:?}, retail battle source {:?} field source {:?}",
        retail.battle_alt,
        retail.formation_head,
        names(battle),
        names(field)
    );
    if retail.formation_head == GAZA_FORMATION_HEAD {
        det.insert(
            "battle_track".into(),
            format!("not scored (set zeroed by the intro): {d}"),
        );
        return (ch, det);
    }
    let ok = match (expected, battle) {
        (None, None) => field.is_some(),
        (Some(t), Some(b)) => b.bank_tracks.contains(&t) || b.score_tracks.contains(&t),
        (None, Some(_)) => false,
        (Some(_), None) => {
            det.insert(
                "battle_track".into(),
                format!("not scored (no battle bundle loaded - a forced entry): {d}"),
            );
            return (ch, det);
        }
    };
    ch.insert("battle_track".into(), f64::from(u8::from(ok)));
    det.insert("battle_track".into(), d);
    (ch, det)
}

/// Read a corpus state's RAM (and, for mednafen, the SPU envelopes).
fn read_state(entry: &CorpusEntry, scus: &[u8]) -> Result<(Vec<u8>, Option<[u16; 24]>)> {
    if entry.emulator == "mednafen" {
        let st = legaia_mednafen::SaveState::from_path(&entry.path)?;
        let ram = st.main_ram()?.to_vec();
        let spu = legaia_mednafen::PsxSpu::new(&st);
        let mut env = [0u16; 24];
        for (i, v) in spu.voices().iter().enumerate() {
            env[i] = v.adsr_env_level.unwrap_or(0);
        }
        Ok((ram, Some(env)))
    } else {
        let (st, _) = legaia_pcsx_redux::gpu::load_with_scus(&entry.path, scus)?;
        Ok((st.main_ram().to_vec(), None))
    }
}

/// Run the sweep over the library.
pub fn run_sweep(
    manifest: &ScenarioManifest,
    library: &Path,
    extracted: &Path,
    filter: Option<&str>,
) -> Result<Vec<AudioSweepRow>> {
    let scus = std::fs::read(extracted.join("SCUS_942.54")).context("read SCUS_942.54")?;
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT"))?;
    let bank = TrackBank::load(&SceneHost::open_extracted(extracted)?);
    let mut rows = Vec::new();
    for entry in enumerate_corpus(manifest, library) {
        if excluded_state(&entry) {
            continue;
        }
        if let Some(f) = filter
            && !f.split(',').filter(|a| !a.is_empty()).any(|a| {
                a.strip_prefix('=')
                    .map_or(entry.label.contains(a), |x| entry.label == x)
            })
        {
            continue;
        }
        let mut row = AudioSweepRow {
            label: entry.label.clone(),
            emulator: entry.emulator.to_string(),
            scene: String::new(),
            class: String::new(),
            retail: RetailAudio::default(),
            engine: None,
            unseeded: None,
            channels: BTreeMap::new(),
            detail: BTreeMap::new(),
        };
        let mut obs = match crate::retail_compare::read_retail(&entry, &scus) {
            Ok(o) => o,
            Err(e) => {
                row.unseeded = Some(format!("unreadable state: {e:#}"));
                rows.push(row);
                continue;
            }
        };
        let (ram, env) = match read_state(&entry, &scus) {
            Ok(x) => x,
            Err(e) => {
                row.unseeded = Some(format!("unreadable state: {e:#}"));
                rows.push(row);
                continue;
            }
        };
        row.retail = RetailAudio::from_ram(&ram, &bank, env);
        obs.settle_on_loaded_scene(&cdname);
        row.scene = obs.scene.clone();
        row.class = format!("{:?}", obs.class);
        if obs.class == StateClass::Battle {
            let (c, d) = score_battle(&row.retail);
            row.channels = c;
            row.detail = d;
        }
        if !matches!(obs.class, StateClass::Field | StateClass::WorldMap) {
            row.unseeded = Some(format!("{:?}: not a walkable state", obs.class));
            rows.push(row);
            continue;
        }
        // A walked capture whose manifest names its door is entered through
        // it, so the music the door kept comes from the engine's own door
        // logic; any other walkable state seeds through the card load.
        let door = manifest
            .scenarios
            .iter()
            .find(|sc| sc.label == entry.label)
            .and_then(|sc| sc.arrival.as_ref());
        let run = match door {
            Some(d) => crate::retail_compare::run_engine_listening_through_door(extracted, &obs, d),
            None => crate::retail_compare::run_engine_listening(extracted, &obs),
        };
        match run {
            Ok((_, engine)) => {
                let (c, d) = score(&row.retail, &engine, &bank);
                row.channels = c;
                row.detail = d;
                row.engine = Some(engine);
            }
            Err(e) => row.unseeded = Some(format!("seed failed: {e:#}")),
        }
        rows.push(row);
    }
    Ok(rows)
}

/// Per-channel mean over the scored states, and how many states scored it.
pub fn channel_means(rows: &[AudioSweepRow]) -> BTreeMap<String, (f64, usize)> {
    let mut acc: BTreeMap<String, (f64, usize)> = BTreeMap::new();
    for r in rows {
        for (c, v) in &r.channels {
            let e = acc.entry(c.clone()).or_default();
            e.0 += v;
            e.1 += 1;
        }
    }
    for v in acc.values_mut() {
        v.0 /= v.1.max(1) as f64;
    }
    acc
}

/// `legaia-engine audio-sweep`: run the sweep and write `report.json` (and a
/// short text summary on stdout). The report carries no Sony bytes - track
/// ids, flag words, counts.
#[derive(clap::Args, Debug)]
pub struct AudioSweepArgs {
    /// Save-state library (default: `$LEGAIA_SAVES_LIBRARY`, else
    /// `saves/library`).
    #[arg(long)]
    pub library: Option<std::path::PathBuf>,
    /// Extracted disc (default: `$LEGAIA_EXTRACTED_DIR`, else `extracted`).
    #[arg(long)]
    pub extracted_root: Option<std::path::PathBuf>,
    /// Scenario manifest (default: `scripts/scenarios.toml`).
    #[arg(long)]
    pub manifest: Option<std::path::PathBuf>,
    /// Report path (gitignored directory).
    #[arg(long, default_value = "captures/audio-sweep/report.json")]
    pub out: std::path::PathBuf,
    /// Only states whose label contains this substring (comma = any of
    /// several; `=label` matches exactly).
    #[arg(long)]
    pub filter: Option<String>,
}

pub fn run(args: AudioSweepArgs) -> Result<()> {
    let (m, l, e) = crate::retail_compare::resolve_dirs();
    let manifest_path = args.manifest.or(m).context("no scripts/scenarios.toml")?;
    let library = args.library.or(l).context("no save library")?;
    let extracted = args.extracted_root.or(e).context("no extracted disc")?;
    let manifest = ScenarioManifest::from_path(&manifest_path)?;
    let rows = run_sweep(&manifest, &library, &extracted, args.filter.as_deref())?;
    if let Some(dir) = args.out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&args.out, serde_json::to_string_pretty(&rows)?)
        .with_context(|| format!("write {}", args.out.display()))?;
    let scored = rows.iter().filter(|r| r.engine.is_some()).count();
    println!(
        "audio-sweep: {} states read, {scored} seeded -> {}",
        rows.len(),
        args.out.display()
    );
    for (c, (mean, n)) in channel_means(&rows) {
        println!("  {c:<8} {mean:.3} over {n}");
    }
    for r in &rows {
        for (c, v) in &r.channels {
            if *v < 1.0 {
                println!(
                    "  MISS {:<40} {c:<8} {}",
                    r.label,
                    r.detail.get(c).map_or("", String::as_str)
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libsnd_rounds_up_only_past_the_half() {
        // 500000 us/qn = 120 BPM exactly.
        assert_eq!(libsnd_bpm(500_000), 120);
        // 60e6 / 750001 = 79.99989: remainder 749921 > 375000 -> 80.
        assert_eq!(libsnd_bpm(750_001), 80);
        // 60e6 / 7 = 8571428 r 4; 7 >> 1 = 3 < 4 -> rounds up.
        assert_eq!(libsnd_bpm(7), 8_571_429);
        // 60e6 / 8 = 7500000 r 0 -> exact.
        assert_eq!(libsnd_bpm(8), 7_500_000);
        assert_eq!(libsnd_bpm(0), 0);
    }

    fn row_ram() -> Vec<u8> {
        vec![0u8; 0x20_0000]
    }

    #[test]
    fn an_empty_libsnd_reads_no_slots_and_no_voices() {
        let bank = TrackBank { tracks: Vec::new() };
        let a = RetailAudio::from_ram(&row_ram(), &bank, None);
        assert!(a.seqs.is_empty());
        assert!(a.cue_voices.is_empty());
        assert_eq!(a.seq_voices, 0);
    }

    #[test]
    fn a_playing_slot_and_a_cue_voice_read_back() {
        let mut ram = row_ram();
        let put16 = |ram: &mut [u8], va: u32, v: u16| {
            let o = (va & 0x1F_FFFF) as usize;
            ram[o..o + 2].copy_from_slice(&v.to_le_bytes());
        };
        let put32 = |ram: &mut [u8], va: u32, v: u32| {
            let o = (va & 0x1F_FFFF) as usize;
            ram[o..o + 4].copy_from_slice(&v.to_le_bytes());
        };
        put16(&mut ram, SEQ_BITMAP, 1 << 1);
        put32(&mut ram, SEQ_TABLE + 4, 0x8010_0000);
        put16(&mut ram, 0x8010_0098, 0x0001);
        // The field BGM source names libsnd sequence 1.
        put16(&mut ram, SOUND_SOURCES + 0x10 + 8, 1);
        put16(&mut ram, SOUND_SOURCES + 0x10 + 0xA, 1);
        put16(&mut ram, 0x8010_0058, 127);
        put32(&mut ram, 0x8010_0094, 80);
        // Voice 3: a cue voice with a live envelope; voice 4: a sequence's.
        put16(&mut ram, VOICE_RECORDS + 3 * VOICE_STRIDE + 6, 0x1000);
        put16(&mut ram, VOICE_RECORDS + 3 * VOICE_STRIDE + 0x10, CUE_OWNER);
        put16(&mut ram, VOICE_RECORDS + 3 * VOICE_STRIDE + 0x18, 6);
        put16(&mut ram, VOICE_RECORDS + 4 * VOICE_STRIDE + 6, 0x1000);
        put16(&mut ram, VOICE_RECORDS + 4 * VOICE_STRIDE + 0x10, 0x0001);
        let bank = TrackBank { tracks: Vec::new() };
        let a = RetailAudio::from_ram(&ram, &bank, None);
        assert_eq!(a.seqs.len(), 1);
        let s = a.music_slot().unwrap();
        assert!(s.playing);
        assert_eq!((s.slot, s.vol.0, s.tempo_bpm), (1, 127, 80));
        assert_eq!(s.source, Some(FIELD_BGM_SOURCE));
        assert_eq!(a.cue_voices, vec![(3, 6)]);
        assert_eq!(a.seq_voices, 1);
    }
}
