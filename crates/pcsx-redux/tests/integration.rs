//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-pcsx-redux --test integration <name>::`.

mod anchor_load;
mod bring_your_own_game;
mod generic_state;
mod resident_patch_manifest;
mod scratchpad_read;
mod structural_matches_anchor;
