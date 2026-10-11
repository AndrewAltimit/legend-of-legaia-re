//! Battle HUD model - renderer-agnostic UI state for the in-battle screen.
//!
//! Holds per-slot HP / MP / AP / status-icon state plus a queue of damage
//! popups and battle-event log lines. The `engine-render` crate's
//! [`legaia_engine_render::battle_hud_draws_for`] turns one of these into
//! a `Vec<TextDraw>` for the GPU pipeline; engines that render via a
//! different path (web / terminal) can read the same struct directly.
//!
//! The HUD is fed by [`crate::world::World`] events:
//!
//! - `BattleEvent::ApplyArtStrike` → `push_damage_popup` (per-strike
//!   popup with a fade timer).
//! - `StatusEvent::TickDamage` / `Cleared` → `set_status_icons`.
//! - `BattleRound::begin/end` → `sync_from_world` to refresh HP / MP / AP.
//!
//! ## Frame timing
//!
//! Damage popups carry a `frames_remaining` counter; [`BattleHud::tick`]
//! decrements it each frame and drops popups whose counter reaches zero.
//! Default lifetime is 60 frames (~1 s at PSX 60 Hz). Simultaneous popups
//! are bounded by retail's 8-slot ring (`FUN_801F44A0`, delegated to
//! `legaia_engine_vm::battle_gauge_rearm::DamagePopupRing` by
//! [`BattleHud::push_popup`]): a ninth push overwrites the slot the ring
//! cursor names instead of growing the list.

use crate::ap_gauge::ApGauge;
use legaia_engine_battle_vm::status_effects::{StatusEffectTracker, StatusIcon, StatusKind};
use legaia_engine_vm::battle_gauge_rearm::DamagePopupRing;
pub use legaia_engine_vm::battle_gauge_rearm::POPUP_RING_SLOTS;
pub use legaia_engine_vm::battle_value_readout::ComboStyle;
use legaia_engine_vm::battle_value_readout::{COMBO_SLIDE_FRAMES, COMBO_SLIDE_IN_X, combo_slide};
mod plaques;
pub use plaques::*;
mod command_chips;
pub use command_chips::*;
mod subdraw;
pub use subdraw::*;

mod actor_names;
mod chrome;
mod hud_state;
mod phase;
mod slot;
mod world_sync;

pub use actor_names::*;
pub use chrome::*;
pub use hud_state::*;
pub use phase::*;
pub use slot::*;
pub use world_sync::*;

#[cfg(test)]
mod tests;
