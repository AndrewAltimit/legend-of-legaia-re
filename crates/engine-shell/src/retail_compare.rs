//! Retail comparison corpus: put the engine where each retail save state
//! is and score, channel by channel, how far what it shows is from what
//! retail showed.
//!
//! The corpus is the save-state library catalogued in `scripts/scenarios.toml`
//! (both emulators' backups). A state's RAM and VRAM carry every observable
//! this module compares, so nothing here runs an emulator:
//!
//! - **identity** - the CDNAME scene label (`0x8007050C`) and the next
//!   game-mode word (`0x8007B83C`);
//! - **player** - `(X, footing, Z)` at `player+0x14/0x16/0x18`;
//! - **camera** - pitch / yaw at `0x8007B790/92`, GTE `H` at `0x8007B6F4`,
//!   the eye trio at `0x800840B8/BC/C0`;
//! - **BGM** - the track-select word `0x8007BAC8`;
//! - **party / flags / bag / gold** - the live game-state window at
//!   `0x80084140`, which is byte for byte the front `0x1A18` bytes of a save
//!   block (the composer's own copy, `docs/subsystems/save-screen.md`), so it
//!   lifts through [`legaia_save::SaveFile::from_retail_sc_block`] exactly as
//!   a memory-card save does;
//! - **frame** - the on-screen framebuffer, cropped out of VRAM by the
//!   display-start / display-mode registers
//!   ([`crate::retail_compare_image`]).
//!
//! The engine is seeded through its own card-load path
//! ([`crate::BootSession::resume_save`] over the lifted block), then the
//! player is seated on the state's `(X, Z)` - the same debug seat
//! `LEGAIA_SEAT` gives `play-window` - and the session ticks
//! [`SETTLE_TICKS`] frames with no input. The seeding model and its gaps are
//! documented in `docs/tooling/retail-compare.md`.
//!
//! Only walkable states (field-run in a field scene or on a kingdom
//! overworld) are seedable today. Every other class is catalogued with its
//! retail observables and a reason, so the corpus summary counts what the
//! instrument cannot reach instead of hiding it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use legaia_engine_core::scene::BgmDirector;
use legaia_engine_core::world::SceneMode;
use legaia_mednafen::game_anchors;
use legaia_mednafen::{PsxGpu, SaveState as MednafenState, ScenarioManifest};
use serde::{Deserialize, Serialize};

use crate::boot::{BootConfig, BootSession, FieldLiveOpts};
use crate::retail_compare_image::{Frame, ImageScore};

/// Engine ticks run between seeding and sampling.
pub const SETTLE_TICKS: u64 = 60;

/// Start of the live game-state window a save block is composed from.
const LIVE_STATE_VA: u32 = 0x8008_4140;
/// Bytes of it a save block carries.
const LIVE_STATE_LEN: usize = legaia_save::card::RETAIL_LIVE_STATE_SIZE;
/// A retail save block's size (the window is padded to it).
const SC_BLOCK_LEN: usize = 0x2000;
const GTE_H: u32 = 0x8007_B6F4;
const CAM_ROT: u32 = 0x8007_B790;
const CAM_EYE: u32 = 0x8008_40B8;
const BGM_ID: u32 = 0x8007_BAC8;
/// The ambient-particle (fog pool) master gate, raised / cleared only by
/// field-VM op `0x4C` nibble 3 (`docs/subsystems/field-ambient-fx.md`).
const FOG_GATE: u32 = 0x8007_B854;

/// What kind of retail situation a state is, as the engine would have to be
/// seeded to reproduce it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateClass {
    /// Field-run (`0x03`) in a field / town / dungeon scene with a player.
    Field,
    /// Field-run on a kingdom overworld (`mapNN`), which the port models as
    /// its own world-map mode.
    WorldMap,
    /// Field-init (`0x02`): a scene mid-load.
    FieldInit,
    /// Battle init / run (`0x14` / `0x15`).
    Battle,
    /// The CARD pair (`0x17`): title / save screens and the pause menu.
    Menu,
    /// STR movie modes (`0x1A` / `0x1B`).
    Cutscene,
    /// `OTHER MODE` (`0x19`): the minigames.
    Minigame,
    /// Anything else (boot / logo modes), or a state whose anchors do not
    /// read.
    Other,
}

impl StateClass {
    /// Whether the engine can currently be seeded into this class.
    pub fn seedable(self) -> bool {
        matches!(self, StateClass::Field | StateClass::WorldMap)
    }

