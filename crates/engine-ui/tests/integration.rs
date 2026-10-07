//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-engine-ui --test integration <name>::`.

mod camera_relative_retail_oracle;
mod pause_menu_compose;
mod w2b_numeral_host_parity;
mod w2c_battle_fx_ladder;
mod w4a_shop_quantity_compose;
