//! The boot front end's `World`-free kernels: the publisher-logos phase,
//! the title screen state machine, the title overlay's per-frame tick
//! (`FUN_801DD35C`, the sub-mode dispatcher), the SCUS primitive helpers it
//! calls and the title-art atlas.
//!
//! These modules name each other and `legaia-engine-system` - no menu
//! module, and no menu module names them. The pause / shop / save screens
//! are `legaia-engine-menus`, a sibling. See the crate README for the
//! module map.

#![forbid(unsafe_code)]

pub mod publisher_logos;
pub mod title;
pub mod title_overlay;
pub mod title_prim;
pub mod title_screen_atlas;
