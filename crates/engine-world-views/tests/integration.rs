//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-engine-world-views --test integration <name>::`.

mod field_lit_rows_cave01_disc;
mod scene_clut_over_shared_layer;
mod scene_mesh_pool_walk_disc;
mod world_map_kingdom_clut_overwrite;
mod world_map_render_ladder;
