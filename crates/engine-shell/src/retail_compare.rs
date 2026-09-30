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
//! Walkable states (field-run in a field scene or on a kingdom overworld)
//! and battle states are seedable; the battle half - reading the encounter
//! out of RAM and entering it through the engine's own encounter path - is
//! [`crate::retail_compare_battle`]. Every other class is catalogued with
//! its retail observables and a reason, so the corpus summary counts what
//! the instrument cannot reach instead of hiding it.

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
/// The camera focus pair `_DAT_80089118` / `_DAT_80089120` - the world X / Z
/// the view orbits, stored **negated** (`engine-core::camera`, axes 6 / 8).
const CAM_FOCUS: u32 = 0x8008_9118;
const BGM_ID: u32 = 0x8007_BAC8;
/// The field BGM sound-source slot (`docs/subsystems/audio.md`).
const BGM_SLOT: u32 = 0x8007_052C;
/// `0x8007B708`: `1` after the slot's replay, `0` after a stop / pause.
const BGM_PLAYING: u32 = 0x8007_B708;
/// `0x80084540`, the **loaded** scene's raw CDNAME define. The label at
/// `0x8007050C` is written by the scene-change packet ahead of the load, so
/// between a door and the next field init the two disagree and this one
/// names the scene still running (`docs/tooling/retail-compare.md`).
const LOADED_SCENE_DEFINE: u32 = 0x8008_4540;
/// The ambient-particle (fog pool) master gate, raised / cleared only by
/// field-VM op `0x4C` nibble 3 (`docs/subsystems/field-ambient-fx.md`).
const FOG_GATE: u32 = 0x8007_B854;
/// `_DAT_801F348C`, the field party HUD's idle countdown (`FUN_801D0D38`).
const HUD_COUNTDOWN: u32 = 0x801F_348C;
/// `DAT_801E46A4`, the menu overlay's current sub-screen id
/// (`docs/subsystems/save-screen.md`).
const MENU_SUBSCREEN: u32 = 0x801E_46A4;
/// The Equip row's three retail steps: the character picker (`0x12`,
/// `FUN_801D98F0` - the id the root row routes to), the slot browse
/// (`0x13`, `FUN_801D99F0`) and the candidate list (`0x14`, `FUN_801D9C14`).
const MENU_EQUIP_PICK: u8 = 0x12;
const MENU_EQUIP_SLOTS: u8 = 0x13;
const MENU_EQUIP_CANDIDATES: u8 = 0x14;

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
        matches!(
            self,
            StateClass::Field | StateClass::WorldMap | StateClass::Battle | StateClass::Menu
        )
    }

    /// The reason a non-seedable class is not seeded.
    pub fn unseeded_reason(self) -> &'static str {
        match self {
            StateClass::Field | StateClass::WorldMap | StateClass::Battle | StateClass::Menu => "",
            StateClass::FieldInit => "scene mid-load; no settled frame to reproduce",
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
    /// The focus X / Z words as retail stores them (negated world X / Z).
    /// Without them a frame aimed at the wrong place scores its camera whole:
    /// rotation and eye are all relative to the focus.
    pub focus: [i32; 2],
}

/// Everything read off one retail state.
pub struct RetailObs {
    pub scene: String,
    /// `0x80084540`: the raw CDNAME define of the scene actually loaded.
    pub loaded_define: u16,
    /// The label a scene-change packet had already written when the state
    /// was taken, when it names a scene other than the loaded one
    /// ([`RetailObs::settle_on_loaded_scene`]).
    pub pending_scene: Option<String>,
    pub game_mode: u8,
    pub class: StateClass,
    /// `(X, footing, Z)`.
    pub player: Option<[i16; 3]>,
    pub camera: CameraObs,
    pub bgm_id: u16,
    /// Whether the field BGM slot `0x8007052C` is attached and at a non-zero
    /// volume: the playing word `0x8007B708` (raised by the replay primitive
    /// `FUN_80026478`, cleared by the stop / pause / timed-release arms) and
    /// the slot's `SsSeqSetVol` word `+0x6` (`FUN_8002657C`, zeroed by the
    /// same arms and by the battle intro's field-voice stop).
    pub bgm_sounding: bool,
    /// `_DAT_8007B854 != 0`.
    pub fog_gate: bool,
    /// The live game-state window lifted as a save.
    pub save: Option<legaia_save::SaveFile>,
    /// The field party HUD's idle countdown `_DAT_801F348C` (field-overlay
    /// data, so read on a [`StateClass::Field`] state only). The image
    /// channel lands the engine's countdown on it at the capture tick, so
    /// the readout is up in the engine frame exactly when it is in retail's.
    pub hud_countdown: Option<i16>,
    /// The displayed frame, when the state carries VRAM + display registers.
    pub frame: Option<Frame>,
    /// Battle observables for a battle-class state; `Err` names why the
    /// state cannot seed a battle.
    pub battle: Option<std::result::Result<crate::retail_compare_battle::RetailBattle, String>>,
    /// The pause-menu screen a menu-class state shows; `Err` names why the
    /// state is not one the seed can drive to.
    pub menu: Option<std::result::Result<RetailMenu, String>>,
    /// The field-VM contexts the state holds
    /// ([`crate::retail_compare_script`]).
    pub scripts: crate::retail_compare_script::RetailScripts,
}

