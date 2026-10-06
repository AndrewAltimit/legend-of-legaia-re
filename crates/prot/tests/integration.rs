//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-prot --test integration <name>::`.

mod archive_tail_real;
mod archive_tiling_real;
mod cdname_retail_parse_disc;
mod prot_fuzz;
mod runtime_toc_span_real;
