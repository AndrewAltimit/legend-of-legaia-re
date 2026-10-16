//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-overlay-images --test integration <name>::`.

mod boot_overlay_disc;
mod cast_module_data_rows_real;
mod cast_presentation_real;
mod enemy_stager_real;
mod slot_b_record_bounds_real;
mod static_overlay_extract;
mod summon_overlay_block;
mod summon_overlay_real;
