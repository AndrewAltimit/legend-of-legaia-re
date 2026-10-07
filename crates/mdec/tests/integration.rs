//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-mdec --test integration <name>::`.

mod mdec_robustness_fuzz;
mod st_ring_real_str;
mod str_player_segment;
mod w1a_fmv_ladder;
