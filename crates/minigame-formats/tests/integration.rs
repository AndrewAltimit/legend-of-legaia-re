//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-minigame-formats --test integration <name>::`.

mod baka_opponents_real;
mod baka_presentation_real;
mod dance_art_real;
mod dance_chart_real;
mod fishing_exchange_real;
mod fishing_species_real;
mod muscle_dome_real;
mod slot_cabinet_mesh_disc;
mod slot_payout_real;
