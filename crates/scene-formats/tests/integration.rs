//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-scene-formats --test integration <name>::`.

mod clut_walk_real;
mod field_anim_tables_real;
mod field_op_census_real;
mod field_pack_real;
mod inn_costs_disc;
mod man_section_corpus;
mod ocean_real;
mod prescript_move_stager_records_real;
mod scene_destinations_p2_disc;
mod scene_event_records_word_aligned_real;
mod scene_v12_corpus;
mod scratch_global_bit_writers_real;
mod shop_stock_cli_real;
mod thread_camera_roll_census;
mod walk_decorations_real;
mod world_map_overlay_real;
