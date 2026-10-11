//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-asset --test integration <name>::`.
//!
//! Only tests that spawn the `asset` binary live here; a format's own tests
//! sit in the crate that owns the format.

mod slot_bonus_marquee;
