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
/// The arrival facing `_DAT_80073EFC` (op `0x3F`'s compass write, zeroed by
/// the card load), which the entry script's `4C 3A` copies onto the player.
const ARRIVAL_FACING: u32 = 0x8007_3EFC;
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
/// The actor `+0x10` bit under which op `0x4C` nibble-4 sub-2 writes the
/// script height `+0x8E` into `world_y`.
const PLAYER_SCRIPT_HEIGHT: u32 = 0x2000_0000;
/// `_DAT_801F348C`, the field party HUD's idle countdown (`FUN_801D0D38`).
const HUD_COUNTDOWN: u32 = 0x801F_348C;
/// The field camera parameter block's base (`0x8007B600`; the block proper
/// is `+6..+0x28`, [`legaia_engine_core::camera_zone::CameraZoneConfig::from_retail_block`]).
const CAMERA_BLOCK: u32 = 0x8007_B600;
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
    /// The scenario's `resident_patch` - the patch family whose executable
    /// the state replays, when it was made on a patched disc.
    pub resident_patch: Option<String>,
    /// The scenario's `ram_injected` - combatant fields its capture probe
    /// wrote after battle init, which the battle channels do not score.
    pub ram_injected: Vec<String>,
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
                    resident_patch: sc.resident_patch.clone(),
                    ram_injected: sc.ram_injected.clone(),
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
    /// The height a script holds the player at, when one does: the
    /// player's `+0x8E` while `+0x10 & 0x20000000` is up. Op `0x4C` nibble-4
    /// sub-2 ramps `+0x8E` and, with that bit set, writes `world_y = -value`
    /// over the floor, so the `Y` above is the script's and not a floor
    /// sample (`docs/subsystems/script-vm.md`, the actor `+0x8E` row).
    pub script_height: Option<i16>,
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
    /// The field camera parameter block (`0x8007B607..0x8007B627`) on a
    /// walkable state: the camera-region record a script or a walk-on
    /// loader last installed. The seat composes its arrival snap from it
    /// rather than from a tile re-query, because which loader ran last is
    /// walk history the save does not carry (`kor5`'s tile-X 26 / 28 band
    /// loads `P2[1]` / `P2[0]`).
    pub camera_block: Option<legaia_engine_core::camera_zone::CameraZoneConfig>,
    /// The focus pair (`0x80089118` / `0x80089120`, stored form) on a
    /// walkable state whose focus is not on the player: history the seat
    /// cannot replay. The follow ease writes the focus only on a frame it
    /// runs and the player moved (`FUN_801DB510`'s stationary test at
    /// `0x801DB578..0x801DB5A4`), and the player tick skips the ease
    /// entirely while the player is movement-locked (`FUN_801D1344`,
    /// branch at `0x801D17DC`), so a player that was poked, or carried by a
    /// script while locked, stands away from a focus that stays where the
    /// last eased frame left it. The image child takes it as
    /// `LEGAIA_SEAT_FOCUS` so it frames what retail framed; the headless
    /// `camera` channel does not, so it keeps reporting the focus miss.
    pub seat_focus: Option<[i32; 2]>,
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
    /// The field VM's script-counter slot table (`0x801C6460`, signed
    /// halfwords). It sits outside the save window, so the card-load seed
    /// cannot restore it; scripts keep timers in it (the door-fade
    /// handshake's frame count, `jouind`'s spawn delay), and a timer seeded
    /// at `0` instead of its captured count fires on a different frame.
    pub slot_table: Option<[i16; 256]>,
    /// The live mode-3 CLUT-cell cyclers ([`retail_cell_fx`]), handed to the
    /// image child as `LEGAIA_SEAT_CLUT_FX` so the cycled palettes show the
    /// captured phase.
    pub cell_fx: Vec<legaia_engine_core::clut_cell_fx::ClutCellFx>,
    /// The live fog-pool records ([`retail_fog`]), handed to the image child
    /// as `LEGAIA_SEAT_FOG` and installed on the frame it captures.
    pub fog: Vec<legaia_engine_core::fog_particles::FogParticle>,
    /// The live mode-4 scroller rects and their captured texels
    /// ([`retail_scroll_rects`]), handed to the image child as a
    /// `LEGAIA_SEAT_VRAM_RECTS` file.
    pub scroll_rects: Vec<SeededVramRect>,
    /// Drawn field actors' live model ids ([`retail_object_models`]), handed
    /// to the image child as `LEGAIA_SEAT_OBJECT_MODELS` for the placed
    /// objects a motion stream re-binds.
    pub object_models: Vec<(u16, i16)>,
    /// Field actors' live VDF morph envelopes ([`retail_morphs`]), handed to
    /// the image child as `LEGAIA_SEAT_MORPHS`.
    pub morphs: Vec<MorphSeed>,
    /// Ambient walkers' live seats ([`retail_walkers`]), handed to the image
    /// child as `LEGAIA_SEAT_WALKERS`.
    pub walkers: Vec<WalkerSeed>,
    /// The live image-panel widget ([`retail_panel`]), handed to the image
    /// child as `LEGAIA_SEAT_PANEL`; the texels it shows ride the
    /// `LEGAIA_SEAT_VRAM_RECTS` file beside the scroller rects.
    pub panel: Option<legaia_engine_core::screen_fx::PanelWidget>,
    /// The frame's clear colour - the draw environment's `r0 / g0 / b0`
    /// (`0x8007BF5D..5F`) - handed to the image child as `LEGAIA_SEAT_CLEAR`
    /// ([`retail_clear_rgb`]).
    pub clear_rgb: Option<[u8; 3]>,
    /// The player's heading `+0x26` (retail space, `0` = -Z), when it still
    /// equals the arrival facing `_DAT_80073EFC` the entry script's `4C 3A`
    /// gave it; `None` once the pad has turned the player.
    pub player_facing: Option<i16>,
    /// The player's heading `+0x26` (retail space), whatever turned it: the
    /// seed stands the player in it, as it seats the position.
    pub player_heading: Option<i16>,
    /// `_DAT_80073EFC`, the arrival facing.
    pub arrival_facing: i16,
    /// Every field-actor-ticked placement's heading ([`retail_actor_facings`]).
    pub actor_facings: Vec<ActorFacing>,
}

/// One field actor's heading as a retail state holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorFacing {
    /// `+0x50`, the record's flat MAN index.
    pub flat: u16,
    /// `+0x26`, the yaw (retail space: `0` = -Z, 12-bit).
    pub facing: i16,
    /// `+0x14` / `+0x18`.
    pub x: i16,
    pub z: i16,
    /// `+0x64`, the live model id (bank-relative ids are not resolved).
    pub model: i16,
    /// `+0x10`.
    pub flags: u32,
    /// The heading is ambient history: the actor runs the ambient motion VM
    /// (`FUN_80038158`, [`retail_ambient_heading`]) on a stream that turns
    /// it, so `+0x26` is where the stream's ramps and `rand()`-picked
    /// wanders stood at the capture instant - time and stream history since
    /// the entry, which no seed replays.
    pub ambient: bool,
}

/// Motion-VM ops that write `+0x26`: the directional steps `0x03` / `0x19`
/// / `0x20`, the ramps `0x04` / `0x0D`, the home-relative step `0x06` and
/// the AABB wander `0x18` (`docs/subsystems/motion-vm.md`).
const AMBIENT_HEADING_OPS: [u8; 7] = [0x03, 0x04, 0x06, 0x0D, 0x18, 0x19, 0x20];

/// Whether actor node `n`'s heading is the ambient motion VM's: the VM is
/// dispatched on it (`+0x10 & 0x80`), no script or pursue context holds it
/// (`+0x10 & 0x500`, the busy test the interpreter defers to), and the
/// variant its PC sits in (`*(+0x80) + *(+0x84)`, the variant table
/// `[u16 selector][s16 delta]` the preamble walks) carries a heading op
/// before its loop-back `0x01`.
pub fn retail_ambient_heading(ram: &[u8], n: u32) -> bool {
    game_anchors::u32_at(ram, n + 0x10) & 0x500 == 0 && retail_stream_turns(ram, n)
}

/// Whether node `n` runs an ambient motion stream that walks or turns it
/// (`+0x10 & 0x80`, a heading op in the variant its PC sits in), whatever
/// holds it now: an engaged wanderer stopped where its walks left it.
pub fn retail_stream_turns(ram: &[u8], n: u32) -> bool {
    use legaia_asset::man_motion::op_width;
    let flags = game_anchors::u32_at(ram, n + 0x10);
    if flags & 0x80 == 0 {
        return false;
    }
    let stream = game_anchors::u32_at(ram, n + 0x80);
    if !(0x8000_0000..0x8020_0000).contains(&stream) {
        return false;
    }
    let pc = stream + u32::from(game_anchors::u16_at(ram, n + 0x84));
    // The variant whose code holds the PC.
    let mut header = stream;
    let mut code = None;
    for _ in 0..64 {
        let selector = game_anchors::u16_at(ram, header);
        let delta = game_anchors::i16_at(ram, header + 2);
        let end = if selector == 0xFFFF || delta <= 0 {
            u32::MAX
        } else {
            header + delta as u32
        };
        if (header + 4..end).contains(&pc) {
            code = Some(header + 4);
            break;
        }
        if end == u32::MAX {
            break;
        }
        header = end;
    }
    let Some(mut at) = code else {
        return false;
    };
    for _ in 0..256 {
        if at >= 0x8020_0000 {
            break;
        }
        let op = game_anchors::u8_at(ram, at);
        if AMBIENT_HEADING_OPS.contains(&op) {
            return true;
        }
        match (op, op_width(op)) {
            (0x01, _) | (_, None) => break,
            (_, Some(w)) => at += w as u32,
        }
    }
    false
}

