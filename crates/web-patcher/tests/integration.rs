//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-web-patcher --test integration <name>::`.

mod texture_pack_roundtrip;
mod texture_registry_parity;
mod translate_workbench_disc;
