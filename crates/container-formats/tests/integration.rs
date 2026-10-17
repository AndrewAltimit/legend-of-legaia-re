//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-container-formats --test integration <name>::`.

mod bse_bank_real;
mod bse_bank_tail_real;
mod data_field_trailer_real;
mod sound_pack_stream_real;
mod sound_pack_vab_seq_real;
