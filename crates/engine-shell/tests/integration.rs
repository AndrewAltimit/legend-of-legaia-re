//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-engine-shell --test integration <name>::`.

mod arts_swing_ladder;
mod av_decode_oracle;
mod battle_depth_replay;
mod battle_spell_plate_disc;
mod boss_approach_disc;
mod casino_floor_softlock;
mod chapter1_boss_spine_oracle;
mod chapter1_hub_breadth_oracle;
mod chapter1_hub_depth_oracle;
mod chapter1_hub_sweep_oracle;
mod chapter1_spine_oracle;
mod controller_input;
mod critical_path_replay;
mod determinism_j2;
mod dome_cast_band_ladder;
mod field_walk_host_parity;
mod full_game_ladder;
mod gte_sin_lut_real;
mod host_frame_parity;
mod menu_replay;
mod menu_window_dispatch_real;
mod minigame_replay;
mod mode_seat_host_parity;
mod native_card_import;
mod native_front_end;
mod native_minigame_ladder;
mod ocean_anim_real;
mod pause_learned_content_ladder;
mod retail_compare_corpus;
mod ropeway_gondola_walk_off_disc;
mod scenarios;
mod soak_harness;
mod tower_lift_arrival_disc;
mod v0_1_playthrough;
mod world_map_camera_remap;
// --- lane K ---
mod casino_seat_round_trip;
// --- lane H ---
mod baka_seat_duel_hud;
mod battle_ui_sfx_disc;