/// The heading `+0x26` of every actor the field actor tick (`FUN_8003BC08`)
/// runs, keyed by its flat MAN index `+0x50` (first node per index).
pub fn retail_actor_facings(ram: &[u8]) -> Vec<ActorFacing> {
    let player = game_anchors::player_ptr(ram);
    let mut seen = std::collections::BTreeSet::new();
    crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .filter(|&n| Some(n) != player && game_anchors::u32_at(ram, n + 0x0C) == 0x8003_BC08)
        .filter_map(|n| {
            let flat = game_anchors::u16_at(ram, n + 0x50);
            seen.insert(flat).then(|| ActorFacing {
                flat,
                facing: game_anchors::i16_at(ram, n + 0x26),
                x: game_anchors::i16_at(ram, n + 0x14),
                z: game_anchors::i16_at(ram, n + 0x18),
                model: game_anchors::i16_at(ram, n + 0x64),
                flags: game_anchors::u32_at(ram, n + 0x10),
                ambient: retail_ambient_heading(ram, n),
            })
        })
        .collect()
}

/// The draw environment's clear colour bytes (`r0 / g0 / b0`).
const DRAW_ENV_CLEAR: u32 = 0x8007_BF5D;

/// The clear colour a field state's frame is filled with wherever no
/// primitive lands: the draw environment's `r0 / g0 / b0` (`0x8007BF5D..5F`),
/// which op `4C 13` writes and the MAN loader zeroes. It is the system
/// script's history - `town01`'s entry loop sets cave brown inside its cliff
/// box only on a pass the player is free for, and the opening holds the
/// player from the install pass on (the `rim_elm_zoom_intro` capture's system
/// context is still parked on its install-pass PC, the colour black) - which
/// a seed that runs the loop before the resume cannot reproduce. The image
/// child writes it over the engine's on the frame it captures.
pub fn retail_clear_rgb(ram: &[u8]) -> [u8; 3] {
    [0, 1, 2].map(|i| game_anchors::u8_at(ram, DRAW_ENV_CLEAR + i))
}

use legaia_engine_core::world::SeededVramRect;

/// The actor tick that runs a move-VM part (`FUN_80021DF4`).
const PART_TICK: u32 = 0x8002_1DF4;

/// Every live mode-4 VRAM scroller on a retail state's actor lists - a
/// `FUN_80021DF4` part with `+0x5A = 4`, rect `+0xD0..+0xD6`
/// ([`legaia_engine_core::world::ambient`]'s `vram_scroll`) - with the
/// texels the state's VRAM (`1024 x 512` BGR555 LE) holds there.
pub fn retail_scroll_rects(ram: &[u8], vram: &[u8]) -> Vec<SeededVramRect> {
    if vram.len() != 1024 * 512 * 2 {
        return Vec::new();
    }
    let mut out: Vec<SeededVramRect> = Vec::new();
    let step = i16::from(crate::retail_compare_battle::frame_step(ram));
    for n in crate::retail_compare_script::actor_nodes(ram) {
        if game_anchors::u32_at(ram, n + 0x0C) != PART_TICK
            || game_anchors::i16_at(ram, n + 0x5A) != 4
        {
            continue;
        }
        let rect = (
            game_anchors::u16_at(ram, n + 0xD0),
            game_anchors::u16_at(ram, n + 0xD2),
            game_anchors::u16_at(ram, n + 0xD4),
            game_anchors::u16_at(ram, n + 0xD6),
        );
        let (x, y, w, h) = rect;
        if w == 0 || h == 0 || w > 1024 || h > 512 || out.iter().any(|(r, _)| *r == rect) {
            continue;
        }
        let texels = (0..h)
            .flat_map(|row| (0..w).map(move |col| (row, col)))
            .map(|(row, col)| {
                let o =
                    (((usize::from(y + row) & 0x1FF) * 1024) + (usize::from(x + col) & 0x3FF)) * 2;
                u16::from_le_bytes([vram[o], vram[o + 1]])
            })
            .collect::<Vec<u16>>();
        // The displayed frame is two game frames older than the VRAM: take
        // back the rotations the scroller fired in between.
        let fires = scroll_fires_within(
            game_anchors::i16_at(ram, n + 0xC4),
            game_anchors::i16_at(ram, n + 0xC6),
            step,
            DISPLAY_LAG_FRAMES,
        );
        let back = |per_tick: i16, extent: u16| -> usize {
            let e = i32::from(extent).max(1);
            (i32::from(per_tick) * i32::from(step) * fires).rem_euclid(e) as usize
        };
        let (bw, bh) = (
            back(game_anchors::i16_at(ram, n + 0xCC), w),
            back(game_anchors::i16_at(ram, n + 0xCE), h),
        );
        out.push((
            rect,
            unrotate_rect(&texels, usize::from(w), usize::from(h), bw, bh),
        ));
    }
    out
}

/// Game frames the displayed frame lags the RAM by (the double-buffer law of
/// [`crate::retail_compare_battle::display_lag_vsyncs`], in frames).
const DISPLAY_LAG_FRAMES: i32 = 2;

/// How many times a mode-4 scroller fired over its last `lag` game ticks,
/// from its live countdown `+0xC6` and reload `+0xC4`: the countdown drains
/// `step` a tick and fires the tick it goes negative, reloading to the
/// period (`vram_scroll::mode4_integrate`), so it fires every
/// `period / step + 1` ticks and a countdown equal to the period fired on the
/// current tick.
fn scroll_fires_within(period: i16, countdown: i16, step: i16, lag: i32) -> i32 {
    let (p, c, s) = (
        i32::from(period),
        i32::from(countdown),
        i32::from(step.max(1)),
    );
    if p < 0 || c > p {
        return 0;
    }
    let cycle = p / s + 1;
    let since = (p - c) / s;
    if since > lag - 1 {
        0
    } else {
        1 + (lag - 1 - since) / cycle
    }
}

/// Rotate a `w x h` rect **right** by `dx` and **down** by `dy` - the inverse
/// of the scroller's left / up rotation.
fn unrotate_rect(texels: &[u16], w: usize, h: usize, dx: usize, dy: usize) -> Vec<u16> {
    if w == 0 || h == 0 || texels.len() < w * h {
        return texels.to_vec();
    }
    (0..h)
        .flat_map(|row| (0..w).map(move |col| (row, col)))
        .map(|(row, col)| texels[((row + h - dy % h) % h) * w + (col + w - dx % w) % w])
        .collect()
}

/// The image-panel widget's handler (`FUN_801F849C`, PROT 0900).
const PANEL_TICK: u32 = 0x801F_849C;

/// The first live image-panel widget on a retail state's actor lists
/// ([`legaia_engine_core::screen_fx::PanelWidget`]'s field map: current
/// `+0x14..+0x1A` / `+0x24`, targets `+0x3C..+0x42` / `+0x26`, base sizes
/// `+0xB8..+0xBC`, tween `+0x9C` / `+0x9E`, spawn size `+0xAA` / `+0xAC`,
/// texel origin `+0xA4` / `+0xA8`, pages `+0xA0` / `+0xA2`).
///
/// The ending vignettes spawn the panel from the vignette's own record
/// (`43 12` grabs the drawn frame into `(512, 0)`, `43 13` shows it), and a
/// capture is usually parked in the credits record that runs after it -
/// `ending_panel_corner` holds record 13, the panel already shrunk to the
/// corner - so a seed that resumes the running record never spawns it.
pub fn retail_panel(ram: &[u8]) -> Option<legaia_engine_core::screen_fx::PanelWidget> {
    let n = crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .find(|&n| game_anchors::u32_at(ram, n + 0x0C) == PANEL_TICK)?;
    let h = |o: u32| game_anchors::i16_at(ram, n + o);
    Some(legaia_engine_core::screen_fx::PanelWidget {
        cur: [h(0x14), h(0x16), h(0x18), h(0x1A), h(0x24)],
        target: [h(0x3C), h(0x3E), h(0x40), h(0x42), h(0x26)],
        base: [h(0xB8), h(0xBA), h(0xBC)],
        t: h(0x9C),
        dur: h(0x9E),
        w0: h(0xAA),
        h0: h(0xAC),
        u: game_anchors::u8_at(ram, n + 0xA4),
        v: game_anchors::u8_at(ram, n + 0xA8),
        texpage: game_anchors::u16_at(ram, n + 0xA0),
        texpage2: game_anchors::u16_at(ram, n + 0xA2),
    })
}

