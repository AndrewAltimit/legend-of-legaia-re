//! The battle side of the menu front end, minus `World`: the command-flow
//! state byte (`FUN_801D0748`), player battle command input, the open
//! banner, Koru's timed-fight strip and the Muscle Dome match session.
//!
//! These modules name each other, the battle kernels below them
//! (`legaia-engine-battle`, `legaia-engine-battle-vm`), the Muscle Dome
//! tables (`legaia-engine-minigames`) and the option value enums in
//! `legaia-engine-system` - no pause, shop or save screen. The item-use
//! session the live loop opens from the Item command is
//! `legaia-engine-menus`' `inventory_use`, a sibling. Doc comments name
//! `engine-core` items in plain code spans, since rustdoc cannot resolve a
//! link into a dependent crate. See the crate README for the module map.

#![forbid(unsafe_code)]

pub mod battle_flow;
pub mod battle_input;
pub mod battle_open;
pub mod muscle_dome_session;
pub mod timed_fight;
