//! Composite actor + scene runtime that wires the per-VM hosts together.
//!
//! `legaia-engine-vm` ships each script VM (actor / sprite, move-table,
//! effect, field, battle action) as a small clean-room port + a `Host` trait
//! that lets engines plug in their own state. This module is the engine-side
//! glue: a single [`World`] that owns the per-actor data and implements every
//! VM `Host` trait by routing into that data.
//!
//! ## Why a single composite
//!
//! In the retail runtime, "an actor" is a 0xCB-byte record holding everything
//! all four VMs read/write - world position, anim banks, flags, render bank,
//! per-action queue, etc. Splitting that across four crates would force
//! engines to keep four parallel index tables in sync. The composite pattern
//! here keeps the per-VM `ActorState` structs intact (clean-room boundary
//! preserved) but lets one struct own them.
//!
//! Engines that want a different layout - say, ECS storage - should
//! implement the VM `Host` traits themselves; this is the default.
//! REF: FUN_8001E890, FUN_80021DF4, FUN_80026B4C, FUN_8003CA38, FUN_8003CE08, FUN_800520F0
//! REF: FUN_801D65D8, FUN_801D77F4, FUN_801D8DE8, FUN_801DE840, FUN_801DFDF8
//!
//! PORT: FUN_800467E8 (`world_map_camera_relative_bits` - held-pad camera-yaw
//!       remap; the engine reads the world-map camera azimuth directly from
//!       [`WorldMapController`] rather than the retail `gp+0x2D8` quadrant.)

use std::sync::Arc;

use crate::battle_events::{BattleEvent, BattleHitFx, BattleSfxCue};
use crate::field_events::FieldEvent;
use crate::input;
use crate::levelup::{LevelUpBanner, LevelUpResult, LevelUpTracker};
use crate::man_field_scripts::WalkTouchEvent;
use crate::move_buffer_host;
use crate::tactical_arts::{ArtLearnedBanner, TacticalArtsTracker};
use crate::world_map::WorldMapController;
pub use legaia_anm::{AnimPlayer, PoseFrame};
use legaia_asset::monster_archive::MonsterAnimation;
use legaia_engine_vm as vm;
use legaia_save;
use vm::Position as ActorVmPosition;
use vm::actor_tick::{ActorPhysics, ListenerState, TickEvent, TickResult, TickScalars};
use vm::battle_action::{BattleActionCtx, BattleActor, BattleEndCause, StepOutcome};
use vm::effect_vm::Pool;
use vm::field::{CameraParam, FieldCtx, StepResult as FieldStepResult};
use vm::move_buffer::{MoveBufferState, cursor_advance};
use vm::move_vm::ActorState as MoveActorState;

use vm_hosts::{
    ActorVmHostImpl, BattleHostImpl, EffectHostImpl, FieldCarrierHostImpl, FieldHostImpl,
    MoveVmHostImpl, WorldMapEntityHostImpl,
};

mod config;
mod state;
mod types;

pub use config::*;
pub use state::*;
pub use types::*;

mod actors;
mod assets_events;
mod battle;
mod effects;
pub use effects::{
    ClutCellFx, ClutCellFxPhase, DEBUG_EFFECT_LIFETIME_FRAMES, MAX_DEBUG_EFFECTS, ScriptVramMove,
};
mod encounters;
pub use encounters::FieldBossStager;
mod field_carriers;
pub mod field_elevation;
pub use field_elevation::{CELL_ELEVATION_OVERRIDE, ElevationOverride};
mod field_loop;
mod field_movement;
mod frame_tick;
mod items_arts;
mod narration;
mod prop_interact;
mod save;
mod vm_hosts;
mod worldmap;

#[cfg(test)]
mod tests;
