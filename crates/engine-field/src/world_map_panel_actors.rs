//! The field-overlay **panel actor state machines** - the `ctx[+0x54]`-keyed
//! phase machines that sit on top of the shared window leaves in
//! [`crate::world_map_panel`].
//!
//! Six of them live in the world-map band of PROT 0897 (base `0x801CE818`):
//! the brightness fade/flash (`FUN_801ED308`), the sub-list open/close
//! (`FUN_801ED590`), the return-to-title soft reset (`FUN_801EDF00`), the
//! screen-fill fade transition (`FUN_801EE5D4`), the text-box dispatcher
//! (`FUN_801EE90C`) and the flag-window picker (`FUN_801EF014`). A seventh,
//! the field party HUD (`FUN_801D0D38`), is not a phase machine at all - it
//! is a per-frame panel builder with an idle timer.
//!
//! Every body here is read out of the statically extracted PROT 0897 image at
//! its own base, not out of a dump: `scripts/ghidra-analysis/locate-entry-image.py`
//! reports a stack-frame prologue for all seven VAs in `897/field` and in no
//! other based image. The `overlay_0897_*`-prefixed dumps at these addresses
//! print bodies that resolve against no image at the queried VA
//! (`docs/tooling/dump-corpus-integrity.md`); the capture-tagged dumps
//! (`801ed308.txt`, `801ed590.txt`, `801edf00.txt`, `801ee5d4.txt`,
//! `801ee90c.txt`, `801ef014.txt`,
//! `overlay_cutscene_dialogue_801d0d38.txt`) agree with the image.
//!
//! ## Shared conventions
//!
//! - `ctx[+0x54]` is the phase halfword; `ctx[+0x50]` is the system-actor
//!   handler id `FUN_801F159C` dispatches on; `ctx[+0x9E]` / `ctx[+0x9C]` are
//!   per-actor frame counters.
//! - Retiring an actor is always the same four stores: `scene[+0x2E] = -1`,
//!   `scene[+0x40] = ctx[+0x50]`, `ctx[+0x50] = <next handler id>`,
//!   `ctx[+0x54] = 0`, through the scene-struct pointer at `0x801C6EA4`. That
//!   is [`ActorExit`], and [`ActorExit::apply`] performs it. `scene[+0x2E]`
//!   and `scene[+0x3E]` are **different** halfwords - the exit clears the
//!   former and never touches the latter, which is what `case 5` of
//!   `FUN_801ED308` zeroes on its own (`sh zero,0x3e(v0)` at `0x801ED52C`
//!   against `sh v0,0x2e(v1)` at `0x801ED53C`).
//! - `_DAT_1F800393` is the scratchpad frame-delta byte every ramp scales by.
//!   It is `frame_delta` throughout.
//! - `_DAT_8007BB80` is the global input lock: while it is non-zero every
//!   picker phase returns without reading the pad.
//!
//! ## Wiring status
//!
//! All seven are hosted by `legaia_engine_world_views::world_map_panel_host`, which
//! hangs off `WorldMapController::panels` and is stepped once a frame by
//! `World::tick_world_map`. That host owns the scene-struct fields the
//! terminal arms write, the `0x801F2B98` descriptor array the panel scripts
//! address, the brightness / flash / cursor / slide globals the phases read,
//! and the story-flag bank the picker commits into.
//!
//! Two things it supplies are the **port's** rather than retail's, and the
//! host names each where it defines it: the panel-script table (retail's live
//! at overlay VAs the engine never loads) and the pad chords that install an
//! actor.
//!
//! An [`ActorExit`] is followed where the next handler's body is ported. The
//! dispatcher `FUN_801F159C` **is** ported, as
//! [`crate::submode_hub_actors::hub_dispatch`], and resolves an id through the
//! 52-entry `PTR_FUN_801F33B4`, whose slots [`crate::submode_hub_actors::slot`]
//! names. The fade/flash exits pick `0x29` and `0x2B` - the table's words
//! there are `FUN_801EE094` (Riremito) and `FUN_801EE328` (Rula) in the PROT
//! 0897 image - and the host continues as those travel arts. The sub-list,
//! text-box and flag-window exits hand back to `0x1A`
//! ([`crate::submode_hub_actors::slot::DRAW_TICK`]), the draw tick
//! `FUN_801F20B0`, which the host runs next: it installs the panel script at
//! `0x801F2A88`, pumps, and clears the completion gate so the actor retires.

use crate::world_map_panel::{CursorOutcome, CursorPad, list_cursor_input};

mod fade_flash;
pub use fade_flash::*;
mod sub_list;
pub use sub_list::*;
mod soft_reset;
pub use soft_reset::*;
mod fill_fade;
pub use fill_fade::*;
mod text_box;
pub use text_box::*;
mod flag_window;
pub use flag_window::*;
mod field_hud;
pub use field_hud::*;

/// The four stores every terminal panel arm makes through the scene struct at
/// `0x801C6EA4` before parking the actor.
///
/// PORT: FUN_801ed308 (cases 6/7), FUN_801ed590 (state 2), FUN_801ee5d4
/// (case 4), FUN_801ee90c (the `0x801EEA50` block), FUN_801ef014 (state 3)
///
/// Wired: `crate::world_map_panel_host::PanelActorHost::retire`
/// applies all four stores through [`ActorExit::apply`] and records the pair
/// in `PanelFrame::exits` - see the module wiring status for why the new
/// handler id is recorded and not dispatched, except id `0` (the close tick
/// `FUN_801F2134`), which the host follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorExit {
    /// `scene[+0x40]` takes the actor's *old* `ctx[+0x50]`.
    pub saved_handler: u16,
    /// The handler id written back into `ctx[+0x50]`.
    pub next_handler: u16,
}

impl ActorExit {
    /// `scene[+0x2E]`, written `-1` on every one of these paths.
    pub const SCENE_SLOT_CLEAR: i16 = -1;

    /// Perform the four stores against the caller's mirrors, in retail's
    /// order. Parameters are named for the offsets they stand in for.
    ///
    /// Read off `FUN_801ED308`'s two terminal arms, which are the shape every
    /// other one repeats (`0x801ED530..0x801ED578`): `li v0,-0x1;
    /// sh v0,0x2e(v1)` then `lhu v0,0x50(s0); sh v0,0x40(v1)`, then the shared
    /// tail `sh v0,0x50(s0); sh zero,0x54(s0)` with `v0` holding the arm's own
    /// next-handler immediate. Note `scene[+0x3E]` is **not** among them - the
    /// separate `case 5` arm is the only thing that clears it.
    ///
    /// Wired: `crate::world_map_panel_host::PanelActorHost::retire`
    /// applies every terminal arm's exit through here, reached from
    /// `PanelActorHost::tick` -> `World::tick_world_map_panels` ->
    /// `World::tick_world_map` -> `World::tick`.
    ///
    /// PORT: FUN_801ed308 (cases 6/7), FUN_801ed590 (state 2), FUN_801ee5d4 (case 4), FUN_801ee90c (the `0x801EEA50` block), FUN_801ef014 (state 3)
    pub fn apply(self, scene_2e: &mut i16, scene_40: &mut u16, ctx_50: &mut u16, ctx_54: &mut i16) {
        *scene_2e = Self::SCENE_SLOT_CLEAR;
        *scene_40 = self.saved_handler;
        *ctx_50 = self.next_handler;
        *ctx_54 = 0;
    }
}

#[cfg(test)]
mod tests;
