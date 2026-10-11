//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-engine-field --test integration <name>::`.

mod hub_entry_sub_panel;
mod scene_bgm_labels_disc;
mod scene_fmv_triggers_disc;
mod timed_flag_scheduler_chain;
mod world_map_portal_classification_disc;