/// A menu-class capture the seed can reproduce: a pause-menu screen, named
/// by the root row whose confirm opens it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailMenu {
    /// `DAT_801E46A4`.
    pub subscreen: u8,
    /// The root row whose confirm reaches it.
    pub row: legaia_engine_core::field_menu::FieldMenuRow,
    /// For an Equip-row screen, how many confirms past the root row it sits
    /// (`0` character picker, `1` slot browse, `2` candidate list).
    pub equip_depth: u8,
}

impl RetailMenu {
    /// Classify a menu-class capture's sub-screen id. The pause menu runs
    /// over a walkable scene with the id set; the title / boot family (the
    /// attract loop, the title picker, the card-boot save select) holds it
    /// clear, and a script-entered screen (the casino prize exchange,
    /// `0x20`) is no root row's.
    fn from_ram(ram: &[u8], scene: &str) -> std::result::Result<Self, String> {
        use legaia_engine_core::field_menu::FieldMenuRow;
        let subscreen = game_anchors::u8_at(ram, MENU_SUBSCREEN);
        if subscreen == 0 {
            return Err(format!(
                "title / boot screen on {scene} (sub-screen 0x00); no title seeding path"
            ));
        }
        let (row, equip_depth) = match subscreen {
            MENU_EQUIP_PICK => (FieldMenuRow::Equip, 0),
            MENU_EQUIP_SLOTS => (FieldMenuRow::Equip, 1),
            MENU_EQUIP_CANDIDATES => (FieldMenuRow::Equip, 2),
            _ => {
                let row = FieldMenuRow::from_retail_subscreen(subscreen).ok_or_else(|| {
                    format!("sub-screen 0x{subscreen:02X} is no pause-menu row's (script-entered)")
                })?;
                (row, 0)
            }
        };
        Ok(Self {
            subscreen,
            row,
            equip_depth,
        })
    }
}

/// Prefix of the reason a menu-class state carries when it is not a
/// pause-menu screen the seed can drive to.
pub const MENU_NOT_SEEDABLE: &str = "menu not seedable: ";

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
        let menu = (class == StateClass::Menu).then(|| RetailMenu::from_ram(ram, &scene));
        let camera = CameraObs {
            pitch: rd16(ram, CAM_ROT),
            yaw: rd16(ram, CAM_ROT + 2),
            h: rd16(ram, GTE_H),
            eye: [
                rd32(ram, CAM_EYE),
                rd32(ram, CAM_EYE + 4),
                rd32(ram, CAM_EYE + 8),
            ],
            focus: [rd32(ram, CAM_FOCUS), rd32(ram, CAM_FOCUS + 8)],
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
            loaded_define: game_anchors::u16_at(ram, LOADED_SCENE_DEFINE),
            pending_scene: None,
            game_mode,
            class,
            player,
            camera,
            bgm_id,
            bgm_sounding: game_anchors::u16_at(ram, BGM_PLAYING) != 0
                && game_anchors::u16_at(ram, BGM_SLOT + 6) != 0,
            fog_gate,
            save,
            hud_countdown: (class == StateClass::Field).then(|| rd16(ram, HUD_COUNTDOWN)),
            frame,
            battle: (class == StateClass::Battle)
                .then(|| crate::retail_compare_battle::RetailBattle::from_ram(ram)),
            menu,
            scripts: crate::retail_compare_script::RetailScripts::from_ram(ram),
        }
    }
}

