//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-engine-scene --test integration <name>::`.

mod cd_dma_real_prot;
mod coplanar_residual_disc;
mod env_clut_coverage_disc;
mod field_actor_placements_disc;
mod field_dialog_boxpack_disc;
mod field_dialog_pickers_disc;
mod field_floor_layer_disc;
mod field_ground_surface_disc;
mod field_ground_texture_pages_disc;
mod field_npc_placements_disc;
mod field_object_binds_disc;
mod field_prop_pose_sampler_disc;
mod field_text_free_spawn_touch_disc;
mod man_midvisit_rearrangement_disc;
mod man_variant_carrier_census_disc;
mod minigame_entry_census_disc;
mod model_bank_disc;
mod motion_flag_census_disc;
mod op49_sub_op_census;
mod op49_window_census_disc;
mod opening_scene_anm_bundle;
mod rim_elm_sparring_carrier;
mod scene_bundle_smoke;
mod stream_file_real;
mod system_flag_census_disc;
mod talk_face_acquire_bind_disc;
mod town01_entry_script;
mod town01_p1_arm_sites;
mod v12_bundle_man_disc;
mod walk_field_map;
mod world_map_pack_untextured_real;
