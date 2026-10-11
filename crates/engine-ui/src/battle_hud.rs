//! UI draw-builder functions: sprite/text batching, shop rows, level-up +
//! capture banners, and the battle HUD. Each returns renderer-agnostic
//! [`TextDraw`]/[`SpriteDraw`] batches.
//!
//! Dialog text is NOT built here: both hosts render dialog through the
//! `DialogSnapshot` path (native `window/hud.rs`, web `play_dialog`) at the
//! byte-pinned retail geometry. A plain typewriter fallback
//! (`dialog_box_draws_for` + its `DialogBoxLayout`/`DialogGlyphView` types)
//! used to live here; nothing called it on either host, so it was removed
//! rather than waived in the ui-host-drift gate.

use crate::*;

mod banners;
mod battle_hud_draws;
mod battle_hud_layout;
mod battle_hud_views;
mod battle_readouts;
mod shop;
mod stage_draws;

pub use banners::*;
pub use battle_hud_draws::*;
pub use battle_hud_layout::*;
pub use battle_hud_views::*;
pub use battle_readouts::*;
pub use shop::*;
pub use stage_draws::*;
