//! Battle HUD model - renderer-agnostic UI state for the in-battle screen.
//!
//! Holds per-slot HP / MP / AP / status-icon state plus a queue of damage
//! popups and battle-event log lines. The `engine-render` crate's
//! `legaia_engine_ui::battle_hud_draws_for` turns one of these into
//! a `Vec<TextDraw>` for the GPU pipeline; engines that render via a
//! different path (web / terminal) can read the same struct directly.
//!
//! The HUD is fed by [`crate::world::World`] events:
//!
//! - `BattleEvent::ApplyArtStrike` → `push_damage_popup` (per-strike
//!   popup with a fade timer).
//! - `StatusEvent::TickDamage` / `Cleared` → `set_status_icons`.
//! - `BattleRound::begin/end` → `sync_from_world` to refresh HP / MP / AP.

use legaia_engine_battle::battle_hud_state::*;
use legaia_engine_battle_vm::battle_value_readout::ComboStyle;

mod plaques;
pub use plaques::*;
mod command_chips;
pub use command_chips::*;

mod actor_names;
mod chrome;
mod phase;
mod world_sync;

pub use actor_names::*;
pub use chrome::*;
pub use phase::*;
pub use world_sync::*;

#[cfg(test)]
mod tests;
