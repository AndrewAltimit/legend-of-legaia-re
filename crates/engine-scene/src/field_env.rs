//! Shared field/world **environment-geometry draw resolver**.
//!
//! A field scene's static geometry is composed from two sources that live in
//! different assets:
//!
//! - the **environment mesh pack**: the scene-owned PROT entry whose LZS
//!   sections carry the scene's object-local Legaia TMDs (buildings, props,
//!   terrain-decor tiles) - the `scene_asset_table` type-2 `Tmd` descriptor;
//! - the **placements**: the field `.MAP` object grid
//!   ([`legaia_scene_formats::field_objects`], retail `FUN_8003A55C`), whose records
//!   select a pack mesh (`+0x10`) and give it a world transform, with world Y
//!   resolved through the MAN header's 16-entry floor-height LUT.
//!
//! This module is the platform-independent kernel both renderers share: the
//! native play-window (`engine-shell`, which maps the draws onto wgpu model
//! matrices) and the WASM web viewer (which streams them to a WebGL assembled
//! view). It resolves *which* [`SceneResources`] TMD each placement draws and
//! *where*, leaving mesh upload / matrix conventions to the caller.
//!
//! A placed object may additionally be **bound** to a MAN partition-0 record (see
//! [`object_binds`]). The bind is not a spawn gate - retail's two placed-object
//! sweeps between them create every placed record - but it does decide, when the
//! object's mesh has more than one TMD object, which animation clip poses those
//! objects.
//!
//! REF: FUN_8003A55C (object-grid walk), FUN_801D7B50 (window rebuild),
//! REF: FUN_8003AEB0 (floor-LUT install)
//! REF: FUN_801F69D8, FUN_801F7088 (per-cell terrain emitters: corner-average Y)

use crate::scene::Scene;
use crate::scene_resources::SceneResources;
use legaia_engine_field::field_regions::{self, TileTrigger};
use legaia_scene_formats::field_objects::Placement;
use legaia_scene_formats::man_section::ManFile;
use std::collections::{HashMap, HashSet};

#[cfg(test)]
mod anim_tests;
mod draws;
mod floor;
#[cfg(test)]
mod live_placed_tests;
mod object_binds;
mod placed;
mod prop_anim;
mod prop_anim_bank;
mod prop_program;
#[cfg(test)]
mod tests;

pub use draws::*;
pub use floor::*;
pub use object_binds::*;
pub use placed::*;
pub use prop_anim::*;
pub use prop_anim_bank::*;
pub use prop_program::*;
