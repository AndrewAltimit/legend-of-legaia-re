//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-engine-vm --test integration <name>::`.

mod ambient_motion_disc_oracle;
mod ambient_motion_ops;
mod arts_auto_combo_tail_real;
mod battle_action_validator_arms;
mod battle_ai_and_guard_wiring;
mod battle_attack_camera_real;
mod battle_burst_real_records;
mod battle_cast_facing;
mod battle_cue_group_real;
mod battle_cue_group_wiring;
mod battle_grid_cue_scus_real;
mod battle_intro_chain;
mod battle_physical_predamage;
mod battle_strike_facing;
mod camera_mover_recomp_oracle;
mod cast_chain_bodies_real;
mod cast_module_trampoline_bodies;
mod effect_vm_real_efect;
mod hub_entry_sub_panel;
mod move_vm_overlay_ext_real_data;
mod w2_timed_flag_scheduler_chain;
