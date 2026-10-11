//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-engine-fishing --test integration <name>::`.

mod w1f1_fishing_pond_ladder;
mod w1g_fishing_tackle_pick_ladder;
mod w2c_fishing_lure_point;
