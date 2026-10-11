//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-disc-audit --test integration <name>::`.

mod asset_fuzz;
mod battle_backdrop_real;
mod battle_jump_tables_real;
mod battle_texture_catalog_real;
mod byte_account_entries;
mod card_font_pack_real;
mod categorize_coverage;
mod field_map_real;
mod formation_census_real;
mod inherited_tail_real;
mod overlay_data_consumers_real;
mod packer_buffer_slack_real;
mod parser_robustness_fuzz;
mod scene_asset_table_walk_real;
mod scene_tmd_stream_real;
mod slot_b_module_layout_real;
mod tim_catalog_coverage;
mod tim_deep_catalog_coverage;
