//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-vab --test integration <name>::`.

mod corpus_chunk_carriage;
mod corpus_vag_spacer;
mod vab_fuzz;
