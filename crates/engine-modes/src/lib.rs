//! Rules engines for the port's own game modes - content retail never had
//! (poker, golf, racing, trading-card battles, prop hunt). Each is a
//! deterministic, `World`-free session ticked once per frame from pad
//! input, the same contract `engine-minigames` keeps for the retail
//! minigames, so a mode replays bit-identically and can run in lockstep
//! across networked players.
//!
//! The shared contract is [`session::ModeSession`]; the integer math every
//! physics kernel uses is [`fixed`]. Each mode builds its config from the
//! matching `modes.toml` / `cards.toml` instance of an expansion pack.

#![forbid(unsafe_code)]

pub mod active;
pub mod cards;
pub mod fixed;
pub mod golf;
pub mod map_frame;
pub mod poker;
pub mod prop_hunt;
pub mod racing;
pub mod session;
