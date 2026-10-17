//! Battle-side VM kernels: the battle action state machine, the battle
//! formulas, the battle camera scripts and the cast-module ticks.
//!
//! Each module's whole closure is inside this crate (plus the asset
//! crates), so the crate sits strictly below `legaia-engine-vm` with no
//! cycle.
//!
//! The shared GTE camera (`psx_camera`, `camera_mover`, the projection
//! constants and `BattleCamPose`) is `legaia-engine-camera`, below this
//! crate; the battle camera script here builds its poses over it.
//!
//! Doc comments name `legaia-engine-vm` modules in plain code spans, since
//! rustdoc cannot resolve a link into a dependent crate. See the crate README for the module map.

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
pub mod battle_hp_bar;
pub mod battle_impact_fx;
pub mod battle_party_panel;
pub mod battle_pose_blend;
pub mod battle_record_writer;
pub mod battle_separation;
pub mod battle_slot;
pub mod battle_stream_slot;
pub mod battle_target_group;
pub mod battle_value_readout;
pub mod bgr555_grey;
pub mod mes_escape_append;
pub mod move_no_effect_guard;
pub mod nested_record_copy;
pub mod panel_backread_loader;
pub mod rgb_wash_scale;
pub mod seru_side_effect;
pub mod status_effects;
