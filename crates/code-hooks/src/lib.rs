//! The disc patcher's MIPS code-injection toolkit: the instruction encoders
//! and the R3000 subset simulator every hand-assembled routine is built and
//! tested with, the shared injection-arena map ([`arenas`]), and the hook mods
//! whose builders close over nothing but those.
//!
//! This crate ships only *code*. It never embeds game bytes: every routine is
//! assembled from the encoders here, and every test that needs real game data
//! is disc-gated in `legaia-patcher`.
//!
//! A hook here is a *plan*: it reads image bytes handed in as slices and
//! returns the same-size [`arenas::Edit`]s to write. Nothing in this crate
//! opens a disc, so it depends on `legaia-game-tables` and
//! `legaia-actor-formats` (table addresses) and on no other patcher crate. See the crate README for the module map and the rule
//! that decides what sits here.

#![forbid(unsafe_code)]

pub mod approach_fix;
pub mod arenas;
pub mod bonus_drop;
pub mod conkram_freeze_fix;
pub mod enemy_hp_bar;
pub mod flee_exp;
pub mod jewel_fix;
pub mod mips;
pub mod mips_sim;
pub mod seru_overlay;
pub mod seru_trade;
pub mod shiny_seru;