    /// The reason a non-seedable class is not seeded.
    pub fn unseeded_reason(self) -> &'static str {
        match self {
            StateClass::Field | StateClass::WorldMap => "",
            StateClass::FieldInit => "scene mid-load; no settled frame to reproduce",
            StateClass::Battle => "no battle seeding path (formation + actor table from RAM)",
            StateClass::Menu => "no pause-menu / title seeding path",
            StateClass::Cutscene => "STR playback is not a seeded state",
            StateClass::Minigame => "no minigame session seeding path",
            StateClass::Other => "boot / unknown mode",
        }
    }

    fn classify(mode: u8, scene: &str, has_player: bool) -> Self {
        match mode {
            0x03 if legaia_engine_core::scene::is_world_map_scene(scene) => StateClass::WorldMap,
            0x03 if has_player && !scene.is_empty() => StateClass::Field,
            0x02 => StateClass::FieldInit,
            0x14 | 0x15 => StateClass::Battle,
            0x17 => StateClass::Menu,
            0x1A | 0x1B => StateClass::Cutscene,
            0x19 => StateClass::Minigame,
            _ => StateClass::Other,
        }
    }
}

/// One library state the corpus walks.
#[derive(Debug, Clone)]
pub struct CorpusEntry {
    /// Scenario label (the first scenario naming this backup).
    pub label: String,
    /// `mednafen` or `pcsx-redux`.
    pub emulator: &'static str,
    /// The backup file.
    pub path: PathBuf,
    /// The scenario's `backup_fingerprint`.
    pub fingerprint: String,
}

/// Enumerate every scenario with a library backup on disk, deduplicated by
/// file (several scenarios can name one backup; the first label wins).
pub fn enumerate_corpus(manifest: &ScenarioManifest, library: &Path) -> Vec<CorpusEntry> {
    let mut out: Vec<CorpusEntry> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for sc in &manifest.scenarios {
        let Some(fp) = sc.backup_fingerprint.as_deref() else {
            continue;
        };
        for emulator in ["mednafen", "pcsx-redux"] {
            let Some(path) = legaia_mednafen::scenarios::library_backup_for(emulator, library, fp)
            else {
                continue;
            };
            if seen.insert(path.clone()) {
                out.push(CorpusEntry {
                    label: sc.label.clone(),
                    emulator,
                    path,
                    fingerprint: fp.to_string(),
                });
            }
        }
    }
    out
}

/// The camera observables, both sides use the same shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraObs {
    pub pitch: i16,
    pub yaw: i16,
    pub h: i16,
    pub eye: [i32; 3],
}

/// Everything read off one retail state.
pub struct RetailObs {
    pub scene: String,
    pub game_mode: u8,
    pub class: StateClass,
    /// `(X, footing, Z)`.
    pub player: Option<[i16; 3]>,
    pub camera: CameraObs,
    pub bgm_id: u16,
    /// `_DAT_8007B854 != 0`.
    pub fog_gate: bool,
    /// The live game-state window lifted as a save.
    pub save: Option<legaia_save::SaveFile>,
    /// The displayed frame, when the state carries VRAM + display registers.
    pub frame: Option<Frame>,
}

fn rd16(ram: &[u8], va: u32) -> i16 {
    game_anchors::i16_at(ram, va)
}

fn rd32(ram: &[u8], va: u32) -> i32 {
    game_anchors::u32_at(ram, va) as i32
}

impl RetailObs {
    fn from_ram(ram: &[u8], frame: Option<Frame>) -> Self {
        let scene = game_anchors::scene_name(ram);
        let game_mode = game_anchors::game_mode(ram);
        let player = game_anchors::player_ptr(ram).map(|p| {
            [
                rd16(ram, p + 0x14),
                rd16(ram, p + 0x16),
                rd16(ram, p + 0x18),
            ]
        });
        let class = StateClass::classify(game_mode, &scene, player.is_some());
        let camera = CameraObs {
            pitch: rd16(ram, CAM_ROT),
            yaw: rd16(ram, CAM_ROT + 2),
            h: rd16(ram, GTE_H),
            eye: [
                rd32(ram, CAM_EYE),
                rd32(ram, CAM_EYE + 4),
                rd32(ram, CAM_EYE + 8),
            ],
        };
        let bgm_id = game_anchors::u16_at(ram, BGM_ID);
        let fog_gate = game_anchors::u32_at(ram, FOG_GATE) != 0;
        let lo = (LIVE_STATE_VA & 0x1F_FFFF) as usize;
        let save = ram.get(lo..lo + LIVE_STATE_LEN).and_then(|win| {
            let mut block = win.to_vec();
            block.resize(SC_BLOCK_LEN, 0);
            legaia_save::SaveFile::from_retail_sc_block(
                &block,
                legaia_save::RETAIL_SC_PARTY_RECORDS,
            )
            .ok()
        });
        Self {
            scene,
            game_mode,
            class,
            player,
            camera,
            bgm_id,
            fog_gate,
            save,
            frame,
        }
    }
}

