//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-engine-vm --test integration <name>::`.

mod battle_burst_real_records;
mod effect_vm_real_efect;
mod move_fx_records_vm_exec_disc;
mod move_vm_overlay_ext_real_data;
