//! The minigame rules engines of the engine port: headless state machines
//! driven by disc-parsed tables, with no `World`, no scene loading and no
//! renderer.
//!
//! `legaia-engine-core` depends on this crate. What stays in `engine-core`,
//! and why, is in this crate's `README.md`. The fishing rules engine is
//! `legaia-engine-fishing`, one crate down.

#![forbid(unsafe_code)]

pub mod baka_cabinet;
pub mod baka_duel;
pub mod baka_fighter;
pub mod baka_fighter_chrome;
pub mod baka_impact_fx;
pub mod dance;
pub mod dance_tutorial;
pub mod minigame_actor;
pub mod minigame_fx;
pub mod muscle_dome;
pub mod muscle_ringside;
pub mod other_game_overlay;
pub mod prize_exchange;
pub mod slot_machine;
pub mod tile_board;
pub mod tile_board_state;
