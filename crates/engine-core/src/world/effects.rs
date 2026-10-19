//! Move bytecode/system flags/field-script loading, actor spawning, effect markers/sprites/models, summons, field stagers, and move FX.

use super::*;

mod clut_fx;
mod effect_pool;
mod field_stagers;
mod map_markers;
mod move_fx;
mod summon;
mod vm_entry;
mod vram_moves;

pub use clut_fx::*;
pub use effect_pool::*;
pub(crate) use move_fx::*;
pub use vram_moves::*;

/// Base index in [`World::global_tmd_pool`] (= `DAT_8007C018`)
/// where the PROT 0871 effect-model library registers. Its 30 models occupy
/// `[3..=32]`, overwriting the two trailing slots of the PROT 0874 §0 field
/// head (`[3]`, `[4]`) - exactly retail's temporal layout (the field head
/// seeds `[0..=4]`; battle init reloads `[3..=32]`).
///
/// This is the engine's analogue of the retail **battle `gp[0x754]` value** -
/// the additive base `FUN_80021B04` applies to a move-FX / summon part record's
/// `model_sel` (`DAT_8007C018[model_sel + gp[0x754]]`). In retail that base is
/// *not* a constant: it is `party_count + 2` (the two fixed pool slots + the live
/// party-character meshes precede the library), i.e. `3` for the 1-member
/// training party and `5` for the full 3-member party - save-corpus-pinned by
/// `crates/mednafen/tests/summon_model_base.rs` (see `docs/formats/move-power.md`).
/// The engine instead registers the library at a *fixed* `[3..=32]` and keeps
/// `model_sel` library-relative, so `model_sel + 3` lands on the same library
/// model retail reaches via `model_sel + gp[0x754]` - the library content is
/// identical, only its pool offset shifts with party size, so the two layouts are
/// equivalent. `World::spawn_move_fx` uses this fixed base.
pub const EFFECT_MODEL_LIBRARY_BASE: usize = 3;
