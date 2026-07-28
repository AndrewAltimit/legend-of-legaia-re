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
//! **A re-export is not a port site.** The tag below is a `REF:`, not a
//! `PORT:`: the ported pass is
//! [`legaia_engine_core::tile_board::tile_board_actor_draws`], which is where
//! the `PORT:` tag lives and which both hosts reach. A second `PORT:` here
//! declared a port anchor on a module that is only four `pub use` names, so
//! nothing could ever call *it* and the live-reachability audit read it as an
//! inert port - a measurement artifact of the move down into `engine-core`,
//! not a host that stopped drawing the board.
//!
//! REF: overlay_0897_801e0f3c (per-cell tile-actor draw pass - see the
//! provenance caveat on the `engine-core` function: the renderer is the walk
//! SM's tail block at `0x801EFEA0`, not an entry at this address)

pub use legaia_engine_core::tile_board::{
    TileActorDraw, is_tile_actor_slot, tile_actor_slots_needing_mesh, tile_board_actor_draws,
};
