//! Battle command flow, submenu ticks, monster AI, initiative, capture
//! resolution, and battle teardown. Split out of `world.rs` as additional
//! `impl World` blocks.

use super::*;

use crate::battle_events::{BattleEvent, BattleHitFx};
use legaia_engine_vm as vm;
use vm::battle_action::{BattleEndCause, StepOutcome};

mod capture;
mod casting;
mod command_flow;
mod initiative;
mod locomotion;
mod loop_driver;
mod monster_ai;
mod stats;
mod teardown;
mod tutorial;
mod validator_host;

pub use teardown::BattleSpoilsBanner;

/// The staged command id a generic physical swing runs as.
///
/// Retail has no "generic attack": every melee hit is one of the four
/// direction commands `0x0C..=0x0F` the Arts input queued, and both the
/// per-command power scalar (`0x801F64EC[(id - 0x0C) % 5]`) and the
/// UDF-vs-LDF defence pick (`(id - 0x0C) % 10 < 5`) are keyed on that id.
/// The engine's [`World::apply_basic_attack`] is one un-chained swing, so it
/// runs as the **arm** command `0x0C` - the cheapest of the four and the one
/// the gauge deals first.
pub(in crate::world) const BASIC_ATTACK_COMMAND: u8 = 0x0C;
