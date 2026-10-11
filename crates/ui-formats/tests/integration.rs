//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-ui-formats --test integration <name>::`.

mod battle_ui_strings_real;
mod interior_page_real;
mod save_icon_real;
mod screen_elements_real;
mod system_ui_bundle_real;
mod tim_palette_context_real;
mod ui_widgets_real;
