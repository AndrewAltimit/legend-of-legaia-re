//! The disc patcher's Tactical Arts layer: every edit whose subject is an
//! art - its button combo, its power, the AP it grants and costs, and the
//! Super Art list, menu and power tables.
//!
//! This crate ships only *code*. It never embeds game bytes: the data edits
//! mutate a player battle file's `record[0]` handed in as bytes, the code
//! mods are assembled from the `legaia-code-hooks` encoders and seated in its
//! arena map, and every test that needs real game data is disc-gated in
//! `legaia-patcher`.
//!
//! It names `legaia-art` (the arts tables and tokenizer), `legaia-game-tables`,
//! `legaia-lzs`, `legaia-iso`, `legaia-code-hooks` and `legaia-disc-patch`,
//! and no other feature crate. See the crate README for the module map.

#![forbid(unsafe_code)]

pub mod arts;
pub mod arts_ap_grant;
pub mod arts_power;
pub mod oscillating_ap;
pub mod super_art_list;
pub mod super_art_menu;
pub mod super_art_power;
