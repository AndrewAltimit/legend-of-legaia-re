//! Encounter / formation installation + per-scene monster catalog and party
//! battle-stat seeding.

use super::*;

mod disc_table_install;
mod encounter_install;
mod encounter_tick;
mod party_abilities;
mod region_setup;
mod scripted_battles;

#[cfg(test)]
pub(crate) use encounter_tick::ENCOUNTER_JOLT_VSYNCS;
#[cfg(test)]
pub(crate) use encounter_tick::INTRO_INIT_AUDIO_TICK;
pub(crate) use encounter_tick::INTRO_STORE_LEAD_TICKS;
pub use scripted_battles::*;
