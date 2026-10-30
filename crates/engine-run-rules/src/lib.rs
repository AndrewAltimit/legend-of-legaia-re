//! Run rules: the run-wide twists a player picks at New Game - difficulty
//! presets, New Game+, ironman, the in-engine randomizer and Boss Rush - as
//! `World`-free kernels.
//!
//! [`rules::RunRules`] is what a run plays under and what its saves carry
//! ([`save::RunSave`]); its default is the retail game, and every engine
//! reader folds it in as an identity at that default. [`setup::RulesSetup`]
//! is the New Game rules screen both hosts draw, [`boss_rush`] the Boss Rush
//! run and its intermission, [`scaling`] New Game+'s enemy curve. The
//! randomizer's planning logic is the disc patcher's
//! (`legaia-randomizer`); the engine applies its picks at the readers
//! instead of writing a disc. The `World` side is
//! `legaia_engine_core::world::run_rules`. See `docs/subsystems/run-rules.md`.

#![forbid(unsafe_code)]

pub mod boss_rush;
pub mod rules;
pub mod save;
pub mod scaling;
pub mod setup;
