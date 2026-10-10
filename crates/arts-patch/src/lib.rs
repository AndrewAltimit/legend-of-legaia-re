//! The disc patcher's Tactical Arts layer: every edit whose subject is an
//! art - its button combo, its power, the AP it grants and costs, and the
//! Super Art list, menu and power tables.
//!
//! This crate ships only *code*. It never embeds game bytes: the data edits
//! mutate a player battle file's `record[0]` handed in as bytes, the code
//! mods are assembled from the `legaia-code-hooks` encoders, and every test
//! that needs real game data is disc-gated in `legaia-patcher`.
//!
//! Every module names only its siblings, `legaia-art` (the arts tables and
//! tokenizer), `legaia-asset`, `legaia-lzs`, `legaia-iso`, `legaia-code-hooks`
//! and `legaia-disc-patch`, so the crate sits below `legaia-patcher`, which
//! re-exports each module at its old path. Doc links that pointed up at it
//! are plain code spans, since rustdoc cannot resolve a link into a dependent
//! crate. See the crate README for the module map and the split line.

#![forbid(unsafe_code)]

// Modules these files name as `crate::...`, which `legaia-patcher` re-exports
// at its root; binding them here keeps the moved files' paths unchanged.
use legaia_code_hooks::{mips, seru_overlay, shiny_seru};
use legaia_disc_patch::rng;

#[cfg(test)]
use legaia_code_hooks::mips_sim;

pub mod arts;
pub mod arts_ap_grant;
pub mod arts_power;
pub mod oscillating_ap;
pub mod super_art_list;
pub mod super_art_menu;
pub mod super_art_power;
