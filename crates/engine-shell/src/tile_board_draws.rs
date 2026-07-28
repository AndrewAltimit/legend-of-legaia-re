//! Tile-board tile-actor draw assembly - re-exported from `engine-core`.
//!
//! The assembly itself moved down into
//! [`legaia_engine_core::tile_board`], because it was only ever `&World ->
//! Vec<..>` and living here made it unreachable from the browser play page:
//! `legaia-engine-shell` pulls winit and cpal and does not build for wasm32.
//! `World` maintains `tile_board_draw_list` for both hosts - the field VM's
//! op `0x49` installs a board regardless of who is rendering - so a scene
//! that installed one on the play page drew nothing while the walk SM still
//! refused its wall cells. That is a walk into an invisible board, not a
//! cosmetic gap.
//!
//! This module stays as the native window's import path so the bin's redraw
//! pass keeps its existing spelling, and so the two hosts demonstrably share
//! one copy rather than two that can drift.
//!
//! PORT: overlay_0897_801e0f3c (per-cell tile-actor draw pass; the select +
//! reposition halves live in `World::refresh_tile_board_draw_list`)

pub use legaia_engine_core::tile_board::{
    TileActorDraw, is_tile_actor_slot, tile_actor_slots_needing_mesh, tile_board_actor_draws,
};