impl RetailObs {
    /// Name the state by the scene it is **running**, not the one a door has
    /// queued. A walked crossing writes the destination label to
    /// `0x8007050C` with the scene-change packet, frames before the field
    /// init loads the block and stores its define to `0x80084540`; a
    /// field-run capture in that window shows the outgoing scene (its frame,
    /// its camera, its player, its track) under the incoming label. Scored
    /// under the label, every one of those channels compares the outgoing
    /// scene's retail values against a fresh entry of the incoming one.
    ///
    /// Only field-run states are re-named: a mode-`0x02` capture is the load
    /// itself, and the title / battle / menu modes hold other words there.
    pub fn settle_on_loaded_scene(&mut self, cdname: &legaia_prot::cdname::IndexMap) {
        if !matches!(self.class, StateClass::Field | StateClass::WorldMap) {
            return;
        }
        let label_define = cdname
            .iter()
            .find(|(_, name)| **name == self.scene)
            .map(|(&define, _)| define);
        let Some(loaded) = cdname.get(&u32::from(self.loaded_define)) else {
            return;
        };
        if label_define == Some(u32::from(self.loaded_define)) || *loaded == self.scene {
            return;
        }
        let pending = std::mem::replace(&mut self.scene, loaded.clone());
        self.pending_scene = Some(pending);
        self.class = StateClass::classify(self.game_mode, &self.scene, self.player.is_some());
        if self.class != StateClass::Field {
            self.hud_countdown = None;
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
    /// A control op stopped or paused the track after its last start.
    pub bgm_held: bool,
    /// The engine's fog-pool gate (`World::fog.gate`).
    pub fog_gate: bool,
    pub save: legaia_save::SaveFile,
    /// On a menu-class seed: the sub-screen the engine's pause menu reached
    /// (the open row's retail id, `0x01` on the root list, `0x13` for the
    /// Equip candidate step), `None` when no menu opened.
    pub menu_subscreen: Option<u8>,
    /// On a capture inside a running script: the phase gate's outcome.
    pub script: Option<ScriptPhase>,
}

/// Records which track the field VM starts.
#[derive(Default)]
pub(crate) struct RecordingDirector {
    pub(crate) last: Option<u16>,
    /// A control op has stopped or paused the track since the last start.
    pub(crate) held: bool,
}

impl BgmDirector for RecordingDirector {
    fn start(&mut self, bgm_id: u16, _seq: &[u8]) {
        self.last = Some(bgm_id);
        self.held = false;
    }
    fn start_owned_vab(&mut self, bgm_id: u16, _entry: &[u8]) {
        self.last = Some(bgm_id);
        self.held = false;
    }
    // The comparand is the id the scripts selected (retail's track-select
    // word is written by the op-0x35 start arms). A control op starts no
    // track, so it keeps `last` and only moves `held`.
    fn pause(&mut self) {
        self.held = true;
    }
    fn resume(&mut self) {
        self.held = false;
    }
    fn stop(&mut self) {
        self.held = true;
    }
    fn unhalt_pause(&mut self) {
        self.held = false;
    }
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
        && session.host.debug_seat_standing(x, z)
    {
        session.camera.zone.arm_arrival();
    }
    // A capture inside a running script is compared at the script's phase,
    // not after a fixed window ([`crate::retail_compare_script::ScriptGate`]):
    // the session runs until its own context for the record holds retail's
    // PC and wait, up to a deadline. The settle-window sample is kept as the
    // fallback for a gate the engine never meets.
    let gate = (retail.menu.is_none()
        && matches!(retail.class, StateClass::Field | StateClass::WorldMap))
    .then(|| crate::retail_compare_script::ScriptGate::from_retail(&retail.scripts))
    .flatten();
    let deadline = if gate.is_some() {
        crate::retail_compare_script::SCRIPT_GATE_DEADLINE
    } else {
        SETTLE_TICKS
    };
    let mut at_settle = None;
    let mut met_at = None;
    let mut resumed = false;
    for t in 1..=deadline {
        if let Some(g) = &gate {
            let pad = g.advance_pad(&session.host.world, t);
            session.host.world.input.set_pad(pad);
        }
        session.tick()?;
        session.host.route_bgm_events(&mut director)?;
        if let Some(g) = &gate {
            if std::env::var_os("LEGAIA_RC_SCRIPT_TRACE").is_some() && (t % 25 == 0 || t < 5) {
                eprintln!("script gate t={t}: {}", g.trace(&session.host.world));
            }
            if g.met(&session.host.world) {
                met_at = Some(t);
                break;
            }
            if t == SETTLE_TICKS {
                at_settle = Some(sample_engine(&mut session, retail, &director, None));
            }
            if t == crate::retail_compare_script::SCRIPT_RESUME_TICK
                && crate::retail_compare_script::resume_record(&mut session.host, g)
            {
                resumed = true;
            }
        }
    }
    session.host.world.input.set_pad(0);
    let script = gate.as_ref().map(|g| ScriptPhase {
        pc: g.pc,
        wait: g.wait,
        met_at,
        resumed,
    });
    if let (Some(_), None, Some(mut obs)) = (&gate, met_at, at_settle) {
        obs.script = script;
        return Ok(obs);
    }
    let menu_subscreen = match &retail.menu {
        Some(Ok(menu)) => drive_pause_menu(&mut session, menu)?,
        _ => None,
    };
    let mut obs = sample_engine(&mut session, retail, &director, menu_subscreen);
    obs.script = script;
    Ok(obs)
}

/// How a script-gated seed went: retail's phase and the tick the engine
/// reached it (`None`: not within the deadline, so the settle-window sample
/// stands).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptPhase {
    pub pc: usize,
    pub wait: i16,
    pub met_at: Option<u64>,
    /// The record was not running in the engine and was started from its
    /// first opcode ([`crate::retail_compare_script::resume_record`]).
    pub resumed: bool,
}

fn sample_engine(
    session: &mut BootSession,
    retail: &RetailObs,
    director: &RecordingDirector,
    menu_subscreen: Option<u8>,
) -> EngineObs {
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
        focus: [g[6], g[8]],
    };
    let mode = world.mode;
    let fog_gate = world.fog.gate;
    let save = world.save_full();
    let scene = session.host.scene.as_ref().map(|s| s.name.clone());
    // The engine's `_DAT_8007BAC8`: a park-sentinel start (`0x1000`, the
    // ending scenes') reaches no director, but it is the word retail holds.
    let bgm_id = session.host.bgm_track_word.or(director.last);
    EngineObs {
        scene,
        mode,
        player,
        floor_at_retail,
        camera,
        bgm_id,
        bgm_held: director.held,
        fog_gate,
        save,
        menu_subscreen,
        script: None,
    }
}

