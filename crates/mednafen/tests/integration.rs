//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-mednafen --test integration <name>::`.

mod dispatch_table;
mod gte_projection_real;
mod rage_delegated_pick;
mod real_spu_smoke;
mod static_overlay_clean_copy;
mod summon_model_base;
mod training_formation;
