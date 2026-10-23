//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-engine-menus --test integration <name>::`.

mod equip_applier;
mod menu_suite_e2e;
mod pause_menu_kernel_wiring;
mod save_subscreen_ladder;
mod translated_menu_strings_disc;