/// Read a library state of either emulator. `scus` is `SCUS_942.54`, which a
/// PCSX-Redux state needs for its RAM anchor search.
pub fn read_retail(entry: &CorpusEntry, scus: &[u8]) -> Result<RetailObs> {
    match entry.emulator {
        "mednafen" => {
            let st = MednafenState::from_path(&entry.path)
                .with_context(|| format!("parse {}", entry.path.display()))?;
            let ram = st.main_ram()?;
            let gpu = PsxGpu::new(&st);
            let frame = match (gpu.vram_bytes(), gpu.regs().display_crop_rect()) {
                (Some(v), Some(rect)) => Frame::from_vram_display(v, rect),
                _ => None,
            };
            Ok(RetailObs::from_ram(ram, frame))
        }
        _ => {
            let (st, gpu) = legaia_pcsxr::gpu::load_with_scus(&entry.path, scus)?;
            let frame = gpu.and_then(|g| Frame::from_vram_display(&g.vram, g.display_crop_rect()));
            Ok(RetailObs::from_ram(st.main_ram(), frame))
        }
    }
}

/// What the engine shows after seeding.
pub struct EngineObs {
    pub scene: Option<String>,
    pub mode: SceneMode,
    pub player: Option<[i16; 3]>,
    /// The engine's floor sample under retail's own `(X, Z)`.
    pub floor_at_retail: Option<i32>,
    pub camera: CameraObs,
    /// The engine's track-select word after the settle window
    /// ([`legaia_engine_core::scene::SceneHost::bgm_track_word`]), else the
    /// last track the director started.
    pub bgm_id: Option<u16>,
    /// The engine's fog-pool gate (`World::fog.gate`).
    pub fog_gate: bool,
    pub save: legaia_save::SaveFile,
}

/// Records which track the field VM starts.
#[derive(Default)]
struct RecordingDirector {
    last: Option<u16>,
}

impl BgmDirector for RecordingDirector {
    fn start(&mut self, bgm_id: u16, _seq: &[u8]) {
        self.last = Some(bgm_id);
    }
    fn start_owned_vab(&mut self, bgm_id: u16, _entry: &[u8]) {
        self.last = Some(bgm_id);
    }
    // The comparand is the id the scripts selected (retail's track-select
    // word is written by the op-0x35 start arms). A control op starts no
    // track, so each one is overridden on purpose and keeps `last`.
    fn pause(&mut self) {}
    fn resume(&mut self) {}
    fn stop(&mut self) {}
    fn unhalt_pause(&mut self) {}
}

/// Seed the engine from a retail state and sample it after [`SETTLE_TICKS`].
/// How the retail save is applied around the scene entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SeedOrder {
    /// The engine's own card-load path ([`BootSession::resume_save`]): enter
    /// the scene, then hydrate. The scene's entry scripts run before the
    /// save's story flags exist.
    #[default]
    Resume,
    /// Hydrate, enter, hydrate again: the entry scripts see the retail
    /// flags. A diagnostic arm - the difference between the two orders is
    /// how much of a channel's divergence the entry ordering explains.
    FlagsFirst,
}

/// Seed through the engine's own card-load order ([`SeedOrder::Resume`]).
pub fn run_engine(extracted: &Path, retail: &RetailObs) -> Result<EngineObs> {
    run_engine_with(extracted, retail, SeedOrder::Resume)
}

