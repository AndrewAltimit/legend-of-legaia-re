//! `legaia-engine-shell` — the top-level engine driver crate.
//!
//! Houses the `legaia-engine` binary plus a small wiring layer that bridges
//! [`legaia_engine_core`] and [`legaia_engine_audio`] (the BGM director) so
//! the binary and any embedding can share the same per-scene plumbing.

pub mod bgm;
pub mod boot;

pub use bgm::AudioBgmDirector;
pub use boot::{BootConfig, BootSession};
