//! `legaia-engine-shell` - the top-level engine driver crate.
//!
//! Houses the `legaia-engine` binary plus a small wiring layer that bridges
//! [`legaia_engine_core`] and [`legaia_engine_audio`] (the BGM director) so
//! the binary and any embedding can share the same per-scene plumbing.
//!
//! The parity oracles and the retail comparison corpus live in
//! `legaia-parity`, which the binary's trace subcommands drive.

/// The BGM / SFX director lives in `legaia-engine-session` (shared with the
/// browser host); this host instantiates it over the cpal output.
pub mod bgm {
    pub use legaia_engine_session::bgm::*;
    /// The director over the native cpal output.
    pub type AudioBgmDirector =
        legaia_engine_session::bgm::AudioBgmDirector<legaia_engine_audio::AudioOut>;
}
/// The session `BootSession` lives in `legaia-engine-session` (shared with the
/// browser host); this host instantiates it over the cpal output.
pub mod boot {
    pub use legaia_engine_session::boot::*;
    /// The session over the native cpal output.
    pub type BootSession = legaia_engine_session::boot::BootSession<legaia_engine_audio::AudioOut>;
}
pub mod cutscene_av;
pub mod host_setup;
pub mod launcher;
pub mod replay;
pub mod scenarios;
pub mod tile_board_draws;
pub mod window;
pub mod xa_clip;

pub use bgm::AudioBgmDirector;
pub use boot::{BootConfig, BootSession};