/// Seed the engine from a retail state in the given order and sample it
/// after [`SETTLE_TICKS`].
pub fn run_engine_with(
    extracted: &Path,
    retail: &RetailObs,
    order: SeedOrder,
) -> Result<EngineObs> {
    let cfg = BootConfig {
        scene: retail.scene.clone(),
        enable_audio: false,
    };
    let mut session = BootSession::open(extracted, &cfg).context("open boot session")?;
    let opts = FieldLiveOpts::default();
    let save = retail
        .save
        .clone()
        .context("retail state has no liftable save window")?;
    match order {
        // The engine's own card-load path: land the save's scene, then hydrate.
        SeedOrder::Resume => {
            let _landing = session.resume_save(save, &retail.scene, &opts);
        }
        SeedOrder::FlagsFirst => {
            session.host.world.load_full(save.clone());
            session.enter_scene_live(&retail.scene, &opts)?;
            session.host.world.load_full(save);
        }
    }
    let mut director = RecordingDirector::default();
    // The entry itself may already have queued a start.
    session.host.route_bgm_events(&mut director)?;
    if let Some([x, _, z]) = retail.player
        && session.host.world.debug_seat_player(x, z)
    {
        session.camera.zone.arm_arrival();
    }
    for _ in 0..SETTLE_TICKS {
        session.tick()?;
        session.host.route_bgm_events(&mut director)?;
    }
    let world = &mut session.host.world;
    let player = world.player_actor_slot.and_then(|s| {
        world.actors.get(s as usize).map(|a| {
            [
                a.move_state.world_x,
                a.move_state.world_y,
                a.move_state.world_z,
            ]
        })
    });
    let floor_at_retail = retail
        .player
        .map(|[x, _, z]| world.sample_field_floor_height(i32::from(x), i32::from(z)));
    let g = &session.camera.globals.0;
    let camera = CameraObs {
        pitch: g[0] as i16,
        yaw: g[1] as i16,
        h: g[9] as i16,
        eye: [g[3], g[4], g[5]],
    };
    let mode = world.mode;
    let fog_gate = world.fog.gate;
    let save = world.save_full();
    let scene = session.host.scene.as_ref().map(|s| s.name.clone());
    // The engine's `_DAT_8007BAC8`: a park-sentinel start (`0x1000`, the
    // ending scenes') reaches no director, but it is the word retail holds.
    let bgm_id = session.host.bgm_track_word.or(director.last);
    Ok(EngineObs {
        scene,
        mode,
        player,
        floor_at_retail,
        camera,
        bgm_id,
        fog_gate,
        save,
    })
}

/// Offset of the system-flag bank (`0x80085758`) inside the story-flag
/// bitmap a save carries (`0x80085600`).
const SYSTEM_FLAG_WINDOW: usize = 0x158;

/// Every raised bit of the save's system-flag bank, as the flag ids
/// `World::system_flag_set` takes (MSB-first within a byte).
pub fn system_flag_ids(save: &legaia_save::SaveFile) -> Vec<u16> {
    let bits = save
        .ext
        .story_flag_bits
        .get(SYSTEM_FLAG_WINDOW..)
        .unwrap_or(&[]);
    let mut out = Vec::new();
    for (b, &byte) in bits.iter().enumerate() {
        for k in 0..8 {
            if byte & (0x80 >> k) != 0 {
                out.push((b * 8 + k) as u16);
            }
        }
    }
    out
}

/// Linear falloff: `1` within `full`, `0` at or beyond `zero`.
fn falloff(delta: f64, full: f64, zero: f64) -> f64 {
    let d = delta.abs();
    if d <= full {
        1.0
    } else if d >= zero {
        0.0
    } else {
        1.0 - (d - full) / (zero - full)
    }
}

/// Wrapped difference of two 12-bit angles.
fn angle_delta(a: i16, b: i16) -> f64 {
    let d = (i32::from(a) - i32::from(b)).rem_euclid(4096);
    f64::from(d.min(4096 - d))
}

/// The channel names, in report order.
pub const CHANNELS: &[&str] = &[
    "scene",
    "mode",
    "position",
    "footing",
    "camera",
    "bgm",
    "fog_gate",
    "party",
    "flags",
    "inventory",
    "image",
];

