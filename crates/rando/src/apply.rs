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
pub(crate) use crate::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};
pub(crate) use crate::door::SceneDoors;
pub(crate) use crate::drops::{CurrentDrop, DropAssignment, DropMode, plan_drops};
pub(crate) use crate::encounter::SceneEncounters;
pub(crate) use crate::house_door::SceneHouseDoors;
pub(crate) use crate::monster_stats;
pub(crate) use crate::rng::SplitMix64;
pub(crate) use crate::shop::SceneShops;

/// `SCUS_942.54` filename (the static-table container).
const SCUS_NAME: &str = "SCUS_942.54";

// Feature submodules. Each does `use super::*;` to reach the shared
// imports/const above and the other features' re-exported items; every
// previously-public `apply::*` path is preserved by the `pub use` glob
// re-exports below.
mod battle_tuning;
mod chests;
mod code_hooks;
mod doors;
mod drops;
mod encounters;
mod overlay;
mod shops_casino;
mod starting;
mod stats;
mod steals_arts;

pub use battle_tuning::*;
pub use chests::*;
pub use code_hooks::*;
pub use doors::*;
pub use drops::*;
pub use encounters::*;
pub use overlay::*;
pub use shops_casino::*;
pub use starting::*;
pub use stats::*;
pub use steals_arts::*;