/// The VRAM the panel samples, with the state's texels there: page 0 from
/// the texel origin over the spawn size (one page at most), and - for a
/// panel wider than a page - page 1 out to the far edge its quad's `u`
/// reaches. That edge is not the image's: `FUN_801F849C` starts the second
/// quad at `u + 0x100 + 0xE` and ends it at `u + w0 + 0x10`
/// (`0x801F8838..0x801F88B0`, byte-wrapped), past the `320`-wide grab, and
/// the `43 12` split copies `0x60` columns from source `+0xF0` to match
/// (`legaia_engine_vm::vram_rect_copy::op43_sub12_calls`) - so a seed cut at
/// the image's own width left the strip's last texels unseeded.
fn panel_source_rects(
    p: &legaia_engine_core::screen_fx::PanelWidget,
    vram: &[u8],
) -> Vec<SeededVramRect> {
    let (w0, h) = (p.w0.clamp(0, 1024) as u16, p.h0.clamp(0, 512) as u16);
    if w0 == 0 || h == 0 || vram.len() != 1024 * 512 * 2 {
        return Vec::new();
    }
    let page = |tp: u16| ((tp & 0xF) * 64, ((tp >> 4) & 1) * 256 + u16::from(p.v));
    let grab = |x: u16, y: u16, w: u16| -> SeededVramRect {
        let texels = (0..h)
            .flat_map(|row| (0..w).map(move |col| (row, col)))
            .map(|(row, col)| {
                let o =
                    (((usize::from(y + row) & 0x1FF) * 1024) + (usize::from(x + col) & 0x3FF)) * 2;
                u16::from_le_bytes([vram[o], vram[o + 1]])
            })
            .collect();
        ((x, y, w, h), texels)
    };
    let (x0, y0) = page(p.texpage);
    let mut out = vec![grab(x0 + u16::from(p.u), y0, w0.min(0x100))];
    if p.texpage2 != 0 {
        let (x1, y1) = page(p.texpage2);
        let far = u16::from(p.u.wrapping_add(p.w0 as u8).wrapping_add(0x10)) + 1;
        out.push(grab(x1, y1, far));
    }
    out
}

