//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-parity --test integration <name>::`.
//!
//! These are the engine-versus-capture oracles: each compares what an engine
//! or format crate derives against a retail save state, so it needs both
//! sides and lives above both.

mod arts_randomizer_runtime_e2e;
mod attached_light_cave01_retail_capture_disc;
mod attached_light_retail_capture_disc;
mod battle_actor_tint_retail_capture;
mod battle_anim_real;
mod battle_clip_ladder_disc;
mod battle_loss_window_disc;
mod battle_monster_tags_disc;
mod battle_party_palette_retail_capture;
mod chapter1_frontier_ladder;
mod chest_randomizer_runtime_e2e;
mod door_randomizer_runtime_e2e;
mod drop_shadow_retail_capture_disc;
mod effect_default_arm_retail_capture;
mod encounter_randomizer_runtime_e2e;
mod enemy_agl_budget_disc;
mod enemy_stager_binding;
mod evolved_summon_binding;
mod field_npc_entry_positions_disc;
mod field_npc_state_parity_disc;
mod firetail_movefx_liveness;
mod fog_sheet_colour_retail_capture_disc;
mod house_door_randomizer_runtime_e2e;
mod map_door_randomizer_runtime_e2e;
mod monster_drop_randomizer_runtime_e2e;
mod opening_progression_oracle;
mod overworld_draw_order_retail_capture_disc;
mod prologue_sepia_retail_capture;
mod real_saves;
mod s4_warp_endstate_oracle;
mod scene_prescript_consumer_census_disc;
mod seru_trade_randomizer_runtime_e2e;
mod shop_randomizer_runtime_e2e;
mod sparring_exit_retail_capture;
mod starting_items_randomizer_runtime_e2e;
mod steal_randomizer_runtime_e2e;
mod summon_binding_base_high;
mod summon_camera_locked_retail_capture;
mod summon_render_mode_node;
mod super_art_queue_replace;
mod unused_enemy_randomizer_runtime_e2e;
mod unused_item_randomizer_runtime_e2e;
