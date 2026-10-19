//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-engine-battle-hud --test integration <name>::`.

mod battle_formation_span;
mod battle_intro_names;
mod battle_plaque_badge_census_disc;
mod commit_confirm;
mod counterattack;
mod round_prompt_gate;
mod spirit_gauge;