/// One state's result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateReport {
    pub label: String,
    pub emulator: String,
    pub fingerprint: String,
    pub scene: String,
    pub game_mode: u8,
    pub class: StateClass,
    /// Empty when seeded; otherwise why not.
    pub unseeded: String,
    /// Channel -> score in `[0, 1]`. Absent = not measured.
    pub channels: BTreeMap<String, f64>,
    /// Channel -> one-line human detail (both sides' values).
    pub detail: BTreeMap<String, String>,
    /// Mean of the measured channels.
    pub score: Option<f64>,
    /// Pixel metric detail when the image channel ran.
    pub image: Option<ImageScore>,
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn party_score(
    retail: &legaia_save::Party,
    engine: &legaia_save::Party,
) -> (f64, String, Vec<String>) {
    let mut total = 0usize;
    let mut equal = 0usize;
    let mut diffs = Vec::new();
    for (i, r) in retail.members.iter().enumerate() {
        let e = engine.members.get(i);
        let rv = r.hp_mp_sp();
        let fields: [(&str, i64, Option<i64>); 6] = [
            (
                "hp",
                i64::from(rv.hp_cur),
                e.map(|e| i64::from(e.hp_mp_sp().hp_cur)),
            ),
            (
                "hp_max",
                i64::from(rv.hp_max),
                e.map(|e| i64::from(e.hp_mp_sp().hp_max)),
            ),
            (
                "mp",
                i64::from(rv.mp_cur),
                e.map(|e| i64::from(e.hp_mp_sp().mp_cur)),
            ),
            (
                "mp_max",
                i64::from(rv.mp_max),
                e.map(|e| i64::from(e.hp_mp_sp().mp_max)),
            ),
            (
                "level",
                i64::from(r.level()),
                e.map(|e| i64::from(e.level())),
            ),
            (
                "equip",
                i64::from_le_bytes(pad8(&r.equipment().slots)),
                e.map(|e| i64::from_le_bytes(pad8(&e.equipment().slots))),
            ),
        ];
        for (name, want, got) in fields {
            total += 1;
            if got == Some(want) {
                equal += 1;
            } else {
                diffs.push(format!("m{i}.{name} retail={want} engine={got:?}"));
            }
        }
    }
    let score = if total == 0 {
        1.0
    } else {
        equal as f64 / total as f64
    };
    let detail = format!(
        "{equal}/{total} fields; retail {} member(s), engine {}",
        retail.members.len(),
        engine.members.len()
    );
    (score, detail, diffs)
}

fn pad8(s: &[u8]) -> [u8; 8] {
    let mut o = [0u8; 8];
    for (d, v) in o.iter_mut().zip(s) {
        *d = *v;
    }
    o
}

fn flags_score(retail: &[u8], engine: &[u8]) -> (f64, String) {
    let n = retail.len().max(engine.len());
    let mut differ = 0u32;
    let mut union = 0u32;
    for i in 0..n {
        let r = retail.get(i).copied().unwrap_or(0);
        let e = engine.get(i).copied().unwrap_or(0);
        differ += (r ^ e).count_ones();
        union += (r | e).count_ones();
    }
    let score = if union == 0 {
        1.0
    } else {
        1.0 - f64::from(differ) / f64::from(union)
    };
    (score, format!("{differ} differing bit(s) of {union} set"))
}

fn inventory_score(
    retail: &legaia_save::SaveFile,
    engine: &legaia_save::SaveFile,
) -> (f64, String) {
    let rs = &retail.ext.item_slots;
    let es = &engine.ext.item_slots;
    let n = rs.len().max(es.len());
    let mut total = 0usize;
    let mut equal = 0usize;
    for i in 0..n {
        let r = rs.get(i).copied().unwrap_or((0, 0));
        let e = es.get(i).copied().unwrap_or((0, 0));
        if r == (0, 0) && e == (0, 0) {
            continue;
        }
        total += 1;
        equal += usize::from(r == e);
    }
    total += 1;
    let gold_ok = retail.ext.money == engine.ext.money;
    equal += usize::from(gold_ok);
    (
        equal as f64 / total as f64,
        format!(
            "{equal}/{total} (slots + gold); gold retail={} engine={}",
            retail.ext.money, engine.ext.money
        ),
    )
}

