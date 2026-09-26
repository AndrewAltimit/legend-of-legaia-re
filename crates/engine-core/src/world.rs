//! Composite actor + scene runtime that wires the per-VM hosts together.
//!
//! `legaia-engine-vm` ships each script VM (actor / sprite, move-table,
//! effect, field, battle action) as a small port + a `Host` trait
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
//! here keeps the per-VM `ActorState` structs intact (port boundary
//! preserved) but lets one struct own them.
//!
//! Engines that want a different layout - say, ECS storage - should
//! implement the VM `Host` traits themselves; this is the default.
//!
//! ## Layout
//!
//! [`World`] itself keeps the VM contexts, the actor table and the
//! scene-flow latches as direct fields. Everything else is grouped one
//! plain data struct per subsystem, each in its own `world/*_state.rs`
//! file and reached as `world.<group>.<field>`: [`PartyState`],
//! [`BattleState`], [`EncounterState`], [`SeruState`], [`CastFxState`],
//! [`FieldTerrain`], [`FieldLocomotion`], [`FieldPropState`],
//! [`FieldNpcState`], [`FieldVmState`], [`DialogState`], [`CutsceneState`],
//! [`WorldMapState`], [`FieldCarrierState`], [`MinigameState`],
//! [`ShopState`], [`MenuState`], [`TileBoardState`], [`CameraRig`],
//! [`ScreenFxState`], [`AmbientFxState`], [`AudioState`],
//! [`MoveVmGlobals`], [`FrameClock`], [`DiscTables`], [`StoryFlagState`]
//! and [`WorldToggles`]. The groups carry no methods of their own - every
//! `impl World` block reads and writes them directly, so a borrow of one
//! group never conflicts with another.
//! REF: FUN_8001E890, FUN_80021DF4, FUN_80026B4C, FUN_8003CA38, FUN_8003CE08, FUN_800520F0
//! REF: FUN_801D65D8, FUN_801D77F4, FUN_801D8DE8, FUN_801DE840, FUN_801DFDF8
//!
//! PORT: FUN_800467E8 (`world_map_camera_relative_bits` - held-pad camera-yaw
//!       remap; the engine reads the world-map camera azimuth directly from
//!       [`WorldMapController`] rather than the retail `gp+0x2D8` quadrant.)

use std::sync::Arc;

use crate::battle_events::{BattleEvent, BattleHitEvent, BattleHitFx, BattleSfxCue};
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

mod ambient_fx_state;
mod audio_residency;
mod audio_state;
mod battle_state;
mod camera_hooks;
mod camera_rig;
mod cast_fx_state;
mod config;
mod cutscene_state;
mod dialog_state;
mod disc_tables;
mod encounter_state;
mod field_carrier_state;
mod field_locomotion;
mod field_npc_state;
mod field_prop_state;
mod field_script_actor_state;
mod field_terrain;
mod field_vm_state;
mod frame_clock;
mod frame_step_floor;
mod item_bag;
mod menu_state;
mod minigame_state;
mod move_vm_globals;
mod party_state;
mod screen_fx_state;
mod seru_state;
mod shop_state;
mod state;
mod story_flag_state;
mod tile_board_state;
mod types;
mod world_map_state;
mod world_toggles;

