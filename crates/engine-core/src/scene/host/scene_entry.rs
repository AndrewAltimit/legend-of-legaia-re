//! `SceneHost` field/world-map scene entry, shop population, archive/move-power seeding, and per-frame tick.
//!
//! Extracted verbatim from `scene/host.rs` as an additional `impl SceneHost` block.

use super::*;

use crate::cutscene::FmvHandoff;

mod battle_inputs;
mod entry_seat;
mod field_scene;
mod fmv_handoff;
mod tick;
mod walk_on_triggers;
mod world_map_scene;

pub use fmv_handoff::*;

/// Extraction index of `bse.dat`, the battle runtime SFX descriptor bank (raw
/// TOC `0x37A`, `docs/formats/bse-dat.md`).
const BSE_BANK_PROT_ENTRY: u32 = 888;
