//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-mednafen --test integration <name>::`.

mod dispatch_table;
mod enemy_stager_binding;
mod evolved_summon_binding;
mod firetail_movefx_liveness;
mod gte_projection_real;
mod rage_delegated_pick;
mod real_saves;
mod real_spu_smoke;
mod static_overlay_clean_copy;
mod summon_binding_base_high;
mod summon_model_base;
mod summon_render_mode_node;
mod training_formation;