/// Score one seeded state. `image` is the frame comparison when it ran.
pub fn compare(
    retail: &RetailObs,
    engine: &EngineObs,
    image: Option<ImageScore>,
) -> (BTreeMap<String, f64>, BTreeMap<String, String>) {
    let mut ch = BTreeMap::new();
    let mut det = BTreeMap::new();
    let mut put = |name: &str, score: f64, detail: String| {
        ch.insert(name.to_string(), round3(score));
        det.insert(name.to_string(), detail);
    };

    let scene_ok = engine.scene.as_deref() == Some(retail.scene.as_str());
    put(
        "scene",
        f64::from(u8::from(scene_ok)),
        format!("retail={} engine={:?}", retail.scene, engine.scene),
    );
    let want_mode = match retail.class {
        StateClass::WorldMap => SceneMode::WorldMap,
        _ => SceneMode::Field,
    };
    put(
        "mode",
        f64::from(u8::from(engine.mode == want_mode)),
        format!(
            "retail=0x{:02X} ({:?}) engine={:?}",
            retail.game_mode, retail.class, engine.mode
        ),
    );
    if let (Some(r), Some(e)) = (retail.player, engine.player) {
        let dx = f64::from(i32::from(e[0]) - i32::from(r[0]));
        let dz = f64::from(i32::from(e[2]) - i32::from(r[2]));
        let dist = (dx * dx + dz * dz).sqrt();
        put(
            "position",
            falloff(dist, 4.0, 256.0),
            format!(
                "retail=({}, {}) engine=({}, {}) dist={dist:.1}",
                r[0], r[2], e[0], e[2]
            ),
        );
    }
    if retail.class == StateClass::Field
        && let (Some(r), Some(floor)) = (retail.player, engine.floor_at_retail)
    {
        let d = f64::from(floor - i32::from(r[1]));
        put(
            "footing",
            falloff(d, 2.0, 128.0),
            format!("retail footing={} engine floor={floor}", r[1]),
        );
    }
    {
        let (r, e) = (retail.camera, engine.camera);
        let parts = [
            falloff(angle_delta(r.pitch, e.pitch), 16.0, 256.0),
            falloff(angle_delta(r.yaw, e.yaw), 16.0, 256.0),
            falloff(f64::from(i32::from(r.h) - i32::from(e.h)), 4.0, 128.0),
            falloff(f64::from(r.eye[0] - e.eye[0]), 16.0, 1024.0),
            falloff(f64::from(r.eye[1] - e.eye[1]), 16.0, 1024.0),
            falloff(f64::from(r.eye[2] - e.eye[2]), 16.0, 1024.0),
        ];
        put(
            "camera",
            parts.iter().sum::<f64>() / parts.len() as f64,
            format!(
                "retail pitch/yaw/H={}/{}/{} eye={:?}; engine {}/{}/{} eye={:?}",
                r.pitch, r.yaw, r.h, r.eye, e.pitch, e.yaw, e.h, e.eye
            ),
        );
    }
    put(
        "bgm",
        f64::from(u8::from(engine.bgm_id == Some(retail.bgm_id))),
        format!("retail={} engine={:?}", retail.bgm_id, engine.bgm_id),
    );
    put(
        "fog_gate",
        f64::from(u8::from(engine.fog_gate == retail.fog_gate)),
        format!("retail={} engine={}", retail.fog_gate, engine.fog_gate),
    );
    if let Some(rs) = &retail.save {
        let (s, d, diffs) = party_score(&rs.party, &engine.save.party);
        let d = if diffs.is_empty() {
            d
        } else {
            format!(
                "{d}; {}",
                diffs.iter().take(4).cloned().collect::<Vec<_>>().join(", ")
            )
        };
        put("party", s, d);
        let (s, d) = flags_score(&rs.ext.story_flag_bits, &engine.save.ext.story_flag_bits);
        put("flags", s, d);
        let (s, d) = inventory_score(rs, &engine.save);
        put("inventory", s, d);
    }
    if let Some(img) = image {
        put(
            "image",
            img.within,
            format!("mae={:.1} within={:.3} ({})", img.mae, img.within, img.note),
        );
    }
    (ch, det)
}

/// Mean of the measured channels.
pub fn state_score(channels: &BTreeMap<String, f64>) -> Option<f64> {
    (!channels.is_empty()).then(|| round3(channels.values().sum::<f64>() / channels.len() as f64))
}

/// Corpus-level numbers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CorpusSummary {
    pub states: usize,
    pub seeded: usize,
    pub seed_failed: usize,
    /// Class -> count, over every state.
    pub classes: BTreeMap<String, usize>,
    /// Channel -> `(mean score, states measured)` over seeded states.
    pub channels: BTreeMap<String, (f64, usize)>,
    /// Mean of per-state scores over seeded states.
    pub mean_state_score: f64,
}

pub fn summarise(reports: &[StateReport]) -> CorpusSummary {
    let mut s = CorpusSummary {
        states: reports.len(),
        ..Default::default()
    };
    let mut sums: BTreeMap<String, (f64, usize)> = BTreeMap::new();
    let mut state_sum = 0.0;
    for r in reports {
        *s.classes
            .entry(
                serde_json::to_value(r.class)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default(),
            )
            .or_default() += 1;
        if r.class.seedable() {
            if r.unseeded.is_empty() {
                s.seeded += 1;
            } else {
                s.seed_failed += 1;
            }
        }
        if let Some(sc) = r.score {
            state_sum += sc;
            for (k, v) in &r.channels {
                let e = sums.entry(k.clone()).or_default();
                e.0 += v;
                e.1 += 1;
            }
        }
    }
    s.channels = sums
        .into_iter()
        .map(|(k, (sum, n))| (k, (round3(sum / n as f64), n)))
        .collect();
    s.mean_state_score = if s.seeded == 0 {
        0.0
    } else {
        round3(state_sum / s.seeded as f64)
    };
    s
}

