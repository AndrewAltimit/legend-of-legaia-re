//! The scene host and what a host runs around it: [`scene_host::SceneHost`]
//! owns a `World` and the `ProtIndex`, enters scenes and ticks them, and
//! drains what a tick queues (door warps, BGM routing, the FMV hand-off).
//! Beside it sit where a save resumes or a New Game starts ([`resume`]), the
//! headless live-scene preview ([`scene_live`]), the disc readers the scene
//! entry feeds kernels from ([`disc_readers`]) and the fishing venue's host
//! seam ([`fishing_venue`]).
//!
//! `World` and everything that reads or writes it - the tick, the frame-step
//! rules, the battle loop - is `legaia-engine-core`; this crate sits above it
//! and below the play hosts.

pub mod disc_readers;
pub mod fishing_venue;
pub mod place_name_banner;
pub mod resume;
pub mod scene_host;
pub mod scene_live;
