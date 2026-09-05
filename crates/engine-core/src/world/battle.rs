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
mod stage;
mod stats;
mod teardown;
mod tutorial;
mod validator_host;
mod victory;

pub use teardown::BattleSpoilsBanner;
pub use victory::{
    LEVEL_UP_CUE, VICTORY_EXIT_PHASE, VICTORY_FADE_PHASE_SEED, VICTORY_LOAD_FRAMES,
    VICTORY_RESULTS_HOLD_FRAMES, VictoryPhase, VictorySequence, victory_pose_column,
    victory_pose_id, victory_pose_tier,
};

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

/// The sound cue a landed melee swing submits - `li a0,0x10c` at
/// `0x801EEBD8`, the one `jal 0x8004fe5c` in the melee kernel
/// `FUN_801EC3E4`. See [`World::apply_one_basic_strike`]'s cue arm for what
/// each of the funnel's two legs does with it.
pub(in crate::world) const MELEE_IMPACT_CUE: u32 = 0x10C;

/// Clip slot of the per-character melee grunt bank - `XA30.XA` (`li a0,0x1d`
/// at `0x801EEB18` / `0x801EEB28` / `0x801EEB38` of `FUN_801EC3E4`).
pub(in crate::world) const GRUNT_CLIP_SLOT: u32 = 0x1D;

/// Attacker element the SFX funnel's tinted leg writes when the engine can
/// resolve none - retail's non-elemental id, the same `7` the melee damage
/// path passes as `attacker_element`.
pub(in crate::world) const NEUTRAL_ELEMENT: u8 = 7;
