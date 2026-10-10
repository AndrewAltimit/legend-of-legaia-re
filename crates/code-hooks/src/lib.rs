//! The disc patcher's MIPS code-injection layer: the instruction encoders and
//! the R3000 subset simulator every hand-assembled routine is built and
//! tested with, the injection arenas, and the hook mods whose builders close
//! over nothing else.
//!
//! This crate ships only *code*. It never embeds game bytes: every routine is
//! assembled from the encoders here, and every test that needs real game data
//! is disc-gated in `legaia-patcher`.
//!
//! Every module names only its siblings, `legaia-asset` (table addresses and
//! the monster archive stride), `legaia-lzs` and `legaia-disc-patch`, so the
//! crate sits below `legaia-patcher`, which re-exports each module at its old
//! path. See the crate README for the module map and the split line.

#![forbid(unsafe_code)]

// Modules these files name as `crate::...`, which `legaia-patcher` re-exports
// at its root; binding them here keeps the moved files' paths unchanged.
use legaia_disc_patch::disc;
#[allow(unused_imports)] // named by rustdoc links only
use legaia_disc_patch::space_ledger;

pub mod approach_fix;
pub mod bonus_drop;
pub mod delilas_cast;
pub mod enemy_hp_bar;
pub mod flee_exp;
pub mod item_name;
pub mod jewel_fix;
pub mod mips;
pub mod mips_sim;
pub mod monster;
pub mod seru_overlay;
pub mod seru_trade;
pub mod shiny_seru;
