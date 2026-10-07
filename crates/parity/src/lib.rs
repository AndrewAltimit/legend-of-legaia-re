//! `legaia-parity` - the engine measured against retail.
//!
//! The parity oracles and the retail comparison corpus: each drives the
//! native engine session and scores it against an emulator save state's own
//! RAM, VRAM or SPU, or against a recorded trace.
//!
//! - [`vram_oracle`], [`mode_trace_oracle`], [`audio_trace_oracle`],
//!   [`pcm_oracle`] - per-channel oracles behind `legaia-engine vram-oracle` /
//!   `mode-trace` / `audio-trace` / `pcm-trace`.
//! - [`sim_trace`] - the engine side of the frame-tagged differential oracle
//!   against the static recomp (`legaia-engine sim-trace`).
//! - [`retail_compare`] and its siblings - seed the engine from every walkable
//!   and battle library state and score scene / mode / position / camera /
//!   BGM / party / flags / bag / battle / frame against the state
//!   (`docs/tooling/retail-compare.md`).
//!
//! Tool code, never shipped in a play host: it reads emulator states and links
//! the wgpu renderer for frame captures.

pub mod audio_trace_oracle;
pub mod mode_trace_oracle;
pub mod pcm_oracle;
pub mod retail_compare;
pub mod retail_compare_battle;
pub mod retail_compare_cli;
pub mod retail_compare_image;
pub mod retail_compare_script;
pub mod sim_trace;
pub mod vram_oracle;

/// The session over the native cpal output, as `legaia-engine-shell` names it.
pub(crate) mod boot {
    pub use legaia_engine_session::boot::*;
    pub type BootSession = legaia_engine_session::boot::BootSession<legaia_engine_audio::AudioOut>;
}

pub(crate) use boot::{BootConfig, BootSession};
