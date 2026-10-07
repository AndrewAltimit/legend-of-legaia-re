//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-save --test integration <name>::`.

mod card_item_slot_roundtrip;
mod real_card_roundtrip;
mod region_gate_card_brackets;
mod resume_and_engine_ext;
mod retail_live_state_lift;
mod schema_fixture;
mod w2c_card_inventory_ladder;
