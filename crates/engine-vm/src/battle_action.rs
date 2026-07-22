//! Battle action state machine, ported clean-room from `FUN_801E295C` (battle
//! overlay `0898`). Drives the per-actor execution of a chosen battle action -
//! the layer between "the player picked Attack" and "the actor's body has
//! finished swinging the sword and HP has been deducted."
//!
//! PORT: FUN_801E295C, FUN_8003F2B8, FUN_8004E2F0, FUN_801D5854, FUN_801D8DE8
//! PORT: FUN_801DABA4, FUN_801DBF9C, FUN_801DC0A0, FUN_801E7320, FUN_801EED1C, FUN_801EFE44
//!
//! See [`docs/subsystems/battle-action.md`](../../../docs/subsystems/battle-action.md)
//! for the byte-level reference. This is **not** a bytecode VM. It's a
//! per-frame edge-triggered state machine: each `case ctx.action_state` body
//! waits on a per-actor condition (animation matched, timer expired, distance
//! check passed) and writes the next `action_state` value when ready. Actions
//! that need multiple frames (most) do nothing on the frames where their
//! condition isn't met yet.
//!
//! ## Three nested keys
//!
//! 1. **Action category** - `actor.action_category` (was `actor[+0x1DE]`):
//!    0=Tactical Arts, 1=Item, 2=Magic, 3=Attack, 4=Spirit, 5=Run/Defend.
//! 2. **Execution phase** - `ctx.action_state` (was `ctx[7]`).
//! 3. **Per-actor sub-state** - `actor.flag_bits` and the per-action parameter
//!    byte stream `actor.params[..]`.
//!
//! ## Clean-room boundary
//!
//! No bytes from `SCUS_942.54` or any overlay live here. The Ghidra
//! decompilation at `ghidra/scripts/funcs/overlay_battle_action_801e295c.txt`
//! is the *spec*, not source. The [`BattleActionHost`] trait abstracts every
//! call the original made into the engine layer. Tests use synthetic ctx /
//! actor state.
#![allow(clippy::too_many_arguments)]

mod types;
pub use types::*;

mod host;
pub use host::*;

mod dispatch;
pub use dispatch::*;

mod attack;
use attack::*;

mod magic;
use magic::*;

mod summon;
use summon::*;

mod spirit;
use spirit::*;

mod done;
use done::*;

mod run;
use run::*;

mod enemy_budget;
pub use enemy_budget::*;

mod validator;
pub use validator::*;

mod pool_ops;
pub use pool_ops::*;

mod queue_applier;
pub use queue_applier::*;

#[cfg(test)]
mod tests;
