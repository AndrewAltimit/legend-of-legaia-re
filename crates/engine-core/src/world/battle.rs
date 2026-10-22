//! Battle command flow, submenu ticks, monster AI, initiative, capture
//! resolution, and battle teardown.

use super::*;

use legaia_engine_battle::battle_events::{BattleEvent, BattleHitFx};
use legaia_engine_battle_vm::battle_action::{BattleEndCause, StepOutcome};
use legaia_engine_vm as vm;

mod actor_draw;
mod auto_combo;
mod auto_command;
mod camera_ghost;
mod capture;
mod cast_band;
mod casting;
pub(in crate::world) mod clip_ladder;
mod command_flow;
mod commit_log_launch;
mod counterattack;
mod effect_route;
mod effect_teardown;
mod fatal_decision;
mod formation_span;
mod frame_clock;
mod homing;
mod initiative;
mod intro_names;
mod locomotion;
pub(in crate::world) use locomotion::stream_pose;
mod loop_driver;
mod member_step;
mod message_banner;
mod monster_ai;
mod selectable;
mod seru_absorb;
mod sideband;
mod stage;
mod stats;
mod steal_attack;
mod summon_seat;
mod teardown;
mod tutorial;
mod validator_host;
mod victory;

pub use actor_draw::{BATTLE_BODY_NCLIP, BattleActorDrawPlan, PARTY_BODY_RADIUS};
pub use auto_combo::{AutoComboInputs, AutoComboState};
pub use cast_band::{
    CASTER_STAGE_TICK_LIMIT, CasterStagePhase, CasterStageRun, ModulePresentation, PendingCast,
    SUMMON_SPAWN_BEHIND, SUMMON_STRIKE_BEHIND, SummonPhase, SummonStager,
};
pub(in crate::world) use commit_log_launch::BATTLE_PASS_STEP_PER_TICK;
pub use effect_route::RoutedEffectSpawn;
pub use frame_clock::{BattleFrameClock, DEFAULT_BATTLE_FRAME_STEP};
pub use message_banner::{
    ABSORB_BANNER_ELEMENT, BattleMessageBanner, COUNTER_MESSAGE_HOLD, COUNTER_MESSAGE_VA,
    MAGIC_LEVEL_BANNER_ELEMENT, TIMED_MESSAGE_ELEMENT,
};
pub use teardown::{BattleDefeatBanner, BattleSpoilsBanner};

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
/// `FUN_801EC3E4`. See `World::land_melee_hit`'s cue arm for what each of
/// the funnel's two legs does with it.
pub(in crate::world) const MELEE_IMPACT_CUE: u32 = 0x10C;

/// Clip slot of the per-character melee grunt bank - `XA30.XA` (`li a0,0x1d`
/// at `0x801EEB18` / `0x801EEB28` / `0x801EEB38` of `FUN_801EC3E4`).
pub(in crate::world) const GRUNT_CLIP_SLOT: u32 = 0x1D;
