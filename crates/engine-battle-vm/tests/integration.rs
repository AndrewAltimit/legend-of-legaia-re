//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-engine-battle-vm --test integration <name>::`.

mod arts_auto_combo_tail_real;
mod battle_action_validator_arms;
mod battle_ai_and_guard_wiring;
mod battle_attack_camera_real;
mod battle_cast_facing;
mod battle_cue_group_real;
mod battle_cue_group_wiring;
mod battle_grid_cue_scus_real;
mod battle_physical_predamage;
mod battle_strike_facing;
mod translated_battle_strings_disc;
