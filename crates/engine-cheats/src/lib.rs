//! Cheats applied to a running `World`: the player-facing cheat list both
//! play hosts expose ([`cheats`]) and the GameShark / Pro-Action-Replay code
//! applier behind `play-window --cheat-file` ([`cheat_applier`]).
//!
//! Both only write a `World` the host already owns, and nothing in the
//! simulation calls them, so the crate sits directly above
//! `legaia-engine-core` and a cheat change rebuilds only the hosts. The
//! cheat database parser itself is `legaia-cheats`.

#![forbid(unsafe_code)]

pub mod cheat_applier;
pub mod cheats;