pub use ambient_fx_state::AmbientFxState;
pub use audio_residency::{
    DANCE_SLOT2_PROT_INDEX, FISHING_SLOT2_PROT_INDEX, SHARED_REGION_SLOTS,
    SLOT_MACHINE_SLOT2_PROT_INDEX, SfxBankResidency, SharedRegionBank, minigame_slot2_bank,
};
pub use audio_state::{
    AudioState, FIELD_INIT_SIDE_BAND_REQUEST, SIDE_BAND_PARK, SfxRingOp, SideBandBank,
    VAB_01_RAW_BASE, runtime_sfx_descriptor_in, side_band_bank_for_request,
};
pub use battle_state::{BattleState, ClipRibbon};
pub use camera_hooks::CameraZoneRequest;
pub use camera_rig::CameraRig;
pub use cast_fx_state::CastFxState;
pub use config::*;
pub use cutscene_state::CutsceneState;
pub use dialog_state::DialogState;
pub use disc_tables::DiscTables;
pub use encounter_state::EncounterState;
pub use field_carrier_state::FieldCarrierState;
pub use field_locomotion::FieldLocomotion;
pub use field_npc_state::FieldNpcState;
pub use field_prop_state::FieldPropState;
pub use field_script_actor_state::{
    FieldAttachedLight, FieldScriptActorState, FieldScriptArc, ScriptActorRef,
};
pub use field_terrain::FieldTerrain;
pub use field_vm_state::FieldVmState;
pub use frame_clock::FrameClock;
pub use item_bag::{BagEntry, ItemBag};
pub use menu_state::MenuState;
pub use minigame_state::MinigameState;
pub use move_vm_globals::{MOVE_STRIP_REQUEST_CAP, MoveVmGlobals};
pub use party_state::PartyState;
pub use screen_fx_state::ScreenFxState;
pub use seru_state::SeruState;
pub use shop_state::ShopState;
pub use state::*;
pub use story_flag_state::StoryFlagState;
pub use tile_board_state::TileBoardState;
pub use types::*;
pub use world_map_state::WorldMapState;
pub use world_toggles::WorldToggles;

mod actors;
pub mod ambient;
mod cutscene_elements;
mod field_script_actors;
pub use field_script_actors::FieldLightDraw;
mod drop_shadow_render;
mod fog_render;
pub use cutscene_elements::{
    AMBIENT_EMITTER_SCENE_ARM, AMBIENT_EMITTER_TEMPLATE_VA, CutsceneElement, ElementFrame,
    ElementKind, ElementLink, WorldRng,
};
mod assets_events;
mod bag_rows;
pub use bag_rows::BagRow;
mod battle;
pub use battle::{
    ABSORB_BANNER_ELEMENT, BattleActorDrawPlan, BattleMessageBanner, BattleSpoilsBanner,
    LEVEL_UP_CUE, MAGIC_LEVEL_BANNER_ELEMENT, PARTY_BODY_RADIUS, PendingCast, RoutedEffectSpawn,
    SUMMON_SPAWN_BEHIND, SUMMON_STRIKE_BEHIND, SummonPhase, SummonStager, VICTORY_EXIT_PHASE,
    VICTORY_FADE_PHASE_SEED, VICTORY_LOAD_FRAMES, VICTORY_RESULTS_HOLD_FRAMES, VictoryPhase,
    VictorySequence, victory_pose_column, victory_pose_id, victory_pose_tier,
};
mod effects;
pub use effects::{
    ClutBlendFx, ClutCellFx, ClutCellFxPhase, DEBUG_EFFECT_LIFETIME_FRAMES, MAX_DEBUG_EFFECTS,
    ScriptVramMove,
};
mod encounters;
pub use encounters::FieldBossStager;
mod field_carriers;
pub mod field_elevation;
mod field_hud;
pub use field_elevation::{CELL_ELEVATION_OVERRIDE, ElevationOverride};
pub use field_hud::PassiveHudPoints;
mod field_loop;
mod field_movement;
mod field_warp;
pub use field_warp::FieldWarpTick;
mod frame_tick;
mod handler_actors;
pub use handler_actors::TransitionSweepReport;
mod items_arts;
mod narration;
mod prop_interact;
mod retail_progression;
pub use retail_progression::RetailProgressionTables;
mod save;
mod scene_program;
pub use scene_program::SceneProgramFrame;
mod vm_hosts;
mod vram_rect_fx;
pub use vram_rect_fx::OT_LEN_UNBOUNDED;
mod worldmap;

#[cfg(test)]
mod tests;