/// [`retail_panel`] as `LEGAIA_SEAT_PANEL`: the widget's fields as
/// comma-separated integers in [`panel_from_env`]'s order.
pub fn panel_env(p: &legaia_engine_core::screen_fx::PanelWidget) -> String {
    let mut v: Vec<i32> = Vec::new();
    v.extend(p.cur.iter().map(|&x| i32::from(x)));
    v.extend(p.target.iter().map(|&x| i32::from(x)));
    v.extend(p.base.iter().map(|&x| i32::from(x)));
    v.extend([
        i32::from(p.t),
        i32::from(p.dur),
        i32::from(p.w0),
        i32::from(p.h0),
        i32::from(p.u),
        i32::from(p.v),
        i32::from(p.texpage),
        i32::from(p.texpage2),
    ]);
    v.iter()
        .map(|x| x.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Inverse of [`panel_env`].
pub fn panel_from_env(s: &str) -> Option<legaia_engine_core::screen_fx::PanelWidget> {
    let v: Vec<i32> = s
        .split(',')
        .map(|t| t.trim().parse().ok())
        .collect::<Option<_>>()?;
    if v.len() != 21 {
        return None;
    }
    let h = |i: usize| v[i] as i16;
    Some(legaia_engine_core::screen_fx::PanelWidget {
        cur: [h(0), h(1), h(2), h(3), h(4)],
        target: [h(5), h(6), h(7), h(8), h(9)],
        base: [h(10), h(11), h(12)],
        t: h(13),
        dur: h(14),
        w0: h(15),
        h0: h(16),
        u: v[17] as u8,
        v: v[18] as u8,
        texpage: v[19] as u16,
        texpage2: v[20] as u16,
    })
}

/// [`retail_scroll_rects`] as the bytes of a `LEGAIA_SEAT_VRAM_RECTS` file:
/// per rect `x, y, w, h` (`u16` LE) then its `w * h` texels.
pub fn vram_rects_file(rects: &[SeededVramRect]) -> Vec<u8> {
    let mut out = Vec::new();
    for ((x, y, w, h), texels) in rects {
        for v in [*x, *y, *w, *h].iter().chain(texels) {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

/// Inverse of [`vram_rects_file`]; a truncated tail is dropped.
pub fn vram_rects_from_file(bytes: &[u8]) -> Vec<SeededVramRect> {
    let words: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= words.len() {
        let (x, y, w, h) = (words[i], words[i + 1], words[i + 2], words[i + 3]);
        let n = usize::from(w) * usize::from(h);
        let Some(texels) = words.get(i + 4..i + 4 + n) else {
            break;
        };
        out.push(((x, y, w, h), texels.to_vec()));
        i += 4 + n;
    }
    out
}

/// The scene model bank's pool base (`*(u16*)0x8007B6F8`): a field actor's
/// `+0x64` is this plus its scene-bank model id (`FUN_8003A1E4`,
/// `FUN_80024E08`).
const MODEL_BANK_BASE: u32 = 0x8007_B6F8;

/// Each bind record's `(record +0x50, scene-bank model id)`, off the
/// **first** field actor in list order that carries it: the one a motion
/// stream is bound to (`FUN_8003A9D4`, [`legaia_engine_core::field_env::stream_bound_draws`]),
/// whose `+0x64` less the bank base is the model the stream's op `0x0E` last
/// swapped in.
pub fn retail_object_models(ram: &[u8]) -> Vec<(u16, i16)> {
    let base = i32::from(game_anchors::u16_at(ram, MODEL_BANK_BASE));
    let mut seen = std::collections::BTreeSet::new();
    crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .filter(|&n| game_anchors::u32_at(ram, n + 0x0C) == 0x8003_BC08)
        .filter_map(|n| {
            let record = game_anchors::u16_at(ram, n + 0x50);
            if !seen.insert(record) {
                return None;
            }
            let id = i32::from(game_anchors::i16_at(ram, n + 0x64)) - base;
            (0..0xF0).contains(&id).then_some((record, id as i16))
        })
        .collect()
}

/// One ambient walker's live seat: flat MAN index `+0x50`, `+0x14` /
/// `+0x18`, and the retail-space heading `+0x26`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkerSeed {
    pub flat: u16,
    pub x: i16,
    pub z: i16,
    pub heading: u16,
}

/// Every placement the ambient motion VM is walking or turning
/// ([`retail_ambient_heading`]), at its live seat - the `rand()` history
/// the facing channel does not score, which the image child stands where
/// retail's frame shows it (`World::seed_ambient_walker`).
pub fn retail_walkers(ram: &[u8]) -> Vec<WalkerSeed> {
    let player = game_anchors::player_ptr(ram);
    let mut seen = std::collections::BTreeSet::new();
    crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .filter(|&n| Some(n) != player && game_anchors::u32_at(ram, n + 0x0C) == 0x8003_BC08)
        .filter(|&n| retail_stream_turns(ram, n))
        .filter_map(|n| {
            let flat = game_anchors::u16_at(ram, n + 0x50);
            seen.insert(flat).then(|| WalkerSeed {
                flat,
                x: game_anchors::i16_at(ram, n + 0x14),
                z: game_anchors::i16_at(ram, n + 0x18),
                heading: game_anchors::u16_at(ram, n + 0x26) & 0x0FFF,
            })
        })
        .collect()
}

/// [`retail_walkers`] as `LEGAIA_SEAT_WALKERS`: `flat:x:z:heading` per
/// walker, `;`-joined, decimal.
pub fn walkers_env(w: &[WalkerSeed]) -> String {
    w.iter()
        .map(|s| format!("{}:{}:{}:{}", s.flat, s.x, s.z, s.heading))
        .collect::<Vec<_>>()
        .join(";")
}

/// Parse [`walkers_env`].
pub fn walkers_from_env(v: &str) -> Vec<WalkerSeed> {
    v.split(';')
        .filter_map(|e| {
            let mut f = e.trim().split(':');
            Some(WalkerSeed {
                flat: f.next()?.parse().ok()?,
                x: f.next()?.parse().ok()?,
                z: f.next()?.parse().ok()?,
                heading: f.next()?.parse().ok()?,
            })
        })
        .collect()
}

/// One field actor's live VDF morph envelope (op `0x4B`,
/// `legaia_engine_core::world::npc_morph`): its flat MAN index `+0x50`, the
/// lane weights `+0xA0 + i*2` over its `+0x6C` lanes, the lane-done mask
/// `+0x7C` and the envelope control word `+0x62`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorphSeed {
    pub flat: u16,
    pub weights: Vec<u16>,
    pub done_mask: u32,
    pub env: u16,
}

/// Every field-actor-ticked node whose envelope is up (`+0x10 & 0x1000`)
/// with armed lanes - where its morph stands is time since the arm
/// (`town01`'s shoreline tide), so the image child writes it over the
/// engine's on the frame it captures (`World::seed_field_morph`).
pub fn retail_morphs(ram: &[u8]) -> Vec<MorphSeed> {
    let mut seen = std::collections::BTreeSet::new();
    crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .filter(|&n| game_anchors::u32_at(ram, n + 0x0C) == 0x8003_BC08)
        .filter(|&n| game_anchors::u32_at(ram, n + 0x10) & 0x1000 != 0)
        .filter_map(|n| {
            let flat = game_anchors::u16_at(ram, n + 0x50);
            let lanes = u32::from(game_anchors::u8_at(ram, n + 0x6C)).min(8);
            (lanes > 0 && seen.insert(flat)).then(|| MorphSeed {
                flat,
                weights: (0..lanes)
                    .map(|i| game_anchors::u16_at(ram, n + 0xA0 + i * 2))
                    .collect(),
                done_mask: game_anchors::u32_at(ram, n + 0x7C),
                env: game_anchors::u16_at(ram, n + 0x62),
            })
        })
        .collect()
}

/// [`retail_morphs`] as `LEGAIA_SEAT_MORPHS`:
/// `flat:w0/w1/..:done:env` per actor, `;`-joined, numbers in hex.
pub fn morphs_env(m: &[MorphSeed]) -> String {
    m.iter()
        .map(|s| {
            format!(
                "{:x}:{}:{:x}:{:x}",
                s.flat,
                s.weights
                    .iter()
                    .map(|w| format!("{w:x}"))
                    .collect::<Vec<_>>()
                    .join("/"),
                s.done_mask,
                s.env
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Parse [`morphs_env`].
pub fn morphs_from_env(v: &str) -> Vec<MorphSeed> {
    v.split(';')
        .filter_map(|e| {
            let mut f = e.trim().split(':');
            let flat = u16::from_str_radix(f.next()?, 16).ok()?;
            let weights = f
                .next()?
                .split('/')
                .map(|w| u16::from_str_radix(w, 16).ok())
                .collect::<Option<Vec<u16>>>()?;
            let done_mask = u32::from_str_radix(f.next()?, 16).ok()?;
            let env = u16::from_str_radix(f.next()?, 16).ok()?;
            Some(MorphSeed {
                flat,
                weights,
                done_mask,
                env,
            })
        })
        .collect()
}

/// The fog pool pointer (`_DAT_8007B7E0`, [`legaia_engine_core::fog_particles`]).
const FOG_POOL_PTR: u32 = 0x8007_B7E0;

/// Every live record of a retail state's fog pool: 80 `0x18`-byte records
/// from pool `+0xA4`, alive byte `+0x05`
/// ([`legaia_engine_core::fog_particles`] has the layout).
pub fn retail_fog(ram: &[u8]) -> Vec<legaia_engine_core::fog_particles::FogParticle> {
    let pool = game_anchors::u32_at(ram, FOG_POOL_PTR);
    if (pool & 0xFFE0_0000) != 0x8000_0000 {
        return Vec::new();
    }
    (0..legaia_engine_core::fog_particles::FOG_POOL_SLOTS as u32)
        .map(|i| pool + 0xA4 + i * 0x18)
        .filter(|&r| game_anchors::u8_at(ram, r + 5) != 0)
        .map(|r| legaia_engine_core::fog_particles::FogParticle {
            age: game_anchors::u16_at(ram, r),
            rate: game_anchors::u16_at(ram, r + 2),
            slot: game_anchors::u8_at(ram, r + 4),
            alive: true,
            vx: game_anchors::u8_at(ram, r + 6) as i8,
            vz: game_anchors::u8_at(ram, r + 7) as i8,
            x: game_anchors::u32_at(ram, r + 8) as i32,
            z: game_anchors::u32_at(ram, r + 0xC) as i32,
            y: game_anchors::i16_at(ram, r + 0x10),
            grey: game_anchors::u8_at(ram, r + 0x14),
        })
        .collect()
}

/// [`retail_fog`] as `LEGAIA_SEAT_FOG`: `slot,age,rate,vx,vz,x,z,y,grey` per
/// record, `;`-separated.
pub fn fog_env(fog: &[legaia_engine_core::fog_particles::FogParticle]) -> String {
    fog.iter()
        .map(|p| {
            format!(
                "{},{},{},{},{},{},{},{},{}",
                p.slot, p.age, p.rate, p.vx, p.vz, p.x, p.z, p.y, p.grey
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Inverse of [`fog_env`]; malformed entries are dropped.
pub fn fog_from_env(s: &str) -> Vec<legaia_engine_core::fog_particles::FogParticle> {
    s.split(';')
        .filter_map(|e| {
            let v: Vec<i64> = e.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            let [slot, age, rate, vx, vz, x, z, y, grey] = <[i64; 9]>::try_from(v).ok()?;
            Some(legaia_engine_core::fog_particles::FogParticle {
                age: age as u16,
                rate: rate as u16,
                slot: slot as u8,
                alive: true,
                vx: vx as i8,
                vz: vz as i8,
                x: x as i32,
                z: z as i32,
                y: y as i16,
                grey: grey as u8,
            })
        })
        .collect()
}

/// Every live mode-3 CLUT-cell cycler on a retail state's actor lists, in
/// list order: a part ticked by `FUN_80021DF4` with render mode `+0x5A = 3`
/// past its first armed frame (`+0x9C > 1`), as the snapshot its next
/// `FUN_80019D50` write uses - rect `+0xA0..+0xA6`, adds `+0x90/92/94`, mode
/// `+0x9E`, white amount `+0x68` ([`legaia_engine_core::clut_cell_fx`]).
pub fn retail_cell_fx(ram: &[u8]) -> Vec<legaia_engine_core::clut_cell_fx::ClutCellFx> {
    crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .filter(|&n| {
            game_anchors::u32_at(ram, n + 0x0C) == PART_TICK
                && game_anchors::i16_at(ram, n + 0x5A) == 3
                && game_anchors::i16_at(ram, n + 0x9C) > 1
        })
        .map(|n| legaia_engine_core::clut_cell_fx::ClutCellFx {
            rect: (
                game_anchors::u16_at(ram, n + 0xA0),
                game_anchors::u16_at(ram, n + 0xA2),
                game_anchors::u16_at(ram, n + 0xA4),
                game_anchors::u16_at(ram, n + 0xA6),
            ),
            h_add: game_anchors::i16_at(ram, n + 0x90),
            s_add: game_anchors::i16_at(ram, n + 0x92),
            v_add: game_anchors::i16_at(ram, n + 0x94),
            mode: game_anchors::i16_at(ram, n + 0x9E),
            white: game_anchors::i16_at(ram, n + 0x68),
        })
        .collect()
}

/// [`retail_cell_fx`] as `LEGAIA_SEAT_CLUT_FX`: `x,y,w,h,h,s,v,mode,white`
/// per part, `;`-separated.
pub fn cell_fx_env(fx: &[legaia_engine_core::clut_cell_fx::ClutCellFx]) -> String {
    fx.iter()
        .map(|f| {
            format!(
                "{},{},{},{},{},{},{},{},{}",
                f.rect.0, f.rect.1, f.rect.2, f.rect.3, f.h_add, f.s_add, f.v_add, f.mode, f.white
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Inverse of [`cell_fx_env`]; malformed entries are dropped.
pub fn cell_fx_from_env(s: &str) -> Vec<legaia_engine_core::clut_cell_fx::ClutCellFx> {
    s.split(';')
        .filter_map(|e| {
            let v: Vec<i32> = e.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            let [x, y, w, h, hh, ss, vv, mode, white] = <[i32; 9]>::try_from(v).ok()?;
            Some(legaia_engine_core::clut_cell_fx::ClutCellFx {
                rect: (x as u16, y as u16, w as u16, h as u16),
                h_add: hh as i16,
                s_add: ss as i16,
                v_add: vv as i16,
                mode: mode as i16,
                white: white as i16,
            })
        })
        .collect()
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

/// The two staging descriptors the composer `FUN_801DAB90` writes: the
/// follow ease's (`FUN_801DB510`) and op `0x45` APPLY's.
const CAMERA_STAGINGS: [u32; 2] = [0x801F_3580, 0x801C_6EA8];

/// Whether retail's camera has composed from `block` since it was loaded:
/// a staging descriptor carries the block's `H` (`+0x26`, copied in every
/// mode). A loader that ran after the last compose - a walk-on band the
/// player arrived on and has not moved from (`kor5_field_card_boot`
/// stands on `P2[0]`'s tile) - leaves the live camera on the previous
/// block, which the seat's tile re-query reproduces.
fn camera_block_composed(
    ram: &[u8],
    block: &legaia_engine_core::camera_zone::CameraZoneConfig,
) -> bool {
    CAMERA_STAGINGS
        .iter()
        .any(|&st| i32::from(rd16(ram, st + 0x26)) == block.h)
}

/// The 40 bytes from [`CAMERA_BLOCK`].
fn camera_block_bytes(ram: &[u8]) -> Option<Vec<u8>> {
    let lo = (CAMERA_BLOCK & 0x1F_FFFF) as usize;
    ram.get(lo..lo + 0x28).map(<[u8]>::to_vec)
}

/// `LEGAIA_SEAT_CAMERA_BLOCK` for `play-window`: the block's 40 bytes from
/// [`CAMERA_BLOCK`], hex.
pub fn camera_block_env(block: &legaia_engine_core::camera_zone::CameraZoneConfig) -> String {
    block
        .to_retail_block()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Inverse of [`camera_block_env`].
pub fn camera_block_from_env(s: &str) -> Option<legaia_engine_core::camera_zone::CameraZoneConfig> {
    let s = s.trim();
    if s.len() != 0x50 {
        return None;
    }
    let bytes = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    legaia_engine_core::camera_zone::CameraZoneConfig::from_retail_block(&bytes)
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
        let script_height = game_anchors::player_ptr(ram)
            .filter(|&p| game_anchors::u32_at(ram, p + 0x10) & PLAYER_SCRIPT_HEIGHT != 0)
            .map(|p| rd16(ram, p + 0x8E));
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
        let mut save = ram.get(lo..lo + LIVE_STATE_LEN).and_then(|win| {
            let mut block = win.to_vec();
            block.resize(SC_BLOCK_LEN, 0);
            legaia_save::SaveFile::from_retail_sc_block(
                &block,
                legaia_save::RETAIL_SC_PARTY_RECORDS,
            )
            .ok()
        });
        let battle = (class == StateClass::Battle)
            .then(|| crate::retail_compare_battle::RetailBattle::from_ram(ram));
        // A capture on the results frame or after it holds the party with
        // the fight's rewards already applied; the seed replays the fight,
        // which applies them again.
        if let (Some(save), Some(Ok(b))) = (save.as_mut(), battle.as_ref()) {
            crate::retail_compare_battle::ungrant_results_rewards(save, b, ram);
            crate::retail_compare_battle::ungrant_magic_level_up(save, b);
        }
        Self {
            scene,
            loaded_define: game_anchors::u16_at(ram, LOADED_SCENE_DEFINE),
            pending_scene: None,
            game_mode,
            class,
            player,
            script_height,
            camera,
            bgm_id,
            bgm_sounding: game_anchors::u16_at(ram, BGM_PLAYING) != 0
                && game_anchors::u16_at(ram, BGM_SLOT + 6) != 0,
            fog_gate,
            save,
            hud_countdown: matches!(class, StateClass::Field | StateClass::WorldMap)
                .then(|| rd16(ram, HUD_COUNTDOWN)),
            camera_block: matches!(class, StateClass::Field | StateClass::WorldMap)
                .then(|| camera_block_bytes(ram))
                .flatten()
                .and_then(|b| {
                    legaia_engine_core::camera_zone::CameraZoneConfig::from_retail_block(&b)
                })
                .filter(|b| camera_block_composed(ram, b)),
            seat_focus: match (class, player) {
                (StateClass::Field | StateClass::WorldMap, Some([x, _, z]))
                    if camera.focus != [-i32::from(x), -i32::from(z)] =>
                {
                    Some(camera.focus)
                }
                _ => None,
            },
            frame,
            battle,
            menu,
            scripts: crate::retail_compare_script::RetailScripts::from_ram(ram),
            cell_fx: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_cell_fx(ram)
            } else {
                Vec::new()
            },
            fog: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_fog(ram)
            } else {
                Vec::new()
            },
            scroll_rects: Vec::new(),
            panel: None,
            walkers: if matches!(class, StateClass::Field) {
                retail_walkers(ram)
            } else {
                Vec::new()
            },
            morphs: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_morphs(ram)
            } else {
                Vec::new()
            },
            object_models: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_object_models(ram)
            } else {
                Vec::new()
            },
            clear_rgb: matches!(class, StateClass::Field).then(|| retail_clear_rgb(ram)),
            // Only while the player still faces the way the entry stood it:
            // after a pad turn the heading is walk history the seat does
            // not replay.
            player_facing: game_anchors::player_ptr(ram)
                .map(|p| rd16(ram, p + 0x26))
                .filter(|&h| h == rd16(ram, ARRIVAL_FACING)),
            player_heading: game_anchors::player_ptr(ram).map(|p| rd16(ram, p + 0x26)),
            arrival_facing: rd16(ram, ARRIVAL_FACING),
            actor_facings: if matches!(class, StateClass::Field) {
                retail_actor_facings(ram)
            } else {
                Vec::new()
            },
            slot_table: {
                let lo = (SLOT_TABLE_VA & 0x1F_FFFF) as usize;
                ram.get(lo..lo + 0x200)
                    .map(|b| std::array::from_fn(|i| i16::from_le_bytes([b[i * 2], b[i * 2 + 1]])))
            },
        }
    }
}

/// The field VM's script-counter slot table
/// (`legaia_engine_core::world::FieldVmState::slot_table`).
const SLOT_TABLE_VA: u32 = 0x801C_6460;

/// Re-assert the capture's script timers before a seeding tick.
///
/// The slot table is RAM outside the save window, so the card-load seed
/// cannot restore it, and the settle window is a warm-up, not elapsed game
/// time: a timer left to run would fire inside the window an arm retail is
/// still counting towards (a door-fade handshake 36 frames into its 46, a
/// spawn delay 24 into its 50) and score that as an engine miss. Holding the
/// captured counts keeps every timer where retail's was.
pub(crate) fn hold_slot_table(session: &mut BootSession, retail: &RetailObs) {
    if let Some(slots) = retail.slot_table {
        session.host.world.field_vm.slot_table = slots;
    }
}

impl RetailObs {
    /// The save the seed lands: the lifted save with the gate record's own
    /// run latches cleared ([`crate::retail_compare_script::run_latches`]).
    ///
    /// Retail's record set those flags after the scene entry ran, so the
    /// entry never saw them; a card load runs the entry over the whole save
    /// and an entry that tests one takes the arm retail did not.
    /// `minigame_dance_pcsx` is caught on `koin3` `P2[6]` two ops past
    /// `55 9C`, the flag the entry (`P1[0]` `+0x178`) reads as "back from
    /// the dance floor": seeded with it up, the entry cleared it and spawned
    /// the judging record `P2[9]` over the frame. The comparand is
    /// [`Self::save`] unchanged.
    ///
    /// The seed raises them again at the settle tick
    /// ([`Self::seed_latches`]), once the entry has run: retail's state holds
    /// them, and a record the seed never reaches would otherwise leave the
    /// flags channel short. Raised straight after the landing, an entry still
    /// running read them anyway - the slot-machine floor's entry restarted
    /// its track on one.
    pub fn seed_save(&self) -> Option<legaia_save::SaveFile> {
        let mut save = self.save.clone()?;
        for idx in self.seed_latches() {
            if let Some(b) = save
                .ext
                .story_flag_bits
                .get_mut(SYSTEM_FLAG_WINDOW + usize::from(idx >> 3))
            {
                *b &= !(0x80u8 >> (idx & 7));
            }
        }
        Some(save)
    }

    /// Read the field scrollers' captured rects ([`retail_scroll_rects`]).
    fn seat_scroll_rects(&mut self, ram: &[u8], vram: Option<&[u8]>) {
        if let Some(v) = vram
            && matches!(self.class, StateClass::Field | StateClass::WorldMap)
        {
            self.scroll_rects = retail_scroll_rects(ram, v);
            self.panel = retail_panel(ram);
            if let Some(p) = self.panel {
                self.scroll_rects.extend(panel_source_rects(&p, v));
            }
        }
    }

    /// The latches [`Self::seed_save`] holds back from the scene entry.
    pub fn seed_latches(&self) -> Vec<u16> {
        if self.menu.is_some() || !matches!(self.class, StateClass::Field | StateClass::WorldMap) {
            return Vec::new();
        }
        self.scripts
            .running
            .first()
            .map(|s| s.latches.clone())
            .unwrap_or_default()
    }

    /// The capture-alignment environment the image child takes on top of
    /// its seat: held-back latches, CLUT-cell phases, fog-pool records and
    /// scroller rects (the last through a file beside the child's frame).
    pub fn seat_env(&self, out_dir: Option<&Path>, label: &str) -> Vec<(&'static str, String)> {
        let mut env: Vec<(&'static str, String)> = self.seed_latches_env().into_iter().collect();
        if !self.cell_fx.is_empty() {
            env.push(("LEGAIA_SEAT_CLUT_FX", cell_fx_env(&self.cell_fx)));
        }
        if !self.fog.is_empty() {
            env.push(("LEGAIA_SEAT_FOG", fog_env(&self.fog)));
        }
        if !self.object_models.is_empty() {
            env.push((
                "LEGAIA_SEAT_OBJECT_MODELS",
                self.object_models
                    .iter()
                    .map(|(r, m)| format!("{r}:{m}"))
                    .collect::<Vec<_>>()
                    .join(","),
            ));
        }
        if !self.morphs.is_empty() {
            env.push(("LEGAIA_SEAT_MORPHS", morphs_env(&self.morphs)));
        }
        if !self.walkers.is_empty() {
            env.push(("LEGAIA_SEAT_WALKERS", walkers_env(&self.walkers)));
        }
        if let Some([r, g, b]) = self.clear_rgb {
            env.push(("LEGAIA_SEAT_CLEAR", format!("{r},{g},{b}")));
        }
        if let Some(p) = self.panel {
            env.push(("LEGAIA_SEAT_PANEL", panel_env(&p)));
        }
        // The frame child always stands in retail's heading: it seats no
        // arrival facing of its own, and the image channel scores no heading.
        if let Some(h) = self.player_heading {
            env.push(("LEGAIA_SEAT_HEADING", (h & 0x0FFF).to_string()));
        }
        if !self.scroll_rects.is_empty() {
            let dir = crate::retail_compare_image::work_dir(out_dir);
            let path = dir.join(format!("{label}.vrect.bin"));
            if std::fs::create_dir_all(&dir).is_ok()
                && std::fs::write(&path, vram_rects_file(&self.scroll_rects)).is_ok()
            {
                env.push(("LEGAIA_SEAT_VRAM_RECTS", path.display().to_string()));
            }
        }
        env
    }

    /// The player heading the seed stands the player in: retail's `+0x26`
    /// when the pad has turned it off the arrival facing, `None` while it
    /// still holds the arrival facing (the engine's own entry gives that).
    pub fn seat_heading(&self) -> Option<i16> {
        self.player_heading.filter(|&h| h != self.arrival_facing)
    }

    /// [`Self::seed_latches`] as `LEGAIA_SEAT_LATCHES` for `play-window`
    /// (hex flag ids, comma-separated), when there are any.
    pub fn seed_latches_env(&self) -> Option<(&'static str, String)> {
        let l = self.seed_latches();
        (!l.is_empty()).then(|| {
            (
                "LEGAIA_SEAT_LATCHES",
                l.iter()
                    .map(|i| format!("{i:x}"))
                    .collect::<Vec<_>>()
                    .join(","),
            )
        })
    }

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
        if !matches!(self.class, StateClass::Field | StateClass::WorldMap) {
            self.hud_countdown = None;
            self.camera_block = None;
            self.seat_focus = None;
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
            let mut obs = RetailObs::from_ram(ram, frame);
            obs.seat_scroll_rects(ram, gpu.vram_bytes());
            Ok(obs)
        }
        _ => {
            let (st, gpu) = legaia_pcsxr::gpu::load_with_scus(&entry.path, scus)?;
            let frame = gpu
                .as_ref()
                .and_then(|g| Frame::from_vram_display(&g.vram, g.display_crop_rect()));
            let mut obs = RetailObs::from_ram(st.main_ram(), frame);
            obs.seat_scroll_rects(st.main_ram(), gpu.as_ref().map(|g| g.vram.as_slice()));
            Ok(obs)
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
    /// The player's heading in retail space (`render_26 - 0x800`).
    pub player_facing: Option<i16>,
    /// Each partition-1 placement's heading (retail space) and position,
    /// keyed by its retail flat MAN index (`N0 + slot`).
    pub npc_facings: BTreeMap<u16, (i16, i16, i16)>,
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
    // Field talk runs on the inline field-VM runner, as it does in both play
    // hosts (`play-window`'s default and the browser page) and in the image
    // child: without it an engaged placement opens the plain typewriter panel
    // and its record never advances, so a capture inside a conversation
    // could not be reached headlessly while the child reached it.
    session.host.world.toggles.use_vm_dialogue = true;
    let opts = FieldLiveOpts::default();
    let save = retail
        .seed_save()
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
    // A non-zero arrival facing is a door's (the card load the seed takes
    // zeroes it): history the seed cannot replay, so it is seated like the
    // position, and the player stands as the state's entry stood it.
    if retail.arrival_facing != 0 {
        session.host.world.locomotion.arrival_facing = retail.arrival_facing;
        session.host.world.apply_arrival_facing();
    }
    if let Some([x, _, z]) = retail.player
        && session.host.debug_seat_standing(x, z)
    {
        // A heading the pad turned is walk history exactly as the position
        // is: stand the player in retail's, as the arrival facing so an
        // entry script's `4C 3A` in the settle window hands over the same
        // heading. One still on retail's arrival facing is left to the
        // engine's own entry, which the facing channel scores.
        if let Some(h) = retail.seat_heading() {
            session.host.world.locomotion.arrival_facing = h;
            session.host.world.apply_arrival_facing();
        }
        match retail.camera_block {
            Some(block) => session.camera.zone.arm_arrival_over(block),
            None => session.camera.zone.arm_arrival(),
        }
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
    if let Some(g) = &gate
        && std::env::var_os("LEGAIA_RC_SCRIPT_TRACE").is_some()
    {
        eprintln!(
            "script gate {}: flat index {} pc {} op {:#04x} wait {} glide left {:?}",
            retail.scene, g.flat_index, g.pc, g.op, g.wait, g.glide_left
        );
    }
    for t in 1..=deadline {
        if let Some(g) = &gate {
            let pad = g.advance_pad(&session.host.world, t);
            session.host.world.input.set_pad(pad);
        }
        hold_slot_table(&mut session, retail);
        session.tick()?;
        session.host.route_bgm_events(&mut director)?;
        if let Some(g) = &gate {
            if std::env::var_os("LEGAIA_RC_SCRIPT_TRACE").is_some()
                && (t % 25 == 0
                    || t < 5
                    || std::env::var_os("LEGAIA_RC_SCRIPT_TRACE_ALL").is_some())
            {
                eprintln!("script gate t={t}: {}", g.trace(&session.host.world));
            }
            if g.met(&session.host.world) {
                if std::env::var_os("LEGAIA_RC_SCRIPT_TRACE").is_some() {
                    eprintln!("script gate met t={t}: {}", g.trace(&session.host.world));
                }
                met_at = Some(t);
                break;
            }
            if t == crate::retail_compare_script::SCRIPT_RESUME_TICK {
                // The record's own latches, held back from the entry, are
                // retail's state again once the entry has run - the
                // settle-window sample included; a resume takes them back
                // before it replays the record.
                for idx in retail.seed_latches() {
                    session.host.world.system_flag_set(idx);
                }
            }
            if t == SETTLE_TICKS {
                at_settle = Some(sample_engine(&mut session, retail, &director, None));
            }
            // The walkers stand where retail's left them before a talk is
            // engaged: the talk snap turns a placement to the bearing from
            // its seat, and a wanderer's seat is `rand()` history
            // (`town01_npc16_dialogue_first_page`'s `P1[16]` wandered off
            // its `4C 51` tile before the press).
            if t == crate::retail_compare_script::SCRIPT_RESUME_TICK {
                for w in &retail.walkers {
                    session
                        .host
                        .world
                        .seed_ambient_walker(w.flat, w.x, w.z, w.heading);
                }
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
    let player_facing = world.player_actor_slot.and_then(|s| {
        world
            .actors
            .get(s as usize)
            .map(|a| a.move_state.render_26.wrapping_sub(0x800))
    });
    let n0 = session
        .host
        .scene
        .as_ref()
        .and_then(|s| s.field_man_payload(&session.host.index).ok().flatten())
        .and_then(|man| legaia_asset::man_section::parse(&man).ok())
        .map(|mf| mf.header.partition_counts[0].max(0) as u16);
    let world = &session.host.world;
    let npc_facings = n0
        .map(|n0| {
            world
                .npcs
                .positions
                .iter()
                .map(|(&slot, &(x, z))| {
                    let h = world.npcs.heading(slot);
                    (n0 + u16::from(slot), (h.wrapping_sub(0x800), x, z))
                })
                .collect()
        })
        .unwrap_or_default();
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
        player_facing,
        npc_facings,
    }
}

/// Ticks between two scripted pad edges of a menu drive: long enough for
/// the menu's open / hand-off beats, which swallow the edge that caused them.
pub const MENU_PRESS_GAP: u64 = 12;

/// Extra ticks the first press after `Start` waits: the field darkens for up
/// to fourteen ticks (`0x83 / 10` at a frame step of `1`) before the menu
/// exists and takes a press (`BootSession::pause_wipe`, retail
/// `FUN_801ED308` phase 1), longer than one [`MENU_PRESS_GAP`].
pub const MENU_OPEN_WIPE_TICKS: u64 = MENU_PRESS_GAP;

/// The pad edges that reach `menu` from a settled field, as `(tick offset,
/// button)` pairs from the first press: `Start`, `Down` until the root
/// cursor (opening on row `0`) sits on the row, `Cross`. The Equip row opens
/// on its character picker (`0x12`); the slot browse (`0x13`) is one more
/// `Cross`, and the candidate list (`0x14`) a `Down` past Best Equipment and
/// a `Cross` after that.
pub fn pause_menu_presses(menu: &RetailMenu) -> Vec<(u64, legaia_engine_core::input::PadButton)> {
    use legaia_engine_core::input::PadButton;
    let mut out = vec![(0, PadButton::Start)];
    let mut t = MENU_OPEN_WIPE_TICKS;
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
    "facing",
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

/// A placement standing farther than this from its retail position is a
/// position miss, not a facing one: its heading is not scored.
const FACING_SEAT_RADIUS: f64 = 96.0;

/// The facing channel: the player's heading and every placement the engine
/// holds near its retail position, each wrapped-angle delta on its own
/// falloff (an eighth turn and more scores zero). Mismatches beyond a
/// sixteenth turn are listed by flat index with both headings.
pub(crate) fn facing_score(retail: &RetailObs, engine: &EngineObs) -> Option<(f64, String)> {
    let part = |r: i16, e: i16| falloff(angle_delta(r, e), 32.0, 512.0);
    let mut parts = Vec::new();
    let mut miss = Vec::new();
    if let (Some(r), Some(e)) = (retail.player_facing, engine.player_facing) {
        parts.push(part(r, e));
        if angle_delta(r, e) > 256.0 {
            miss.push(format!("player r={:#05x} e={:#05x}", r & 0xFFF, e & 0xFFF));
        }
    }
    let mut skipped = 0;
    let mut ambient = 0;
    let hide = legaia_engine_core::world::FIELD_OFFMAP_HIDE_XZ;
    let dump = std::env::var_os("LEGAIA_RC_FACING_DUMP").is_some();
    for a in &retail.actor_facings {
        let Some(&(e, ex, ez)) = engine.npc_facings.get(&a.flat) else {
            continue;
        };
        if dump {
            eprintln!(
                "facing {}: flat {}{} flags {:#010x} model {} r=({}, {}) {:#05x} e=({}, {}) {:#05x}",
                retail.scene,
                a.flat,
                if a.ambient { " (ambient)" } else { "" },
                a.flags,
                a.model,
                a.x,
                a.z,
                a.facing & 0xFFF,
                ex,
                ez,
                e & 0xFFF
            );
        }
        // A parked actor (the off-map seat) is not drawn: its heading is
        // not on screen.
        if (a.x, a.z) == (hide, hide) {
            continue;
        }
        let dx = f64::from(i32::from(ex) - i32::from(a.x));
        let dz = f64::from(i32::from(ez) - i32::from(a.z));
        if (dx * dx + dz * dz).sqrt() > FACING_SEAT_RADIUS {
            skipped += 1;
            continue;
        }
        // An ambient motion stream's heading is time and `rand()` history
        // since the entry - the actor's walk history, as the player's is
        // once the pad has turned it.
        if a.ambient {
            ambient += 1;
            continue;
        }
        parts.push(part(a.facing, e));
        if angle_delta(a.facing, e) > 256.0 {
            miss.push(format!(
                "flat {}{} at ({}, {}) model {} r={:#05x} e={:#05x}",
                a.flat,
                if a.flags & 0x0100_0000 != 0 {
                    " (party)"
                } else {
                    ""
                },
                a.x,
                a.z,
                a.model,
                a.facing & 0xFFF,
                e & 0xFFF
            ));
        }
    }
    if parts.is_empty() {
        return None;
    }
    let n = parts.len();
    let score = parts.iter().sum::<f64>() / n as f64;
    Some((
        score,
        format!(
            "{n} actors scored, {skipped} off their retail seat, {ambient} ambient; misses: [{}]",
            miss.join("; ")
        ),
    ))
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
    let mut footing_note = None;
    let mut put = |name: &str, score: f64, detail: String| {
        ch.insert(name.to_string(), round3(score));
        det.insert(name.to_string(), detail);
    };

    if retail.class == StateClass::Field
        && let Some((score, detail)) = facing_score(retail, engine)
    {
        put("facing", score, detail);
    }
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
        match retail.script_height {
            // A script holds the player's height: retail's `Y` is that, not
            // the floor, so there is no floor reading to score against.
            Some(h) => {
                footing_note = Some(format!(
                    "not scored: retail Y={} is script-held (+0x8E={h}); engine floor={floor}",
                    r[1]
                ));
            }
            None => {
                let d = f64::from(floor - i32::from(r[1]));
                put(
                    "footing",
                    falloff(d, 2.0, 128.0),
                    format!("retail footing={} engine floor={floor}", r[1]),
                );
            }
        }
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
    if let Some(note) = footing_note {
        det.insert("footing".to_string(), note);
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
    /// Only states whose label contains this substring; a comma separates
    /// several alternatives (any one matching keeps the state).
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
            && !f
                .split(',')
                .filter(|alt| !alt.is_empty())
                .any(|alt| entry.label.contains(alt))
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
        retail.seed_save().as_ref(),
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
                crate::retail_compare_script::ScriptGate::displayed_from_retail(&retail.scripts),
            );
            let frame = match gate {
                Some(g) => {
                    let mut env = vec![("LEGAIA_SCRIPT_GATE", g.to_env())];
                    env.extend(retail.seat_env(opts.out_dir, &entry.label));
                    if let Some(n) = retail.hud_countdown {
                        env.push(("LEGAIA_HUD_COUNTDOWN", n.to_string()));
                    }
                    if let Some(b) = &retail.camera_block {
                        env.push(("LEGAIA_SEAT_CAMERA_BLOCK", camera_block_env(b)));
                    }
                    if let Some([fx, fz]) = retail.seat_focus {
                        env.push(("LEGAIA_SEAT_FOCUS", format!("{fx},{fz}")));
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
                    retail.camera_block.as_ref(),
                    retail.seat_focus,
                    &retail.seat_env(opts.out_dir, &entry.label),
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
    // The first stream under which the fight is still on at the sample, its
    // opening reached a prompt without a surprise round the capture's own
    // history does not hold (`EngineBattle::surprise_opening`; an opening
    // capture is that round), and the drive / replayed cast reached the
    // capture's phase (`BATTLE_RNG_SEEDS`), first with the HP as read and
    // then with a party swing's victims revived
    // (`RetailBattle::action_victims`); a state nothing satisfies keeps the
    // first run.
    let mut first = None;
    let mut reached = None;
    let mut reached_victims = false;
    // A battle-end capture also wants retail's win pose: a run that reaches
    // the phase on another one is kept as the fallback while the remaining
    // streams are tried (`RetailBattle::win_pose`).
    let mut off_pose = None;
    let opening = battle.seed_plan() == crate::retail_compare_battle::SeedPlan::Opening;
    // A replayed cast tries its victims revived first: unlike a pad-driven
    // swing, a cast onto a corpse still reaches its phase, so the HP-as-read
    // run would always win and the kill would never be replayed.
    let revive: &[bool] = if battle.action_victims().is_empty() {
        &[false]
    } else if battle.seed_plan() == crate::retail_compare_battle::SeedPlan::Cast {
        &[true, false]
    } else {
        &[false, true]
    };
    'search: for &victims in revive {
        for &seed in &crate::retail_compare_battle::BATTLE_RNG_SEEDS {
            let mut run = crate::retail_compare_battle::run_engine_battle(
                opts.extracted,
                retail,
                battle,
                seed,
                victims,
            );
            // An aged action state the engine left younger than retail's:
            // the same stream again, sampled on that state's last tick.
            if let Ok(e) = &run
                && let Some(accum) = e.age_short
            {
                let mut aged = battle.clone();
                aged.span_gate = crate::retail_compare_battle::SpanGate::Age { accum };
                run = crate::retail_compare_battle::run_engine_battle(
                    opts.extracted,
                    retail,
                    &aged,
                    seed,
                    victims,
                )
                .map(|mut e| {
                    e.age_short = Some(accum);
                    e
                });
            }
            match run {
                Ok(e)
                    if e.mode == legaia_engine_core::world::SceneMode::Battle
                        && e.prompt_tick.is_some()
                        && (!e.surprise_opening || opening)
                        && e.driven != Some(None)
                        && e.inflight != Some(None) =>
                {
                    if battle.win_pose.is_some() && e.win_pose != battle.win_pose {
                        off_pose.get_or_insert(e);
                        continue;
                    }
                    reached = Some(e);
                    reached_victims = victims;
                    break 'search;
                }
                Ok(e) => {
                    first.get_or_insert(Ok(e));
                }
                Err(e) => {
                    first.get_or_insert(Err(e));
                    break 'search;
                }
            }
        }
    }
    // A driven action replays the pushes its capture already holds; run the
    // same stream once more from the ground the replay says retail started
    // on (`RetailBattle::undrift`), and keep it when it still reaches the
    // phase.
    let undrifted;
    let (battle, reached) = match reached
        .as_ref()
        .and_then(|e| battle.undrift(&e.ground_drift).map(|b| (b, e.rng_seed)))
    {
        Some((b, seed)) => match crate::retail_compare_battle::run_engine_battle(
            opts.extracted,
            retail,
            &b,
            seed,
            reached_victims,
        ) {
            // Kept only when it stands the combatants nearer retail's pairs
            // than the first run did - a replay whose push depends on where
            // it starts can land further off.
            Ok(e)
                if e.mode == legaia_engine_core::world::SceneMode::Battle
                    && e.driven.is_some_and(|d| d.is_some())
                    && e.age_short.is_none()
                    && crate::retail_compare_battle::ground_residual(battle, &b, &e)
                        < reached.as_ref().and_then(|r| {
                            crate::retail_compare_battle::ground_residual(battle, battle, r)
                        }) =>
            {
                undrifted = b;
                (&undrifted, Some(e))
            }
            _ => (battle, reached),
        },
        None => (battle, reached),
    };
    let engine = match reached.or(off_pose).map(Ok).or(first) {
        Some(Ok(e)) => e,
        Some(Err(e)) => {
            report.unseeded = format!("seeding failed: {e:#}");
            return;
        }
        None => unreachable!("BATTLE_RNG_SEEDS is not empty"),
    };
    // A re-run sampled on a shorter age: the image child walks the same
    // drive, and the detail names the gate the run used.
    let aged;
    let battle = match engine.age_short {
        Some(accum) => {
            let mut b = battle.clone();
            b.span_gate = crate::retail_compare_battle::SpanGate::Age { accum };
            aged = b;
            &aged
        }
        None => battle,
    };
    let image = battle_image(opts, entry, retail, battle, &engine, report);
    let (mut ch, mut det) =
        crate::retail_compare_battle::compare_battle(retail, battle, &engine, &entry.ram_injected);
    if let Some(img) = &image {
        ch.insert("image".into(), round3(img.within));
        det.insert(
            "image".into(),
            format!("mae={:.1} within={:.3} ({})", img.mae, img.within, img.note),
        );
    }
    // A state made on a patched disc replays that build's executable: what
    // the patch writes into a combatant (the shiny-Seru boost's `x135/100`
    // on a monster's maxima) is not retail behaviour, and a channel that
    // reads it says so instead of reading as an engine miss.
    if let Some(patch) = entry.resident_patch.as_deref() {
        for key in ["enemy_hp", "battle_party"] {
            if let Some(d) = det.get_mut(key) {
                d.push_str(&format!(
                    "; retail ran a patched executable (resident patch: {patch})"
                ));
            }
        }
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
    let mut env = vec![
        (
            "LEGAIA_BATTLE_STAGE",
            format!(
                "{},{}",
                battle.stage_variant,
                u8::from(battle.keep_backdrop_object_1)
            ),
        ),
        ("LEGAIA_BATTLE_RNG_SEED", engine.rng_seed.to_string()),
        // The headless seed settles the landed field before it seeds the
        // stream and arms the fight (`run_engine_battle`); so does the child.
        ("LEGAIA_BATTLE_SETTLE", SETTLE_TICKS.to_string()),
        // The mid-fight bars the headless seed put on its first battle tick.
        (
            "LEGAIA_BATTLE_BARS",
            crate::retail_compare_battle::bar_seeds_to_env(&engine.hp_seed),
        ),
    ];
    // The idle orbit is a clock: phase-align it to the retail instant when
    // retail's own orbit owns the yaw (the battle tick's prologue store,
    // gated on these command-flow bytes - `0x801D07AC..0x801D07CC`).
    if battle.orbit_owns_yaw() {
        env.push(("LEGAIA_BATTLE_ORBIT_YAW", retail.camera.yaw.to_string()));
    }
    env.push((
        "LEGAIA_BATTLE_CAMERA_OPTION",
        battle.camera_option.to_string(),
    ));
    // Retail's HUD glides had all landed: the frame shows every plate at its
    // rest seat whatever the replay's own seed-to-phase time was (a seat
    // seeded on its captured ground skips the approach retail spent the
    // sixteen-frame raise on).
    if battle.hud_glides_landed {
        env.push(("LEGAIA_SEAT_HUD_GLIDES_LANDED", "1".to_string()));
    }
    // Glides still in flight: each seated on the elapsed the displayed
    // frame shows (`HudGlideSeat`).
    if !battle.hud_glides.is_empty() {
        env.push((
            "LEGAIA_SEAT_HUD_GLIDES",
            crate::retail_compare_battle::HudGlideSeat::to_env(&battle.hud_glides),
        ));
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
    // A menu capture or any other action in flight is walked there through
    // the pad path, the same drive the headless seed ran, and captured the
    // first frame it holds.
    if let Some(drive) = battle.battle_drive() {
        if engine.driven == Some(None) {
            report.detail.insert(
                "image".into(),
                "not scored: the pad drive never reached the capture's phase headlessly".into(),
            );
            return None;
        }
        env.push(("LEGAIA_BATTLE_DRIVE", drive.to_env()));
        tick += crate::retail_compare_battle::DRIVE_DEADLINE;
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

    /// Lay this run's measurements over a prior baseline. A state this run
    /// seeded takes this run's channel set: a channel the run no longer
    /// measures on it (a class change, a channel retired) leaves the ratchet
    /// instead of failing every later check as "not measured". The channels
    /// in `unmeasured_by_run` (the image channel on a run without a display)
    /// are carried over, and so is every state outside the run (a
    /// `--filter`) or that the run could not seed.
    pub fn merged_over(self, prior: Option<Baseline>, unmeasured_by_run: &[&str]) -> Self {
        let Some(mut out) = prior else { return self };
        out.settle_ticks = self.settle_ticks;
        out.classes.extend(self.classes);
        for (label, mut chans) in self.states {
            if let Some(old) = out.states.get(&label) {
                for (ch, v) in old {
                    if unmeasured_by_run.contains(&ch.as_str()) {
                        chans.entry(ch.clone()).or_insert(*v);
                    }
                }
            }
            out.states.insert(label, chans);
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

    /// The walker seed survives its env form.
    #[test]
    fn walker_seeds_round_trip_through_their_env_form() {
        let w = vec![
            WalkerSeed {
                flat: 41,
                x: 3456,
                z: -12,
                heading: 0xC00,
            },
            WalkerSeed {
                flat: 7,
                x: 0,
                z: 16320,
                heading: 0,
            },
        ];
        assert_eq!(walkers_from_env(&walkers_env(&w)), w);
    }

    /// The morph-envelope seed survives its env form.
    #[test]
    fn morph_seeds_round_trip_through_their_env_form() {
        let m = vec![
            MorphSeed {
                flat: 7,
                weights: vec![0x1000, 0x0C5A, 0],
                done_mask: 0x8000_0003,
                env: 0x5015,
            },
            MorphSeed {
                flat: 0x45,
                weights: vec![0x10],
                done_mask: 0,
                env: 0x1000,
            },
        ];
        assert_eq!(morphs_from_env(&morphs_env(&m)), m);
    }

    /// The capture-alignment seeds survive their env forms.
    #[test]
    fn cell_fx_and_fog_seeds_round_trip_through_their_env_forms() {
        let fx = vec![legaia_engine_core::clut_cell_fx::ClutCellFx {
            rect: (0, 502, 16, 1),
            h_add: 2880,
            s_add: 0,
            v_add: -98,
            mode: 1,
            white: 256,
        }];
        assert_eq!(cell_fx_from_env(&cell_fx_env(&fx)), fx);
        let fog = vec![legaia_engine_core::fog_particles::FogParticle {
            age: 0x480,
            rate: 12,
            slot: 77,
            alive: true,
            vx: -3,
            vz: 5,
            x: -(40 << 11),
            z: 90 << 11,
            y: -0x60,
            grey: 0x5A,
        }];
        assert_eq!(fog_from_env(&fog_env(&fog)), fog);
        // Period 2 on a step-3 frame fires every tick; a countdown short of
        // the period fired on an earlier tick.
        assert_eq!(scroll_fires_within(2, 2, 3, 2), 2);
        assert_eq!(scroll_fires_within(8, 4, 2, 2), 0);
        assert_eq!(scroll_fires_within(8, 6, 2, 2), 1);
        // Rotating right/down undoes the scroller's left/up rotation.
        let t: Vec<u16> = (0..6).collect(); // 3 wide, 2 high
        assert_eq!(unrotate_rect(&t, 3, 2, 1, 1), vec![5, 3, 4, 2, 0, 1]);
        let rects = vec![((0x280, 0, 2, 2), vec![1, 2, 3, 0x8004])];
        assert_eq!(vram_rects_from_file(&vram_rects_file(&rects)), rects);
        assert!(fog_from_env("1,2,3").is_empty());
    }

    fn chans(kv: &[(&str, f64)]) -> BTreeMap<String, f64> {
        kv.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    /// A re-measured state takes the run's channel set (a retired channel
    /// leaves the ratchet), keeps the image score a display-less run did not
    /// take, and a state outside the run is untouched.
    #[test]
    fn a_bless_replaces_a_measured_states_channel_set() {
        let mut prior = Baseline::default();
        prior.states.insert(
            "arrival".into(),
            chans(&[("camera", 0.9), ("footing", 0.4), ("image", 0.7)]),
        );
        prior
            .states
            .insert("elsewhere".into(), chans(&[("camera", 0.5)]));
        let mut run = Baseline::default();
        run.states
            .insert("arrival".into(), chans(&[("camera", 0.8)]));
        let display_less = run.clone().merged_over(Some(prior.clone()), &["image"]);
        assert_eq!(
            display_less.states["arrival"],
            chans(&[("camera", 0.8), ("image", 0.7)])
        );
        assert_eq!(display_less.states["elsewhere"], chans(&[("camera", 0.5)]));
        let full = run.merged_over(Some(prior), &[]);
        assert_eq!(full.states["arrival"], chans(&[("camera", 0.8)]));
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
        // The first press waits out the pause wipe; the rest are one gap
        // apart.
        assert_eq!(presses[1].0, MENU_OPEN_WIPE_TICKS + MENU_PRESS_GAP);
        assert!(
            presses[1..]
                .windows(2)
                .all(|w| w[1].0 - w[0].0 == MENU_PRESS_GAP)
        );
    }
}
