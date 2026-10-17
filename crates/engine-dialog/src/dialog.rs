//! Dialog panel: port of the field VM's dialog opener path.
//!
//! PORT: FUN_801D84D0
//! REF: FUN_8001FD44, FUN_8001D7F8
//!
//! Those two are named below as what this module is NOT: the name-based
//! scene-change packet and its scene-name sync callee.
//!
//! Wraps a [`legaia_mes::DialogPlayer`] in the runtime state the retail
//! dialog renderer holds: the typed-out glyph buffer for the current page,
//! a pen color tracked through `0xCF` color-change escapes, and an explicit
//! page-break / done flag the host loop polls.
//!
//! This is the minimal substrate engines need to drive an on-screen dialog
//! window without re-implementing the typewriter / page-break semantics in
//! every consumer (the MES viewer in `asset-viewer`, the field VM's dialog
//! opener, the battle dialog overlay, etc). `FUN_8001FD44` is **not** that
//! opener - it is the name-based scene-change packet (`strcpy` into
//! `0x8007050C` / `0x80084548`, `_DAT_1F800394 |= 0x40`, then
//! `FUN_8001D7F8`), and nothing in this module implements it; the tag moved
//! to the op-`0x3F` arm in `legaia_engine_core::world`'s field-VM host.
//!
//! Provenance: the per-page glyph accumulation + page-break gate mirror the
//! retail dialog window pager `FUN_801D84D0` in the dialog overlay (see
//! [`docs/formats/dialog-font.md`](../../../docs/formats/dialog-font.md)).
//! Layout / GPU bridging lives in `legaia-engine-render` -
//! [`text_draws_for`](../../legaia_engine_render/fn.text_draws_for.html)
//! consumes the [`Self::page_glyphs`] byte stream via [`legaia_font::Font`].
//! REF: FUN_80036888
//! REF: FUN_80039B7C, FUN_801DE840

use legaia_mes::{DialogPlayer, Format as MesFormat, Interpreter, MesEvent, PlayerState};
use std::sync::Arc;

mod borrowed_panel;
mod inline_segments;
mod owned_panel;
mod page_decode;
mod picker;
mod scene_mes;
mod substitutions;
mod tick;

pub use borrowed_panel::*;
pub use inline_segments::*;
pub use owned_panel::*;
pub use picker::{
    PICKER_CONFIRM_CUE, PICKER_MOVE_CUE, PICKER_PAD_CONFIRM, PICKER_PAD_DOWN, PICKER_PAD_MASK,
    PICKER_PAD_UP, PickerPadStep,
};
pub use scene_mes::*;
pub use substitutions::*;

#[cfg(test)]
mod tests;
