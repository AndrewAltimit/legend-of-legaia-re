//! Encounter / formation installation + per-scene monster catalog and party
//! battle-stat seeding.

use super::*;

mod disc_table_install;
mod encounter_install;
mod encounter_tick;
mod party_abilities;
mod region_setup;
mod scripted_battles;

pub use scripted_battles::*;
