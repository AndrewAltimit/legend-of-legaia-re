//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-engine-menus --test integration <name>::`.

mod muscle_intermission_cadence;
mod title_attract_handoff;
mod w1g_save_subscreen_ladder;
mod w2e_equip_applier;
mod w2e_title_menu_law;
mod w4a_menu_wiring;
