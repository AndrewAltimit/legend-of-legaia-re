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

use crate::move_buffer_host;
use legaia_actor_formats::monster_archive::MonsterAnimation;
use legaia_anm::{AnimPlayer, PoseFrame};
use legaia_engine_battle::battle_events::{BattleEvent, BattleHitEvent, BattleHitFx, BattleSfxCue};
use legaia_engine_battle::levelup::{LevelUpBanner, LevelUpResult, LevelUpTracker};
use legaia_engine_battle::tactical_arts::ArtLearnedBanner;
use legaia_engine_battle_vm::battle_action::{
    BattleActionCtx, BattleActor, BattleEndCause, StepOutcome,
};
use legaia_engine_field::field_events::FieldEvent;
use legaia_engine_field::man_field_scripts::{FIELD_OFFMAP_HIDE_XZ, WalkTouchEvent};
use legaia_engine_field::world_map::WorldMapController;
use legaia_engine_scene::model_bank::GlobalTmd;
use legaia_engine_system::input;
use legaia_engine_vm as vm;
use legaia_engine_vm::MAX_ACTORS;
use legaia_save;
use vm::Position as ActorVmPosition;
use vm::actor_tick::{ActorPhysics, ListenerState, TickEvent, TickScalars};
use vm::effect_vm::Pool;
use vm::field::{FieldCtx, StepResult as FieldStepResult};
use vm::move_buffer::{MoveBufferState, cursor_advance};
use vm::move_vm::ActorState as MoveActorState;

use vm_hosts::{
    ActorVmHostImpl, BattleHostImpl, EffectHostImpl, FieldCarrierHostImpl, FieldHostImpl,
    MoveVmHostImpl, WorldMapEntityHostImpl, field_step_routed,
};

mod ambient_fx_state;
mod audio_residency;
mod audio_state;
mod battle_state;
mod camera_hooks;
mod cast_fx_state;
mod config;
mod cutscene_state;
mod disc_tables;
mod field_locomotion;
mod field_npc_state;
mod field_prop_state;
mod field_terrain;
mod field_vm_state;
mod frame_step_floor;
mod menu_state;
mod player_rig;
mod screen_fx_state;
mod state;
mod types;
mod world_map_state;
mod world_toggles;

pub use ambient_fx_state::{AmbientFxState, FrameGrab, SeededVramRect};
pub use audio_state::{
    ARENA_SIDE_BAND_REQUEST, AudioState, BGM_SUB_OP_ENGINE_STOP, FIELD_INIT_SIDE_BAND_REQUEST,
    SIDE_BAND_PARK, SideBandBank, VAB_01_RAW_BASE, runtime_sfx_descriptor_in,
    side_band_bank_for_request,
};
pub use battle_state::{BattleState, ClipRibbon, INFLIGHT_GROUND_SLOTS, InflightCastSeed};
pub use cast_fx_state::{CastFxState, PendingBurst};
pub use config::*;
pub use cutscene_state::{CappedTimeline, CutsceneState};
pub use disc_tables::DiscTables;
pub use field_locomotion::FieldLocomotion;
pub use field_npc_state::{FieldFaceRow, FieldNpcState, SPAWN_HEADING};
pub use field_prop_state::FieldPropState;
pub use field_terrain::{FieldTerrain, step_floor_ladder};
pub use field_vm_state::FieldVmState;
use legaia_engine_battle::encounter_state::EncounterState;
use legaia_engine_battle::seru_state::SeruState;
use legaia_engine_field::camera::CameraZoneRequest;
use legaia_engine_field::camera_rig::CameraRig;
use legaia_engine_field::dialog_state::{DialogRequest, DialogState, ThreeActorTalk};
use legaia_engine_field::field_carrier_state::{CarrierMenu, FieldCarrierState, spar_menu_of};
use legaia_engine_field::field_script_actor_state::{
    FieldAttachedLight, FieldScriptActorState, FieldScriptArc, ScriptActorRef,
};
use legaia_engine_field::move_vm_globals::{FieldNpcMotion, MoveVmGlobals};
use legaia_engine_field::world_map_entity_config::{WorldMapEncounterState, WorldMapEntityConfig};
use legaia_engine_menus::party_state::PartyState;
use legaia_engine_menus::shop_state::ShopState;
use legaia_engine_minigame_scenes::minigame_state::MinigameState;
use legaia_engine_minigames::tile_board_state::TileBoardState;
use legaia_engine_system::sound_state::SfxRingOp;
use legaia_engine_system::story_flag_state::StoryFlagState;
use legaia_engine_vm::frame_clock::FrameClock;
pub use menu_state::MenuState;
pub use player_rig::PlayerRigSource;
pub use screen_fx_state::{ClearColourRamp, ScreenFrame, ScreenFxState};
pub use state::*;
pub use types::*;
pub use world_map_state::WorldMapState;
pub use world_toggles::{
    BONUS_DROP_CHANCE_PCT, DAMAGE_AP_RANGE, DAMAGE_AP_RETAIL, DEFAULT_BATTLE_SM_FRAME_CADENCE,
    EXP_RATE_RANGE, EXP_RATE_RETAIL, FLEE_EXP_PCT, FORTRESS_ENCOUNTER_SCENES, FORTRESS_STALE_FLAG,
    SCALE_PINNED_MONSTER_ID, SCALE_RANGE, SCALE_RETAIL, SPIRIT_AP_RANGE, SPIRIT_AP_RETAIL,
    WorldToggles, scale_attack_cost, scale_exp_permille, scale_stat_permille,
};

