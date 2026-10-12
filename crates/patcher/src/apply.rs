//! High-level orchestration: read the current gameplay data off a disc, plan a
//! randomization from a seed, and write the plan back into a [`DiscPatcher`].
//!
//! This is the glue the top-level CLI drives. It keeps the per-module logic
//! (drop planning, slot re-pack, sector write-back) decoupled and testable while
//! giving the binary a single call per feature. It embeds no game bytes - every
//! value it reads comes from the user's own disc image at runtime.

pub(crate) use anyhow::{Context, Result};

pub(crate) use crate::casino::{self, CasinoExchange};
pub(crate) use crate::chest::SceneChests;
pub use crate::door::DoorSiteClass;
pub(crate) use crate::door::SceneDoors;
pub(crate) use crate::drops::{CurrentDrop, DropAssignment, DropMode, plan_drops};
pub(crate) use crate::encounter::SceneEncounters;
pub(crate) use crate::house_door::SceneHouseDoors;
pub(crate) use crate::map_door::SceneMapDoors;
pub(crate) use crate::monster_class;
pub(crate) use crate::monster_stats;
pub(crate) use crate::shop::SceneShops;
pub(crate) use legaia_disc_patch::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};
pub(crate) use legaia_disc_patch::rng::SplitMix64;

/// `SCUS_942.54` filename (the static-table container).
const SCUS_NAME: &str = "SCUS_942.54";

// Feature submodules. Each does `use super::*;` to reach the shared
// imports/const above and the other features' re-exported items; every
// previously-public `apply::*` path is preserved by the `pub use` glob
// re-exports below.
mod arts_name_fix;
mod arts_power;
mod battle_tuning;
mod chests;
mod code_hooks;
mod delilas;
mod doors;
mod drops;
mod earth_egg;
mod encounters;
mod enemy_hp_bar;
mod equipment_edit;
mod fortress_encounter_fix;
mod overlay;
mod place_names;
mod rewards;
mod shops_casino;
mod starting;
mod stats;
mod steals_arts;
mod super_art_list;
mod super_art_power;
mod super_arts_pack;

pub use arts_name_fix::*;
pub use arts_power::*;
pub use battle_tuning::*;
pub use chests::*;
pub use code_hooks::*;
pub use delilas::*;
pub use doors::*;
pub use drops::*;
pub use earth_egg::*;
pub use encounters::*;
pub use enemy_hp_bar::*;
pub use equipment_edit::*;
pub use fortress_encounter_fix::*;
pub use overlay::*;
pub use place_names::*;
pub use rewards::*;
pub use shops_casino::*;
pub use starting::*;
pub use stats::*;
pub use steals_arts::*;
pub use super_art_list::*;
pub use super_art_power::*;
pub use super_arts_pack::*;
