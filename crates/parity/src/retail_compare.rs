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
//! ([`legaia_engine_session::native::NativeSession::resume_save`] over the lifted block), then the
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
use legaia_engine_capture::SETTLE_TICKS;
use legaia_engine_capture::field_seeds::*;
use legaia_engine_core::world::SeededVramRect;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene_host::scene_host::BgmDirector;
use legaia_mednafen::{PsxGpu, SaveState as MednafenState};
use legaia_ram_image::ScenarioManifest;
use legaia_ram_image::game_anchors;
use serde::{Deserialize, Serialize};

use crate::retail_compare_image::{Frame, ImageScore};
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
mod seeds;
pub use seeds::*;
mod scoring;
pub use scoring::*;
mod battle_run;
pub use battle_run::*;

mod baseline_report;
mod corpus_run;
mod engine_run;
mod pause_menu_drive;
pub mod resident_disc;
mod retail_obs;
mod save_flags;
#[cfg(test)]
mod tests;

pub use baseline_report::*;
pub use corpus_run::*;
pub use engine_run::*;
pub use pause_menu_drive::*;
pub use retail_obs::*;
pub use save_flags::*;

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
/// the view orbits, stored **negated** (`engine-core::world::camera_world`, axes 6 / 8).
const CAM_FOCUS: u32 = 0x8008_9118;
const BGM_ID: u32 = 0x8007_BAC8;
/// The destination-entry operand `_DAT_80073EF4` / `_DAT_80073EF8`: the
/// world `(X, Z)` every field entry seats the player from (the MAN loader
/// `FUN_8003AEB0` copies a card load's position snapshot into the pair,
/// `0x8003B764..0x8003B798`; the facing word follows at [`ARRIVAL_FACING`]).
const ENTRY_OPERAND: u32 = 0x8007_3EF4;
/// The region-type mask `_DAT_8007B8F4` (`FUN_800180EC`'s rebuild, the word
/// field-VM op `0x42` mode 0 tests).
const REGION_MASK: u32 = 0x8007_B8F4;
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
/// is `+6..+0x28`, [`legaia_engine_field::camera_zone::CameraZoneConfig::from_retail_block`]).
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
            0x03 if legaia_engine_scene::scene::is_world_map_scene(scene) => StateClass::WorldMap,
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
    /// The scenario's `resident_disc` - the patched image's file name, which
    /// the engine side runs on when it is found ([`resident_disc`]).
    pub resident_disc: Option<String>,
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
            let Some(path) = legaia_ram_image::scenarios::library_backup_for(emulator, library, fp)
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
                    resident_disc: sc.resident_disc.clone(),
                    ram_injected: sc.ram_injected.clone(),
                });
            }
        }
    }
    out
}

/// Put a headless seed's world on the retail-faithful mode its image child
/// runs. The child is `play-window --retail`, whose session switches end in
/// [`legaia_engine_core::world::WorldToggles::pin_retail`]
/// (`SessionSwitches::apply`); a seed left on the engine's defaults keeps the
/// port's tweaks and guards on (the shiny roll's battle-entry `rand()` draw,
/// the HP-readout and hop-landing guards, the Fortress encounter fix), so the
/// two halves would replay different fights off one stream. A state made on
/// a shiny-Seru build keeps that build's entry draw on both halves
/// (`RetailBattle::resident_shiny_seru`); no other resident patch has an
/// engine switch behind it.
pub fn pin_seed_retail(
    toggles: &mut legaia_engine_core::world::WorldToggles,
    resident_shiny_seru: bool,
) {
    toggles.pin_retail();
    if resident_shiny_seru {
        toggles.shiny_seru = true;
    }
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
