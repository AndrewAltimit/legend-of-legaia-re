//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-web-minigames --test integration <name>::`.

mod baka_presentation_wasm_api;
mod dance_howto_disc;
mod dome_ladder_and_hub_real;
mod minigame_dance_sting_wire;
mod minigames_dance_api;
mod minigames_fishing_pond;
mod minigames_wasm_api;
mod muscle_first_visit_live;
mod muscle_page_cadence;
mod muscle_web_real;
mod slot_bonus_marquee_ladder;