/// Options for one corpus run.
pub struct RunOptions<'a> {
    pub extracted: &'a Path,
    pub library: &'a Path,
    pub manifest: &'a ScenarioManifest,
    /// When set, render each seeded state through `play-window` with this
    /// binary and score the frame.
    pub engine_exe: Option<&'a Path>,
    /// Where side-by-side PNGs and engine captures go (gitignored).
    pub out_dir: Option<&'a Path>,
    /// Only states whose label contains this substring.
    pub filter: Option<&'a str>,
    /// Save application order (see [`SeedOrder`]).
    pub order: SeedOrder,
}

/// Run the whole corpus.
pub fn run_corpus(opts: &RunOptions<'_>) -> Result<Vec<StateReport>> {
    let scus = std::fs::read(opts.extracted.join("SCUS_942.54"))
        .with_context(|| format!("read {}/SCUS_942.54", opts.extracted.display()))?;
    let entries = enumerate_corpus(opts.manifest, opts.library);
    let mut out = Vec::new();
    for entry in entries {
        if let Some(f) = opts.filter
            && !entry.label.contains(f)
        {
            continue;
        }
        out.push(run_one(opts, &entry, &scus));
    }
    Ok(out)
}

fn run_one(opts: &RunOptions<'_>, entry: &CorpusEntry, scus: &[u8]) -> StateReport {
    let mut report = StateReport {
        label: entry.label.clone(),
        emulator: entry.emulator.to_string(),
        fingerprint: entry.fingerprint.chars().take(16).collect(),
        scene: String::new(),
        game_mode: 0,
        class: StateClass::Other,
        unseeded: String::new(),
        channels: BTreeMap::new(),
        detail: BTreeMap::new(),
        score: None,
        image: None,
    };
    let retail = match read_retail(entry, scus) {
        Ok(r) => r,
        Err(e) => {
            report.unseeded = format!("unreadable state: {e:#}");
            return report;
        }
    };
    report.scene = retail.scene.clone();
    report.game_mode = retail.game_mode;
    report.class = retail.class;
    if !retail.class.seedable() {
        report.unseeded = retail.class.unseeded_reason().to_string();
        return report;
    }
    let engine = match run_engine_with(opts.extracted, &retail, opts.order) {
        Ok(e) => e,
        Err(e) => {
            report.unseeded = format!("seeding failed: {e:#}");
            return report;
        }
    };
    let image = match (opts.engine_exe, &retail.frame, retail.player) {
        (Some(_), Some(rf), _)
            if crate::retail_compare_image::luma(rf) < crate::retail_compare_image::DARK_LUMA =>
        {
            report.detail.insert(
                "image".into(),
                "not scored: retail frame is a fade (near-black)".into(),
            );
            None
        }
        (Some(exe), Some(rf), Some([x, _, z])) => {
            let flags = retail
                .save
                .as_ref()
                .map(system_flag_ids)
                .unwrap_or_default();
            match crate::retail_compare_image::engine_frame(
                exe,
                opts.extracted,
                &retail.scene,
                x,
                z,
                opts.out_dir,
                &entry.label,
                &flags,
            ) {
                Ok(ef) => {
                    let score = crate::retail_compare_image::score(rf, &ef);
                    if let Some(dir) = opts.out_dir {
                        let _ = crate::retail_compare_image::write_side_by_side(
                            &dir.join(format!("{}.png", entry.label)),
                            rf,
                            &ef,
                        );
                    }
                    Some(score)
                }
                Err(e) => {
                    report
                        .detail
                        .insert("image".into(), format!("engine frame failed: {e:#}"));
                    None
                }
            }
        }
        _ => None,
    };
    let (ch, det) = compare(&retail, &engine, image.clone());
    report.detail.extend(det);
    report.score = state_score(&ch);
    report.channels = ch;
    report.image = image;
    report
}

/// The committed ratchet: per-state, per-channel scores. No pixels, no RAM.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Baseline {
    /// Engine ticks between seeding and sampling when the baseline was cut.
    pub settle_ticks: u64,
    /// Label -> channel -> score.
    pub states: BTreeMap<String, BTreeMap<String, f64>>,
    /// Label -> class, for the non-seeded states too (a class change is
    /// itself worth a review).
    pub classes: BTreeMap<String, StateClass>,
}

/// Slack for float round-trips through the JSON baseline.
const RATCHET_EPS: f64 = 0.0005;

impl Baseline {
    pub fn from_reports(reports: &[StateReport]) -> Self {
        let mut b = Baseline {
            settle_ticks: SETTLE_TICKS,
            ..Default::default()
        };
        for r in reports {
            b.classes.insert(r.label.clone(), r.class);
            if !r.channels.is_empty() {
                b.states.insert(r.label.clone(), r.channels.clone());
            }
        }
        b
    }

