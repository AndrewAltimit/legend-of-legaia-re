//! Save-menu sprite atlas - byte-perfect retail save/load-screen UI.
//!
//! Composes a single 256x256 RGBA atlas containing:
//!
//! - **9-slice panel tiles** decoded from the system-UI TIM at
//!   `PROT.DAT[0x018E0]` with CLUT row 2 (byte-pinned via
//!   `scripts/pcsx-redux/scan_panel_prims.py` against the PCSX-Redux
//!   sstate9 RAM dump; see [[project-load-screen-panel-source-pinned]]).
//!   The tiles sit at their natural source coordinates `(160..192, 0..32)`,
//!   so engines can sample them with the `OVERLAY_SYSTEM_UI_PANEL_*`
//!   constants exported by `legaia_ui_formats::title_pak`.
//! - **SLOT 1 / SLOT 2 pills** decoded from PROT 0899's save-menu TIM
//!   with CLUT 7. The atlas keeps these at their existing source
//!   coordinates `(33, 97/113, 45, 15)` for backward compatibility.
//!
//! Retail draws the panel from **14 GP0_TEXTURED_SPRITE primitives**:
//! 4 corners (4×4 each), top + bottom edges (24×4 tiles repeated 3×
//! with a 1×4 remainder), and left + right edges (4×21). **No
//! interior fill is drawn** - the "marbled blue" look in retail is
//! the dimmed title art bleeding through the empty middle of the
//! 9-slice frame. Engines that need an opaque interior must draw it
//! themselves.

use legaia_ui_formats::title_pak;

mod atlas;
mod build;
mod effect_marks;
mod gradients;
mod rects;
mod sprite_bake;

pub use atlas::*;
pub use build::*;
pub use effect_marks::*;
use gradients::*;
pub use rects::*;
use sprite_bake::*;

#[cfg(test)]
mod tests;
