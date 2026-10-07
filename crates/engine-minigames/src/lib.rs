//! The minigame rules engines of the engine port: headless state machines
//! driven by disc-parsed tables, with no `World`, no scene loading and no
//! renderer.
//!
//! `legaia-engine-core` depends on this crate and re-exports every module at
//! its old path (`legaia_engine_core::slot_machine`, ...), so hosts keep
//! naming the paths they always did. What stays in `engine-core`, and why,
//! is in this crate's `README.md`.

#![forbid(unsafe_code)]

pub mod baka_cabinet;
pub mod baka_duel;
pub mod baka_fighter;
pub mod baka_fighter_chrome;
pub mod baka_impact_fx;
pub mod minigame_actor;
pub mod other_game_overlay;
pub mod slot_machine;
