//! The play hosts' reads of a running `World` outside battle: a live
//! [`legaia_engine_core::world::World`] projected into the view models the
//! kernel crates define.
//!
//! Each module is named for the kernel module whose types it fills in: the
//! rules, layouts and view structs live in that crate (`engine-menus`,
//! `engine-minigames`, `engine-field`, `engine-scene`, `render-kernels`), and
//! the function here is the one read of live `World` state that produces
//! them. Both play hosts and `engine-screens` call these rather than reading
//! the world's fields.
//!
//! It sits directly above `legaia-engine-core` and nothing in the simulation
//! names it, so a change to a view rebuilds the hosts but not `World`. The
//! battle screen's reads are the sibling crate `legaia-engine-battle-hud`.

pub mod field_view_window;
pub mod muscle_dome;
pub mod pause_screens;
pub mod scene_staging;
pub mod shop;
pub mod tile_board;
pub mod timed_fight;
pub mod world_map_markers;
pub mod world_map_panel_host;
pub mod world_map_sky;