    /// Every drop against this baseline. A channel the run did not measure
    /// is skipped when `allow_unmeasured` names it (the image channel on a
    /// run without a display), and is a failure otherwise.
    pub fn regressions(&self, reports: &[StateReport], allow_unmeasured: &[&str]) -> Vec<String> {
        let by_label: BTreeMap<&str, &StateReport> =
            reports.iter().map(|r| (r.label.as_str(), r)).collect();
        let mut out = Vec::new();
        for (label, chans) in &self.states {
            // A state absent from this run is a property of the local
            // library (backups are gitignored and per-machine), not of the
            // engine, so it is reported by the caller rather than failed.
            let Some(r) = by_label.get(label.as_str()) else {
                continue;
            };
            for (ch, &want) in chans {
                match r.channels.get(ch) {
                    Some(&got) if got + RATCHET_EPS < want => {
                        out.push(format!("{label}.{ch}: {got:.3} < baseline {want:.3}"))
                    }
                    Some(_) => {}
                    None if allow_unmeasured.contains(&ch.as_str()) => {}
                    None => out.push(format!("{label}.{ch}: not measured (baseline {want:.3})")),
                }
            }
        }
        out
    }
}

/// Render the human report as markdown.
pub fn markdown_report(reports: &[StateReport], summary: &CorpusSummary) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(s, "# Retail comparison corpus\n");
    let _ = writeln!(
        s,
        "{} states; {} seeded; {} seed failures; mean state score {:.3}\n",
        summary.states, summary.seeded, summary.seed_failed, summary.mean_state_score
    );
    let _ = writeln!(s, "## Channels (mean over seeded states)\n");
    let _ = writeln!(s, "| channel | mean | measured |\n|---|---|---|");
    for ch in CHANNELS {
        if let Some((m, n)) = summary.channels.get(*ch) {
            let _ = writeln!(s, "| {ch} | {m:.3} | {n} |");
        }
    }
    let _ = writeln!(s, "\n## Classes\n");
    for (k, v) in &summary.classes {
        let _ = writeln!(s, "- {k}: {v}");
    }
    let mut ranked: Vec<&StateReport> = reports.iter().filter(|r| r.score.is_some()).collect();
    ranked.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let _ = writeln!(s, "\n## Seeded states, worst first\n");
    for r in ranked {
        let _ = writeln!(
            s,
            "### {} - {:.3} ({} {} 0x{:02X})\n",
            r.label,
            r.score.unwrap_or(0.0),
            r.scene,
            r.emulator,
            r.game_mode
        );
        for ch in CHANNELS {
            if let Some(v) = r.channels.get(*ch) {
                let d = r.detail.get(*ch).map(String::as_str).unwrap_or("");
                let _ = writeln!(s, "- `{ch}` {v:.3} - {d}");
            }
        }
        if r.channels.contains_key("image") {
            let _ = writeln!(s, "\n![{0}]({0}.png)", r.label);
        }
        let _ = writeln!(s);
    }
    let _ = writeln!(s, "## Not seeded\n");
    for r in reports.iter().filter(|r| r.score.is_none()) {
        let _ = writeln!(
            s,
            "- {} ({} 0x{:02X} {:?}): {}",
            r.label, r.scene, r.game_mode, r.class, r.unseeded
        );
    }
    s
}

/// Resolve the library + extracted dirs: env overrides first
/// (`LEGAIA_SAVES_LIBRARY`, `LEGAIA_EXTRACTED_DIR`), then repo-relative.
pub fn resolve_dirs() -> (Option<PathBuf>, Option<PathBuf>, Option<PathBuf>) {
    let pick = |env: &str, rels: &[&str], ok: &dyn Fn(&Path) -> bool| -> Option<PathBuf> {
        if let Some(v) = std::env::var_os(env) {
            let p = PathBuf::from(v);
            return ok(&p).then_some(p);
        }
        rels.iter().map(PathBuf::from).find(|p| ok(p))
    };
    let library = pick(
        "LEGAIA_SAVES_LIBRARY",
        &["saves/library", "../../saves/library"],
        &|p| p.is_dir(),
    );
    let extracted = pick(
        "LEGAIA_EXTRACTED_DIR",
        &["extracted", "../extracted", "../../extracted"],
        &|p| p.join("PROT.DAT").exists() && p.join("CDNAME.TXT").exists(),
    );
    let manifest = ["scripts/scenarios.toml", "../../scripts/scenarios.toml"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.exists());
    (manifest, library, extracted)
}
