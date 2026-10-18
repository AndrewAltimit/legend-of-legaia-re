//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-engine-battle --test integration <name>::`.

mod battle_afterimage_gate_real;
mod battle_attach_swap_real;
mod battle_seats_disc;
mod encounter_man_real;
mod move_power_table_disc;
