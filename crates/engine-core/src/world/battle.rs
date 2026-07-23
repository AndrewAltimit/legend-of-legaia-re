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
mod loop_driver;
mod monster_ai;
mod stats;
mod teardown;
mod tutorial;
mod validator_host;
