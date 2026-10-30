//! Multiplayer for the port: the wire protocol, the lockstep and
//! host-authoritative session cores, transports, and the relay that joins
//! players into rooms.
//!
//! The crate names no engine crate. A session drives anything that
//! implements [`lockstep::Simulation`] (per-frame pad words, a state hash) or
//! [`authority::EventSim`] (discrete events), so a game-mode session and a
//! whole `World` plug in the same way. The protocol and the session cores
//! do no I/O and build for `wasm32`; the socket transports and the relay
//! server are native-only, and the browser transport sits behind the `web`
//! feature.
//!
//! See `docs/subsystems/netplay.md` for the protocol, the two simulation
//! models, the "host a world" flow and the security posture.

#![forbid(unsafe_code)]

pub mod authority;
pub mod codec;
pub mod lockstep;
pub mod protocol;
pub mod relay;
pub mod transport;
pub mod websocket;
pub mod wire;

/// The relay's default port.
pub const DEFAULT_PORT: u16 = 7462;
