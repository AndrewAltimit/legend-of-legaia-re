//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-gamedata --test integration <name>::`.

mod accessory_passives_vs_disc;
mod arts_scus_oracle;
mod casino_prizes_vs_disc;
mod data_files;
mod enemy_stats_vs_disc;
mod equip_slots_vs_disc;
mod item_prices_vs_disc;
mod magic_vs_disc;
mod shop_inventory_vs_disc;
