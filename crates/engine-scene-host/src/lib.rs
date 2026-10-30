//! The scene host and what a host runs around it: [`scene_host::SceneHost`]
//! owns a `World` and the `ProtIndex`, enters scenes and ticks them, and
//! drains what a tick queues (door warps, BGM routing, the FMV hand-off).
//! Beside it sit where a save resumes or a New Game starts ([`resume`]), the
//! headless live-scene preview ([`scene_live`]), the pack maps prepared ahead
//! of their entry ([`expansion_preload`]), the disc readers the scene
//! entry feeds kernels from ([`disc_readers`]) and the fishing venue's host
//! seam ([`fishing_venue`]).
//!
//! `World` and everything that reads or writes it - the tick, the frame-step
//! rules, the battle loop - is `legaia-engine-core`; this crate sits above it
//! and below the play hosts.

pub mod disc_readers;
pub mod expansion_preload;
pub mod fishing_venue;
pub mod place_name_banner;
pub mod resume;
pub mod scene_host;
pub mod scene_live;

// `tests/*.rs` compile into this crate's unit-test target rather than an
// integration binary of their own: an integration binary links the non-test
// library, so a `cargo test` would build the crate twice (once as the
// library, once in test mode for the `src/` unit tests) with the two on one
// critical path. The alias lets those files keep naming the crate as
// `legaia_engine_scene_host::`, the public paths a downstream crate sees.
#[cfg(test)]
extern crate self as legaia_engine_scene_host;
#[cfg(test)]
#[path = "../tests/integration.rs"]
mod integration;
