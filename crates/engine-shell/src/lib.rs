//! `legaia-engine-shell` - the top-level engine driver crate.
//!
//! Houses the `legaia-engine` binary plus a small wiring layer that bridges
//! [`legaia_engine_core`] and [`legaia_engine_audio`] (the BGM director) so
//! the binary and any embedding can share the same per-scene plumbing.

pub mod audio_trace_oracle;
pub mod bgm;
pub mod boot;
pub mod cutscene_av;
pub mod mode_trace_oracle;
pub mod pcm_oracle;
pub mod replay;
pub mod scenarios;
pub mod sim_trace;
pub mod tile_board_draws;
pub mod vram_oracle;

pub use bgm::AudioBgmDirector;
pub use boot::{BootConfig, BootSession};
