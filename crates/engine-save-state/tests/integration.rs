//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-engine-save-state --test integration <name>::`.

mod battle_capture_bgm;
mod battle_loot_use_item;
mod end_to_end_gameplay_loop;
mod item_bag_steal;
mod party_composition;
mod save_block_checksum;
mod save_state;
mod shiny;
mod support;
