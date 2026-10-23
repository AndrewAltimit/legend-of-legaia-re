//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-web-play --test integration <name>::`.

mod attached_light_page_prims;
mod battle_hud_badges;
mod battle_overlay_parity;
mod battle_render_ladder;
mod battle_render_scene;
mod battle_tutorial_page;
mod battle_vram_channel;
mod boot_logos_ladder;
mod commit_log_all_target_page;
mod dev_menu_equip_ladder;
mod dream_yaw_part_page_ladder;
mod fishing_banner_ladder;
mod fishing_line_hosts;
mod fog_page_prims;
mod menu_depth_ladder;
mod menu_parity;
mod morph_page_ladder;
mod new_game_flow_parity;
mod occlusion_fade_page;
mod page_compose_ladder;
mod passive_badge_page;
mod play_actor_tints_disc;
mod play_audio_parity;
mod play_baka_ladder_host;
mod play_boot_tables_parity;
mod play_camera_parity_disc;
mod play_cave01_naming_disc;
mod play_compass_disc;
mod play_compose_ladder;
mod play_fishing_host;
mod play_fmv_real;
mod play_minigames_host;
mod play_npc_tilt_parity;
mod play_object_moves_disc;
mod play_page_host_parity_ladder;
mod play_page_presentation_parity;
mod play_parity;
mod play_render_state;
mod play_ringside_still_disc;
mod play_screen_parity_disc;
mod play_sfx_channel;
mod point_card_toast_page;
mod prize_confirm_page;
mod raseru_cross_out_page;
mod screen_effect_page_prims;
mod shop_overlay_parity;
mod soft_reset_records_page;
mod status_marks_page;
mod title_backdrop_parity;
mod view_window_crop_page;
mod world_map_sublist_sfx_page;
