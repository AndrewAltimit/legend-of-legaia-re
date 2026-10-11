//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-game-tables --test integration <name>::`.

mod absorb_caption_real;
mod accessory_passive_real;
mod battle_attack_camera_table_real;
mod battle_camera_table_real;
mod element_affinity_real;
mod equip_stats_real;
mod fmv_dispatch_real;
mod item_effect_real;
mod item_names_real;
mod level_up_tables_real;
mod menu_windows_real;
mod mode_table_real;
mod move_fx_records_real;
mod move_power_cue_groups_real;
mod move_power_real;
mod new_game_real;
mod new_game_seed_disc;
mod seru_side_effect_real;
mod sfx_table_real;
mod steal_table_real;
mod victory_pose_and_xa_cue_real;
mod widget_script_real;
mod worldmap_menu_real;
