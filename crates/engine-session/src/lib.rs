//! `legaia-engine-session` - the per-frame game session both play hosts run.
//!
//! The host-neutral modules are free of wgpu, winit and cpal, so the native
//! window (`engine-shell`) and the browser play page (`web-viewer`) link the
//! same code. A host supplies only what it owns: the audio output (any
//! [`legaia_engine_audio::AudioSink`]), the GPU, and its input source.
//!
//! - [`battle_fx`] - the move-FX spawn-to-sound step.
//! - [`audio_director`] - the BGM / SFX / CD-XA director over an `AudioSink`.
//! - [`session`] - [`BootSession`], the scene host plus the per-frame order
//!   (mode seat, menus, camera halves, audio routing) around its tick.
//! - [`host_trace`] - one host-agnostic sample of the simulation, for
//!   comparing two hosts tick by tick.
//! - [`mode_trace`] - one sample of a session's dispatch state.
//! - [`xa_banks`] - the CD-XA voice banks' staging, shared by both hosts.
//! - [`native`] - native targets only: the session over the cpal output,
//!   opened from a path, and the CD-XA banks read off a disc image on disk.

pub mod audio_director;
pub mod battle_fx;
pub mod host_trace;
pub mod mode_trace;
#[cfg(not(target_arch = "wasm32"))]
pub mod native;
pub mod session;
pub mod xa_banks;

pub use audio_director::AudioDirector;
pub use session::{BootConfig, BootSession, PauseMenuPress};
