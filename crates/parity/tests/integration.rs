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

mod enemy_stager_binding;
mod evolved_summon_binding;
mod firetail_movefx_liveness;
mod real_saves;
mod summon_binding_base_high;
mod summon_render_mode_node;
mod super_art_queue_replace;
