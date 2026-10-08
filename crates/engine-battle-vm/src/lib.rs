//! Battle-side VM kernels: the battle action state machine, the battle
//! formulas, the battle camera scripts and the cast-module ticks.
//!
//! These modules were split out of `legaia-engine-vm` along a dependency
//! line: their whole closure is inside this crate (plus the asset crates),
//! so the crate sits strictly below `legaia-engine-vm` with no cycle.
//! `legaia-engine-vm` re-exports every module here at its old path
//! (`legaia_engine_vm::battle_action`, ...), so a consumer may name either.
//!
//! `psx_camera` and `camera_mover` live here because the battle camera
//! script and the GTE camera build call each other; the field-side camera
//! modules that use them (`camera_rel_actor`, `retail_cam`, ...) stay in
//! `legaia-engine-vm` and reach them through the re-export.
//!
//! Doc links that point back up at `legaia-engine-vm` modules are plain
//! code spans or unresolved paths, since rustdoc cannot resolve a link into
//! a dependent crate. See the crate README for the module map.

#![forbid(unsafe_code)]

pub mod battle_action;
pub mod battle_actor_draw;
pub mod battle_actor_tick;
pub mod battle_actor_tint;
pub mod battle_anim_rate;
pub mod battle_approach;
pub mod battle_arts_auto_combo;
pub mod battle_attack_camera;
pub mod battle_cam_script;
pub mod battle_camera;
pub mod battle_cast_census;
pub mod battle_cast_cue;
pub mod battle_cast_dispatch;
pub mod battle_commit_log;
pub mod battle_cue_group;
pub mod battle_cursor_pose;
pub mod battle_damage_wrappers;
pub mod battle_formulas;
pub mod battle_gauge;
pub mod battle_gauge_rearm;
pub mod battle_ground_grid;
pub mod battle_helpers;
pub mod battle_hp_bar;
pub mod battle_impact_fx;
pub mod battle_intro_particles;
pub mod battle_intro_styles;
pub mod battle_intro_swirl;
pub mod battle_intro_tiles;
pub mod battle_intro_transition;
pub mod battle_pose_blend;
pub mod battle_record_writer;
pub mod battle_separation;
pub mod battle_stream_slot;
pub mod battle_target_group;
pub mod battle_trail;
pub mod battle_value_readout;
pub mod camera_mover;
pub mod cast_arm_ticks;
pub mod cast_fatal_decision;
pub mod cast_module_camera;
pub mod cast_module_ticks;
pub mod cast_seru_ticks_a;
pub mod cast_seru_ticks_b;
pub mod move_no_effect_guard;
pub mod psx_camera;