/// Ticks between two scripted pad edges of a menu drive: long enough for
/// the menu's open / hand-off beats, which swallow the edge that caused them.
pub const MENU_PRESS_GAP: u64 = 12;

/// The pad edges that reach `menu` from a settled field, as `(tick offset,
/// button)` pairs from the first press: `Start`, `Down` until the root
/// cursor (opening on row `0`) sits on the row, `Cross`. The Equip row opens
/// on its character picker (`0x12`); the slot browse (`0x13`) is one more
/// `Cross`, and the candidate list (`0x14`) a `Down` past Best Equipment and
/// a `Cross` after that.
pub fn pause_menu_presses(menu: &RetailMenu) -> Vec<(u64, legaia_engine_core::input::PadButton)> {
    use legaia_engine_core::input::PadButton;
    let mut out = vec![(0, PadButton::Start)];
    let mut t = 0;
    let mut press = |b: PadButton| {
        t += MENU_PRESS_GAP;
        out.push((t, b));
    };
    for _ in 0..menu.row.index() {
        press(PadButton::Down);
    }
    press(PadButton::Cross);
    if menu.equip_depth >= 1 {
        press(PadButton::Cross);
    }
    if menu.equip_depth >= 2 {
        press(PadButton::Down);
        press(PadButton::Cross);
    }
    out
}

/// Ticks a menu drive runs from its first press to the sample.
pub fn pause_menu_drive_ticks(menu: &RetailMenu) -> u64 {
    pause_menu_presses(menu).last().map_or(0, |p| p.0) + 2 * MENU_PRESS_GAP
}

