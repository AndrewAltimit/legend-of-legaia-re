//! Field-scene kernels: the scripted-scene actor program and plain-template
//! cutscene elements, the field camera (over the `CameraWorld` slice of the
//! world), the follow camera's per-scene parameters, vertical
//! ease and register ramp, player clip playback, the morph-weight apply
//! pass, the opening narration roller, the per-actor handler and its
//! kernels, actor clone, look rotation and screen tween, the scene
//! transition and in-field save-screen actors, the op-`0x49` submode,
//! the field event queue, the scene MAN field-script decoders, the mode
//! table and seat (over the `ModeWorld` slice of the world), CLUT effects, the overworld draw order and
//! ground cue, the overworld controller and its panel screen, walk regen, light-source row shading, the slot-6 audio
//! release, packet colours and the animation / SFX cue routers: the
//! `World`-free half of the field runtime.
//!
//! Every module's whole dependency closure inside the engine is in this
//! crate or below it, so it sits strictly below `legaia-engine-core`,
//! which re-exports each module at its old path. Doc links that pointed
//! back up at `engine-core` are plain code spans, since rustdoc cannot
//! resolve a link into a dependent crate. See the crate README for the
//! module map.

#![forbid(unsafe_code)]

// Modules these files name as `crate::...`, which engine-core re-exports at
// its root; binding them here keeps the moved files' paths unchanged.
use legaia_engine_battle::encounter_record;
use legaia_engine_system::fade;
use legaia_engine_system::input;
use legaia_engine_system::music_labels;
use legaia_engine_system::sound_state;

use legaia_engine_system::mode_entry_init;

pub mod actor_handler;
pub mod actor_look;
pub mod camera;
pub mod camera_ease;
pub mod camera_zone;
pub mod clut_cell_fx;
pub mod clut_fade_actor;
pub mod clut_fx;
pub mod code_lock_actor;
pub mod cutscene_narration;
pub mod cutscene_script_elements;
pub mod cutscene_timeline;
pub mod dev_equip_commit;
pub mod dev_menu_adjust;
pub mod dev_menu_leaves;
pub mod escape_timer;
pub mod field_actor_clone;
pub mod field_actor_kernels;
pub mod field_actor_program;
pub mod field_actor_reflect;
pub mod field_actor_timers;
pub mod field_anim;
pub mod field_audio_release;
pub mod field_events;
pub mod field_lit_mesh;
pub mod field_passive_hud;
pub mod field_player_clip;
pub mod field_regions;
pub mod field_save_screen_actor;
pub mod field_state_pick;
pub mod field_submode;
pub mod field_subsystem_enter;
pub mod field_warp_tile;
pub mod float_tween;
pub mod inline_dialogue;
pub mod man_field_scripts;
pub mod minigame_entry;
pub mod mode;
pub mod morph_weight_apply;
pub mod motion_pause;
pub mod overworld_draw_order;
pub mod overworld_ground_cue;
pub mod packet_color;
pub mod pooled_actor_seed;
pub mod register_ramp;
pub mod scene_transition_actor;
pub mod submode_hub_actors;
pub mod travel_art_actor;
pub mod vdf_pulse;
pub mod walk_regen;
pub mod world_map;
pub mod world_map_dim;
pub mod world_map_entity;
pub mod world_map_horizon;
pub mod world_map_panel;
pub mod world_map_panel_actors;
pub mod world_map_panel_host;
