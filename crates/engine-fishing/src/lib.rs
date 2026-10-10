//! The fishing minigame's rules engine: the pond session, its rod / lure /
//! line actors, the venue chrome, the hub screen and the venue floor grid.
//! Headless state machines driven by disc-parsed tables, with no `World`, no
//! scene loading and no renderer.
//!
//! These modules name only each other, `legaia-asset`, `legaia-engine-vm`
//! and `legaia-tmd` - no other minigame - so the crate sits below
//! `legaia-engine-minigames`, which re-exports each module at its old path
//! (and `legaia-engine-core` re-exports those in turn). Doc links that
//! pointed up at a dependent crate are plain code spans, since rustdoc cannot
//! resolve a link into a dependent crate. See the crate README.

#![forbid(unsafe_code)]

pub mod fishing;
pub mod fishing_actors;
pub mod fishing_chrome;
pub mod fishing_hub;
pub mod minigame_floor;
