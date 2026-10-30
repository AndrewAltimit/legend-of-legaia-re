//! The crate's public-API tests, mounted into the library's unit-test target.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml), and `src/lib.rs` mounts this file as
//! `#[cfg(test)] mod integration`, so the library compiles once in test mode
//! and never a second time as a plain library to link a test binary against.
//! Run a single file's tests with
//! `cargo test -p legaia-engine-session --lib integration::<name>::`.

mod arts_shout_battle;
mod arts_voice_spar_disc;
mod attack_arm_arts_entry;
mod battle_effect_teardown_disc;
mod battle_item_use_presentation;
mod battle_monster_bank_cue;
mod battle_party_forms_headless;
mod battle_xa_clip_bank;
mod bgm_scene_resolution;
mod cast_voice_lazy_stage;
mod cd_reverb_minigame_disc;
mod dance_sfx_disc;
mod dead_target_redirect_disc;
mod dialog_font_metrics;
mod expansion_boss_script_disc;
mod expansion_map_resume_disc;
mod expansion_new_game_disc;
mod expansion_refill_wave_disc;
mod field_actor_guard_skip;
mod field_ambient_ring_cue;
mod fmv_handoff;
mod fog_gate_census;
mod frame_step_kernels;
mod free_roam_camera_reset;
mod minigame_audio_disc;
mod new_game_seed;
mod npc_conversation_terminates;
mod npc_run_clip_bind_disc;
mod pack_audio_disc;
mod picker_party_disc;
mod player_one_shot_clip_disc;
mod player_tilt_live_disc;
mod rayman_collapse_audio_disc;
mod restore_field_bgm_disc;
mod resume_landing;
mod save_gate_field_menu;
mod save_roundtrip_library;
mod scene_encounter_rollable;
mod sfx_cue_resident_bank;
mod sfx_shared_region;
mod taiku_security_door_camera;
mod tetsu_spar_picker_disc;
mod tetsu_spar_somersault;
mod tile_board_draw_live;
mod training_battle;
mod world_map_battle_return_camera;
mod world_map_camera_offset_oracle;
mod world_map_door_encounter_disc;
mod world_map_fog_oracle;
mod world_map_live;
mod xa_cue_level_disc;
