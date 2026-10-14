//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-engine-ui --test integration <name>::`.

mod battle_fx_ladder;
mod camera_relative_retail_oracle;
mod field_hud_stand_in;
mod numeral_host_parity;
mod party_cursor_centring;
mod pause_menu_compose;
mod shop_quantity_compose;
