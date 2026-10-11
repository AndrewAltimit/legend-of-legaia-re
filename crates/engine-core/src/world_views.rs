//! `World` projected into the view models the kernel crates define.
//!
//! Each child is named for the kernel module whose types it fills in: the
//! rules, layouts and view structs live in that crate (`engine-menus`,
//! `engine-minigames`, `engine-field`, `render-kernels`), and the function
//! here is the one read of live [`crate::world::World`] state that produces
//! them. Both play hosts call these rather than reading the world's fields.

pub mod muscle_dome;
pub mod pause_screens;
pub mod shop;
pub mod tile_board;
pub mod timed_fight;
pub mod world_map_panel_host;
pub mod world_map_sky;
