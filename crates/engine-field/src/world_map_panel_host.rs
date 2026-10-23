//! Host for the world-map band's **panel windows and panel actors**.
//!
//! [`crate::world_map_panel`] ports the three shared leaves of the
//! field overlay's world-map band - the panel command-script interpreter
//! (`FUN_801E9B3C`), the shared list cursor (`FUN_801E9DC8`) and the dev-menu
//! row-action dispatcher (`FUN_801EA9B0`) - and
//! [`crate::world_map_panel_actors`] ports the six `ctx[+0x54]`
//! phase machines that sit on top of them. Both were hostless: they modelled a
//! screen the engine did not have. This module is that screen's simulation
//! half, hung off [`crate::world_map::WorldMapController`] and driven once a
//! frame by `World::tick_world_map`.
//!
//! # What is retail here and what is the port's
//!
//! **Retail** - every phase machine, every effect ordering, the window-script
//! opcode set, the cursor kernel, the panel geometry arithmetic, and the
//! record fields the restore arm writes. None of that is re-derived here; this
//! module only supplies inputs and applies outputs.
//!
//! **The port's** - four things retail keeps in overlay data, in a dispatcher
//! this crate does not own, or outside the actor entirely:
//!
//! 1. **The panel scripts.** Retail's window scripts live at overlay VAs
//!    (`0x801F3274`, `0x801F3284`, `0x801F32B4`, `0x801F32DC`, `0x801F2A88`,
//!    `0x801F3304`) that the engine never loads. [`PanelScripts::default`]
//!    ships a minimal stand-in keyed by the same VAs, so the interpreter runs
//!    on real records; a host with the overlay image can replace the table.
//!    This is the same seam `legaia_engine_menus::dev_menu_host::DevMenuSession::flag_tags`
//!    has for the debug flag table.
//! 2. **The handler-id table.** Retail retires an actor by writing a new
//!    handler id into `ctx[+0x50]`, which `FUN_801F159C` turns back into a
//!    function pointer through the 52-entry `PTR_FUN_801F33B4`. The
//!    dispatcher itself is ported
//!    ([`crate::submode_hub_actors::hub_dispatch`]) and takes the resolved
//!    handler as a closure. [`ActorExit::apply`] makes all four stores here;
//!    [`PanelActorHost::retire`] then follows the ids whose bodies are ported:
//!    `0`, the close tick ([`PanelActorKind::CloseTick`]), and `0x29` /
//!    `0x2B`, the fade/flash exits' travel arts (table words `FUN_801EE094` /
//!    `FUN_801EE328`), and `0x1A`, the draw tick the sub-list / text-box /
//!    flag-window exits hand back to ([`PanelActorKind::DrawTick`]). It drops
//!    the actor on any other id, recording the pair in [`PanelFrame::exits`].
//! 3. **Which pad chord installs which actor.** Retail reaches these from
//!    debug branches in the world-map controller. The engine's bindings live
//!    on `World::tick_world_map_panels` and are tabulated there.
//! 4. **Entry phase and dismissal.** Several of these machines park rather
//!    than exit, and retail releases them from outside: the scene manager
//!    watching `scene[+0x3E]`, the executable reload.
//!    [`PanelActorKind::entry_phase`] picks the arm the host wants to run and
//!    [`PanelActorHost::dismiss`] is the escape hatch, without which the first
//!    parking arm wedges the screen.
//!
//! The fade/flash actor's park is **not** in that list any more. Its releaser
//! is identified: the actor's own phase-1 arm hands off to the in-field
//! save/load screen driver ([`SaveScreenHandoff`]), the menu overlay's
//! save-side UI writes both shared globals while the field overlay is paged
//! out, and the counter it leaves selects which of the two terminal arms the
//! actor takes. [`PanelActorHost::release_flash_with`] is that write, and it
//! refuses to fire without an outstanding hand-off, because in retail nothing
//! else stores to `_DAT_8007B43C` at all.
//!
//! # The pad layout trap
//!
//! Every kernel in this band reads the **packed** pad words
//! (`_DAT_8007BB84` newly-pressed, `_DAT_8007B874` held) that `FUN_8001822C`
//! builds, not the raw BIOS layout [`legaia_engine_system::input::PadButton`] carries. The
//! two hold the same 16 buttons with the byte halves swapped, so feeding a raw
//! word straight in cross-wires the whole screen: raw Cross (`0x4000`) arrives
//! as packed D-pad Down, and raw Up (`0x0010`) arrives as nothing at all.
//! [`packed_pad`] does the conversion and every entry point here takes packed
//! words only.

use crate::travel_art_actor::{
    TravelArt, TravelArtActor, TravelDestination, destination_for, find_visited_map,
};
use crate::world_map_panel::{
    CursorPad, PanelCommand, PanelDescriptor, PanelEffect, run_panel_script,
};
use crate::world_map_panel_actors::{
    ActorExit, BRIGHTNESS_MAX, FLASH_COUNTER_DONE_STEP, FLASH_COUNTER_SEED_A, FLASH_COUNTER_SEED_B,
    FadeFlashEffect, FadeFlashInput, FillFadeEffect, FillFadeInput, FlagWindowDescriptor,
    FlagWindowEffect, FlagWindowInput, HudDecision, HudInput, SubListEffect, SubListInput,
    TextBoxEffect, TextBoxInput, fade_flash_tick, field_hud_tick, fill_fade_tick, flag_window_tick,
    soft_reset_tick, sub_list_tick, text_box_tick,
};
use std::collections::HashMap;

mod actor_host;
mod field_party_hud;
mod panel_consts;
mod save_handoff;
mod window_host;

pub use actor_host::*;
pub use field_party_hud::*;
pub use panel_consts::*;
pub use save_handoff::*;
pub use window_host::*;

#[cfg(test)]
mod tests;
