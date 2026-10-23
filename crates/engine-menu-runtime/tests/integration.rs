//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-engine-menu-runtime --test integration <name>::`.

mod bag_row_payload_is_a_slot;
mod bag_use_list_rows;
mod field_menu_subsession_e2e;
mod incense_encounter_window;
mod inn_cost_scene_disc;
mod item_use_applier_legs;
mod list_order_reorder;
mod magic_list_relevance;
mod menu_data_wiring;
mod menu_data_wiring_disc;
mod menu_widget_scripts_real;
mod pause_screens_disc;
mod pause_special_use_ladder;
mod playable_shell_e2e;
mod prize_exchange_wiring;
mod shop_quantity_stepper;
mod status_snapshot_ailment_word;
