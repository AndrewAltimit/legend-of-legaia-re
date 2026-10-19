//! The battle HUD's reads of a running `World`: what each frame of the
//! in-battle screen shows, answered off the battle state.
//!
//! The HUD model itself (`BattleHud`: per-slot HP / MP / AP / status rows,
//! damage popups, the event log) is `legaia_engine_battle::battle_hud_state`;
//! the draw lists are `legaia_engine_ui`'s. This crate is the projection
//! between the simulation and both: [`sync_battle_hud_rows`] refreshes the
//! model's rows from the world, and the predicates and offsets (the phase
//! word, the command chips and commit log, the plaques and their glides, the
//! banners and badges) are what both play hosts and `engine-screens` hand to
//! the builders.
//!
//! It sits directly above `legaia-engine-core` and nothing in the simulation
//! names it, so a HUD change rebuilds the hosts but not `World`. The two
//! facts the simulation itself needs from the battle screen - whether the
//! special word forbids the Ra-Seru chip, and where the result windows'
//! raise glide stands - are `BattleState::raseru_forbidden` and
//! `BattleState::result_windows_dy` in engine-core.

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