mod actors;
pub mod ambient;
mod camera_world;
mod cutscene_elements;
mod field_script_actors;
mod fishing_hub_exits;
pub mod mode_world;
pub use field_script_actors::{FieldLightDraw, NpcDrawPose};
mod drop_shadow_render;
mod fog_render;
mod fog_volume_host;
pub use cutscene_elements::{
    AMBIENT_EMITTER_SCENE_ARM, AMBIENT_EMITTER_TEMPLATE_VA, CutsceneElement, ElementFrame,
    ElementKind, WorldRng,
};
mod assets_events;
mod battle;
pub use battle::{
    ABSORB_BANNER_ELEMENT, BATTLE_BODY_NCLIP, BattleActorDrawPlan, BattleDefeatBanner,
    BattleFrameClock, BattleMessageBanner, BattleSpoilsBanner, COUNTER_MESSAGE_HOLD,
    COUNTER_MESSAGE_VA, DEFAULT_BATTLE_FRAME_STEP, MAGIC_LEVEL_BANNER_ELEMENT, PARTY_BODY_RADIUS,
    PendingCast, RoutedEffectSpawn, SUMMON_SPAWN_BEHIND, SUMMON_STRIKE_BEHIND, SummonPhase,
    SummonStager, TIMED_MESSAGE_ELEMENT,
};
pub use battle::{AutoComboInputs, AutoComboState};
pub use battle::{CASTER_STAGE_TICK_LIMIT, CasterStagePhase, CasterStageRun};
mod effects;
pub use effects::{
    ClutBlendFx, ClutCellFx, ClutCellFxPhase, DEBUG_EFFECT_LIFETIME_FRAMES,
    EFFECT_MODEL_LIBRARY_BASE, EffectChildSeed, MAX_DEBUG_EFFECTS, ScriptVramMove,
};
mod encounters;
pub use encounters::FieldBossStager;
mod field_carriers;
mod field_frame_tail;
pub use field_frame_tail::{EFFECT_SCENE_GRAPH_STEP, NpcClipRetarget, WorldFrameTail};
pub(crate) mod field_xa;
pub use field_xa::{scene_minigame_door_xa_prestage, scene_xa_prestage};
mod field_loop;
mod field_movement;
pub mod field_npc_cull;
mod field_npc_height;
mod field_warp;
mod static_window;
pub use static_window::StaticObjectWindow;
mod motion_pause_host;
pub use field_warp::FieldWarpTick;
mod frame_tick;
mod handler_actors;
pub use handler_actors::TransitionSweepReport;
mod items_arts;
pub use items_arts::BATTLE_GOLD_CAP;
mod narration;
pub use narration::FrameGrade;
mod npc_morph;
pub use npc_morph::MorphOwner;
mod object_actor_height;
use legaia_engine_field::object_tint::{
    ActorTint, ActorTintKey, ObjectGlideLeg, ObjectRampSlot, ObjectSlotRamp,
};
pub mod pause_session;
mod prop_interact;
mod retail_progression;
pub use retail_progression::RetailProgressionTables;
pub use save::{FIELD_MENU_DENY_CUE, FIELD_MENU_LOCK_BIT, FIELD_SYSTEM_LOCK_BIT};
mod save;
mod scene_program;
pub use scene_program::SceneProgramFrame;
mod vm_hosts;
mod vram_rect_fx;
pub use vram_rect_fx::FRAMEBUFFER_LAND_RECT;
pub use vram_rect_fx::OT_LEN_UNBOUNDED;
mod worldmap;

#[cfg(test)]
mod tests;

// The battle-screen steps exposed to test crates above this one (`test-drivers`).
#[cfg(feature = "test-drivers")]
pub mod test_drivers;
