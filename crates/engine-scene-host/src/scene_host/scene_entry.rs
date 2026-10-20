//! `SceneHost` field/world-map scene entry, shop population, archive/move-power seeding, and per-frame tick.
//!
//! Each child is one `impl SceneHost` block: `field_scene` and `world_map_scene` enter a
//! scene, `battle_inputs` seeds what a battle needs, `entry_seat` places the player,
//! `walk_on_triggers` dispatches tile triggers, `tick` is the per-frame step and
//! `fmv_handoff` the post-movie control transfer.

use super::*;

use legaia_engine_system::movie::cutscene::FmvHandoff;

mod battle_inputs;
mod entry_seat;
mod field_scene;
mod fmv_handoff;
mod tick;
mod walk_on_triggers;
mod world_map_scene;

pub use entry_seat::PickerSeat;
pub use fmv_handoff::*;

/// Extraction index of `bse.dat`, the battle runtime SFX descriptor bank (raw
/// TOC `0x37A`, `docs/formats/bse-dat.md`).
const BSE_BANK_PROT_ENTRY: u32 = 888;
