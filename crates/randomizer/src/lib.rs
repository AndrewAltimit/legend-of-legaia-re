//! The randomizer's planning logic, shared by the disc patcher and the
//! engine.
//!
//! Every planner here is a pure function of `(current values, pool, seed,
//! mode)`: it decides *what* each slot should hold and never reads or writes
//! a disc. `legaia-patcher` applies a plan as same-size disc edits; the
//! engine's run rules (`legaia-engine-run-rules`) apply the same plan as
//! reader-side overrides, so one seed string names one randomization in
//! both.
//!
//! The generator is [`rng::SplitMix64`], fixed forever by its constants, so a
//! published seed reproduces on any machine and build.

#![forbid(unsafe_code)]

pub mod bands;
pub mod drops;
pub mod encounters;
pub mod items;
pub mod monsters;
pub mod rng;
pub mod sites;