/// Drive the headless session's pause menu to `menu` through its pad path
/// ([`pause_menu_presses`], each a one-tick edge), settle, and read back the
/// sub-screen it holds.
fn drive_pause_menu(session: &mut BootSession, menu: &RetailMenu) -> Result<Option<u8>> {
    use legaia_engine_core::equip_session::EquipState;
    use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
    let presses = pause_menu_presses(menu);
    for t in 0..=pause_menu_drive_ticks(menu) {
        let mask = presses
            .iter()
            .filter(|p| p.0 == t)
            .fold(0u16, |m, p| m | p.1.mask());
        session.host.world.input.set_pad(mask);
        session.tick()?;
    }
    session.host.world.input.set_pad(0);
    Ok(match (&session.field_menu, &session.field_menu_sub) {
        (None, _) => None,
        (Some(_), None) => Some(0x01),
        // The Equip row's three steps: the character picker, the slot
        // browse and the candidate list.
        (Some(_), Some(sub)) => Some(match sub {
            FieldMenuSubsession::Equip { picking: true, .. } => MENU_EQUIP_PICK,
            FieldMenuSubsession::Equip { session, .. } => match session.state() {
                EquipState::SlotPicker { .. } => MENU_EQUIP_SLOTS,
                _ => MENU_EQUIP_CANDIDATES,
            },
            other => other.row().retail_subscreen(),
        }),
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
    "enemies",
    "enemy_hp",
    "battle_party",
    "phase",
    "menu",
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

pub(crate) fn round3(v: f64) -> f64 {
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

pub(crate) fn flags_score(retail: &[u8], engine: &[u8]) -> (f64, String) {
    let n = retail.len().max(engine.len());
    let mut differ = 0u32;
    let mut union = 0u32;
    // The first few differing bits, named: a system flag by its id (the
    // bank at `+0x158`, MSB-first), anything below it by byte and mask;
    // `+` is set on the engine side only, `-` on retail's only.
    let mut named = Vec::new();
    for i in 0..n {
        let r = retail.get(i).copied().unwrap_or(0);
        let e = engine.get(i).copied().unwrap_or(0);
        differ += (r ^ e).count_ones();
        union += (r | e).count_ones();
        for bit in 0..8u8 {
            let m = 0x80u8 >> bit;
            if (r ^ e) & m != 0 && named.len() < 4 {
                let sign = if e & m != 0 { '+' } else { '-' };
                named.push(match i.checked_sub(SYSTEM_FLAG_WINDOW) {
                    Some(k) => format!("{sign}sys 0x{:03X}", k * 8 + usize::from(bit)),
                    None => format!("{sign}[0x{i:03X}]&0x{m:02X}"),
                });
            }
        }
    }
    let score = if union == 0 {
        1.0
    } else {
        1.0 - f64::from(differ) / f64::from(union)
    };
    let names = if named.is_empty() {
        String::new()
    } else {
        format!(" ({})", named.join(", "))
    };
    (
        score,
        format!("{differ} differing bit(s) of {union} set{names}"),
    )
}

pub(crate) fn inventory_score(
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

/// The camera channel: mean of pitch, yaw (wrapped), `H` and the three eye
/// words, each on its own falloff.
pub(crate) fn camera_score(r: &CameraObs, e: &CameraObs) -> (f64, String) {
    let parts = [
        falloff(angle_delta(r.pitch, e.pitch), 16.0, 256.0),
        falloff(angle_delta(r.yaw, e.yaw), 16.0, 256.0),
        falloff(f64::from(i32::from(r.h) - i32::from(e.h)), 4.0, 128.0),
        falloff(f64::from(r.eye[0] - e.eye[0]), 16.0, 1024.0),
        falloff(f64::from(r.eye[1] - e.eye[1]), 16.0, 1024.0),
        falloff(f64::from(r.eye[2] - e.eye[2]), 16.0, 1024.0),
        falloff(f64::from(r.focus[0] - e.focus[0]), 16.0, 1024.0),
        falloff(f64::from(r.focus[1] - e.focus[1]), 16.0, 1024.0),
    ];
    // Focus prints as world X / Z (the stored words are negated).
    let world = |f: [i32; 2]| [-f[0], -f[1]];
    (
        parts.iter().sum::<f64>() / parts.len() as f64,
        format!(
            "retail pitch/yaw/H={}/{}/{} eye={:?} focus={:?}; engine {}/{}/{} eye={:?} focus={:?}",
            r.pitch,
            r.yaw,
            r.h,
            r.eye,
            world(r.focus),
            e.pitch,
            e.yaw,
            e.h,
            e.eye,
            world(e.focus)
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
        StateClass::Menu => SceneMode::Menu,
        _ => SceneMode::Field,
    };
    if let Some(Ok(menu)) = &retail.menu {
        put(
            "menu",
            f64::from(u8::from(engine.menu_subscreen == Some(menu.subscreen))),
            format!(
                "retail sub-screen=0x{:02X} ({:?}{}) engine={:?}",
                menu.subscreen,
                menu.row,
                match (menu.row, menu.equip_depth) {
                    (legaia_engine_core::field_menu::FieldMenuRow::Equip, 0) => {
                        ", character picker"
                    }
                    (_, 1) => ", slot browse",
                    (_, 2) => ", candidate list",
                    _ => "",
                },
                engine.menu_subscreen.map(|s| format!("0x{s:02X}"))
            ),
        );
    }
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
    let (s, d) = camera_score(&retail.camera, &engine.camera);
    put("camera", s, d);
    put(
        "bgm",
        f64::from(u8::from(engine.bgm_id == Some(retail.bgm_id))),
        format!(
            "retail={}{} engine={:?}{}",
            retail.bgm_id,
            if retail.bgm_sounding { "" } else { " (held)" },
            engine.bgm_id,
            if engine.bgm_held { " (held)" } else { "" },
        ),
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
    if let Some(p) = engine.script {
        det.insert(
            "script".into(),
            match p.met_at {
                Some(t) => format!(
                    "retail parked at pc {} wait {}; engine reached it at tick {t}{}",
                    p.pc,
                    p.wait,
                    if p.resumed {
                        " (record resumed from its start)"
                    } else {
                        ""
                    }
                ),
                None => format!(
                    "retail parked at pc {} wait {}; engine did not reach it in {} ticks (sampled at the settle window)",
                    p.pc,
                    p.wait,
                    crate::retail_compare_script::SCRIPT_GATE_DEADLINE
                ),
            },
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
            } else if r.unseeded.starts_with(BATTLE_NOT_SEEDABLE)
                || r.unseeded.starts_with(MENU_NOT_SEEDABLE)
            {
                // A battle capture whose RAM names why it cannot be seeded
                // (the fight still loading, an empty cell) is a classified
                // instrument limit, not a failure of the seeding path.
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
    let cdname = legaia_prot::cdname::parse(&opts.extracted.join("CDNAME.TXT"))?;
    let entries = enumerate_corpus(opts.manifest, opts.library);
    let mut out = Vec::new();
    for entry in entries {
        if let Some(f) = opts.filter
            && !entry.label.contains(f)
        {
            continue;
        }
        out.push(run_one(opts, &entry, &scus, &cdname));
    }
    Ok(out)
}

fn run_one(
    opts: &RunOptions<'_>,
    entry: &CorpusEntry,
    scus: &[u8],
    cdname: &legaia_prot::cdname::IndexMap,
) -> StateReport {
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
    let mut retail = match read_retail(entry, scus) {
        Ok(r) => r,
        Err(e) => {
            report.unseeded = format!("unreadable state: {e:#}");
            return report;
        }
    };
    retail.settle_on_loaded_scene(cdname);
    if let Some(pending) = &retail.pending_scene {
        report.detail.insert(
            "pending_scene".into(),
            format!(
                "label reads {pending} but define {} ({}) is loaded; scored as {}",
                retail.loaded_define, retail.scene, retail.scene
            ),
        );
    }
    report.scene = retail.scene.clone();
    report.game_mode = retail.game_mode;
    report.class = retail.class;
    if !retail.class.seedable() {
        report.unseeded = retail.class.unseeded_reason().to_string();
        return report;
    }
    if retail.class == StateClass::Battle {
        run_battle(opts, entry, &retail, &mut report);
        return report;
    }
    if let Some(Err(why)) = &retail.menu {
        report.unseeded = format!("{MENU_NOT_SEEDABLE}{why}");
        return report;
    }
    let engine = match run_engine_with(opts.extracted, &retail, opts.order) {
        Ok(e) => e,
        Err(e) => {
            report.unseeded = format!("seeding failed: {e:#}");
            return report;
        }
    };
    let image = match (
        opts.engine_exe,
        &retail.frame,
        retail.player,
        retail.save.as_ref(),
    ) {
        (Some(exe), Some(rf), seat, Some(save))
            if matches!(retail.menu, Some(Ok(_)))
                && crate::retail_compare_image::luma(rf)
                    >= crate::retail_compare_image::DARK_LUMA =>
        {
            let Some(Ok(menu)) = &retail.menu else {
                unreachable!()
            };
            menu_image(opts, exe, entry, &retail, menu, seat, save, rf, &mut report)
        }
        (Some(_), Some(rf), _, _)
            if crate::retail_compare_image::luma(rf) < crate::retail_compare_image::DARK_LUMA =>
        {
            report.detail.insert(
                "image".into(),
                "not scored: retail frame is a fade (near-black)".into(),
            );
            None
        }
        (Some(exe), Some(rf), Some([x, _, z]), Some(save)) => {
            // A capture the headless seed reached by its script phase is
            // framed at that phase too: the child runs the same gate and
            // captures the frame it holds, with the deadline as its bound.
            let gate = engine.script.filter(|p| p.met_at.is_some()).and(
                crate::retail_compare_script::ScriptGate::from_retail(&retail.scripts),
            );
            let frame = match gate {
                Some(g) => {
                    let mut env = vec![("LEGAIA_SCRIPT_GATE", g.to_env())];
                    if let Some(n) = retail.hud_countdown {
                        env.push(("LEGAIA_HUD_COUNTDOWN", n.to_string()));
                    }
                    crate::retail_compare_image::engine_frame_with(
                        exe,
                        opts.extracted,
                        &retail.scene,
                        Some((x, z)),
                        &[],
                        &env,
                        crate::retail_compare_script::SCRIPT_GATE_DEADLINE,
                        opts.out_dir,
                        &entry.label,
                        crate::retail_compare_image::FrameEntry::Resume(save),
                    )
                }
                None => crate::retail_compare_image::engine_frame(
                    exe,
                    opts.extracted,
                    &retail.scene,
                    x,
                    z,
                    opts.out_dir,
                    &entry.label,
                    save,
                    retail.hud_countdown,
                ),
            };
            match frame {
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

/// The pause-menu frame through `play-window`: the card-load resume and seat
/// a field state takes, then the same pad edges the headless drive presses
/// ([`pause_menu_presses`]) as a `--pad-script`, captured once the drive has
/// settled.
#[allow(clippy::too_many_arguments)]
fn menu_image(
    opts: &RunOptions<'_>,
    exe: &Path,
    entry: &CorpusEntry,
    retail: &RetailObs,
    menu: &RetailMenu,
    seat: Option<[i16; 3]>,
    save: &legaia_save::SaveFile,
    rf: &Frame,
    report: &mut StateReport,
) -> Option<ImageScore> {
    let first = crate::retail_compare_image::CAPTURE_TICK;
    let script = pause_menu_presses(menu)
        .iter()
        .map(|(t, b)| format!("{}:{b:?}", first + t))
        .collect::<Vec<_>>()
        .join(",");
    let extra = vec!["--pad-script".to_string(), script];
    match crate::retail_compare_image::engine_frame_with(
        exe,
        opts.extracted,
        &retail.scene,
        seat.map(|[x, _, z]| (x, z)),
        &extra,
        &[],
        first + pause_menu_drive_ticks(menu),
        opts.out_dir,
        &entry.label,
        crate::retail_compare_image::FrameEntry::Resume(save),
    ) {
        Ok(ef) => {
            if let Some(dir) = opts.out_dir {
                let _ = crate::retail_compare_image::write_side_by_side(
                    &dir.join(format!("{}.png", entry.label)),
                    rf,
                    &ef,
                );
            }
            Some(crate::retail_compare_image::score(rf, &ef))
        }
        Err(e) => {
            report
                .detail
                .insert("image".into(), format!("engine frame failed: {e:#}"));
            None
        }
    }
}

/// Prefix of the reason a battle-class state carries when its RAM does not
/// describe a seedable fight.
pub const BATTLE_NOT_SEEDABLE: &str = "battle not seedable: ";

fn run_battle(
    opts: &RunOptions<'_>,
    entry: &CorpusEntry,
    retail: &RetailObs,
    report: &mut StateReport,
) {
    let battle = match &retail.battle {
        Some(Ok(b)) => b,
        Some(Err(why)) => {
            report.unseeded = format!("{BATTLE_NOT_SEEDABLE}{why}");
            return;
        }
        None => {
            report.unseeded = "battle observables not read".into();
            return;
        }
    };
    let engine =
        match crate::retail_compare_battle::run_engine_battle(opts.extracted, retail, battle) {
            Ok(e) => e,
            Err(e) => {
                report.unseeded = format!("seeding failed: {e:#}");
                return;
            }
        };
    let image = battle_image(opts, entry, retail, battle, &engine, report);
    let (mut ch, mut det) = crate::retail_compare_battle::compare_battle(retail, battle, &engine);
    if let Some(img) = &image {
        ch.insert("image".into(), round3(img.within));
        det.insert(
            "image".into(),
            format!("mae={:.1} within={:.3} ({})", img.mae, img.within, img.note),
        );
    }
    report.image = image;
    report.detail.extend(det);
    report.score = state_score(&ch);
    report.channels = ch;
}

/// The battle frame through `play-window --battle`, when the fight has a MAN
/// row to name and the retail frame is not a fade.
fn battle_image(
    opts: &RunOptions<'_>,
    entry: &CorpusEntry,
    retail: &RetailObs,
    battle: &crate::retail_compare_battle::RetailBattle,
    engine: &crate::retail_compare_battle::EngineBattle,
    report: &mut StateReport,
) -> Option<ImageScore> {
    let (exe, rf) = (opts.engine_exe?, retail.frame.as_ref()?);
    if crate::retail_compare_image::luma(rf) < crate::retail_compare_image::DARK_LUMA {
        report.detail.insert(
            "image".into(),
            "not scored: retail frame is a fade (near-black)".into(),
        );
        return None;
    }
    let Some(row) = engine.man_row else {
        report.detail.insert(
            "image".into(),
            "not scored: formation has no MAN row for `play-window --battle`".into(),
        );
        return None;
    };
    let flags = retail
        .save
        .as_ref()
        .map(system_flag_ids)
        .unwrap_or_default();
    let extra = crate::retail_compare_battle::play_window_args(battle, row);
    let mut env = vec![("LEGAIA_BATTLE_STAGE", battle.stage_variant.to_string())];
    // The idle orbit is a clock: phase-align it to the retail instant when
    // retail's own orbit owns the yaw (the battle tick's prologue store,
    // gated on these command-flow bytes - `0x801D07AC..0x801D07CC`).
    if crate::retail_compare_battle::ORBIT_FLOWS.contains(&battle.flow) {
        env.push(("LEGAIA_BATTLE_ORBIT_YAW", retail.camera.yaw.to_string()));
    }
    // A capture taken mid-cast replays its cast and is captured on its phase
    // (the gate), with the fixed tick as the deadline.
    let mut tick = crate::retail_compare_battle::BATTLE_CAPTURE_TICK
        + u64::from(engine.prompt_tick.unwrap_or(0));
    if let (Some(seed), Some(gate)) = (battle.inflight_cast(), battle.display_phase_gate()) {
        if engine.inflight == Some(None) {
            report.detail.insert(
                "image".into(),
                "not scored: the replayed cast never reached the capture's phase headlessly".into(),
            );
            return None;
        }
        let ground: Vec<String> = seed
            .ground
            .iter()
            .map(|g| g.map_or_else(|| "-".to_string(), |[x, z]| format!("{x}:{z}")))
            .collect();
        env.push((
            "LEGAIA_BATTLE_INFLIGHT",
            format!(
                "{},{},{};{}",
                seed.caster,
                seed.spell_id,
                seed.target,
                ground.join(",")
            ),
        ));
        env.push(("LEGAIA_CAPTURE_GATE", gate.to_env()));
        tick += crate::retail_compare_battle::INFLIGHT_DEADLINE;
    }
    match crate::retail_compare_image::engine_frame_with(
        exe,
        opts.extracted,
        &retail.scene,
        None,
        &extra,
        &env,
        tick,
        opts.out_dir,
        &entry.label,
        // The card-load resume the headless side seeds with, so the frame's
        // party is retail's (levels, equipment, HP / MP on the HUD, the
        // assembled battle meshes) rather than the New Game template the
        // bare door entry seeds. The door with the system flags stays the
        // fallback for a state whose save window does not lift.
        match retail.save.as_ref() {
            Some(save) => crate::retail_compare_image::FrameEntry::Resume(save),
            None => crate::retail_compare_image::FrameEntry::Door(&flags),
        },
    ) {
        Ok(ef) => {
            if let Some(dir) = opts.out_dir {
                let _ = crate::retail_compare_image::write_side_by_side(
                    &dir.join(format!("{}.png", entry.label)),
                    rf,
                    &ef,
                );
            }
            Some(crate::retail_compare_image::score(rf, &ef))
        }
        Err(e) => {
            report
                .detail
                .insert("image".into(), format!("engine frame failed: {e:#}"));
            None
        }
    }
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

    /// Lay this run's measurements over a prior baseline: states and
    /// channels this run measured replace the prior values, everything else
    /// (states outside a `--filter`, the image channel on a run without a
    /// display) is carried over unchanged.
    pub fn merged_over(self, prior: Option<Baseline>) -> Self {
        let Some(mut out) = prior else { return self };
        out.settle_ticks = self.settle_ticks;
        out.classes.extend(self.classes);
        for (label, chans) in self.states {
            out.states.entry(label).or_default().extend(chans);
        }
        out
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

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_core::field_menu::FieldMenuRow;
    use legaia_engine_core::input::PadButton;

    fn ram_with_subscreen(sub: u8) -> Vec<u8> {
        let mut ram = vec![0u8; 0x20_0000];
        ram[(MENU_SUBSCREEN & 0x1F_FFFF) as usize] = sub;
        ram
    }

    /// The sub-screen word names the screen: a root row's route, one of the
    /// Equip row's three steps, the title family (clear) or a script-entered
    /// screen no root row routes to.
    #[test]
    fn a_menu_capture_is_classified_by_its_subscreen() {
        let m = RetailMenu::from_ram(&ram_with_subscreen(0x15), "town01").unwrap();
        assert_eq!((m.row, m.equip_depth), (FieldMenuRow::Status, 0));
        let m = RetailMenu::from_ram(&ram_with_subscreen(0x13), "map01").unwrap();
        assert_eq!((m.row, m.equip_depth), (FieldMenuRow::Equip, 1));
        let m = RetailMenu::from_ram(&ram_with_subscreen(0x14), "map01").unwrap();
        assert_eq!((m.row, m.equip_depth), (FieldMenuRow::Equip, 2));
        assert!(RetailMenu::from_ram(&ram_with_subscreen(0), "opdeene").is_err());
        assert!(RetailMenu::from_ram(&ram_with_subscreen(0x20), "koin1").is_err());
    }

    fn field_run_ram(label: &str, loaded_define: u16) -> Vec<u8> {
        let mut ram = vec![0u8; 0x20_0000];
        let at = |va: u32| (va & 0x1F_FFFF) as usize;
        let l = at(game_anchors::SCENE_NAME_VA);
        ram[l..l + label.len()].copy_from_slice(label.as_bytes());
        ram[at(game_anchors::GAME_MODE_VA)] = 0x03;
        let p = at(game_anchors::PLAYER_PTR_VA);
        ram[p..p + 4].copy_from_slice(&0x8010_0000u32.to_le_bytes());
        let d = at(LOADED_SCENE_DEFINE);
        ram[d..d + 2].copy_from_slice(&loaded_define.to_le_bytes());
        ram
    }

    /// A walked crossing caught between the scene-change packet and the next
    /// field init reads the incoming label over the outgoing scene: the state
    /// is scored as the scene `0x80084540` names, and a settled state keeps
    /// its label.
    #[test]
    fn a_pending_door_is_scored_as_the_loaded_scene() {
        let cdname: legaia_prot::cdname::IndexMap =
            [(391, "map03".to_string()), (399, "doman".to_string())].into();
        let mut obs = RetailObs::from_ram(&field_run_ram("doman", 391), None);
        assert_eq!(obs.class, StateClass::Field);
        obs.settle_on_loaded_scene(&cdname);
        assert_eq!(obs.scene, "map03");
        assert_eq!(obs.pending_scene.as_deref(), Some("doman"));
        assert_eq!(obs.class, StateClass::WorldMap);

        let mut settled = RetailObs::from_ram(&field_run_ram("doman", 399), None);
        settled.settle_on_loaded_scene(&cdname);
        assert_eq!(settled.scene, "doman");
        assert_eq!(settled.pending_scene, None);
    }

    #[test]
    fn the_drive_walks_the_root_cursor_onto_the_row() {
        let m = RetailMenu::from_ram(&ram_with_subscreen(0x17), "town01").unwrap();
        let presses = pause_menu_presses(&m);
        let buttons: Vec<PadButton> = presses.iter().map(|p| p.1).collect();
        let mut want = vec![PadButton::Start];
        want.extend(std::iter::repeat_n(PadButton::Down, 4));
        want.push(PadButton::Cross);
        assert_eq!(buttons, want);
        assert!(
            presses
                .windows(2)
                .all(|w| w[1].0 - w[0].0 == MENU_PRESS_GAP)
        );
    }
}
