//! `legaia-engine-session` - the per-frame game session both play hosts run.
//!
//! Everything here is free of wgpu, winit and cpal, so the native window
//! (`engine-shell`) and the browser play page (`web-viewer`) link the same
//! code. A host supplies only what it owns: the audio output (any
//! [`legaia_engine_audio::AudioSink`]), the GPU, and its input source.
//!
//! - [`battle_fx`] - the move-FX spawn-to-sound step.
//! - [`audio_director`] - the BGM / SFX / CD-XA director over an `AudioSink`.
//! - [`boot`] - [`BootSession`], the scene host plus the per-frame order
//!   (mode seat, menus, camera halves, audio routing) around its tick.

pub mod audio_director;
pub mod battle_fx;
pub mod boot;
pub mod mode_trace;
pub mod xa_banks;

pub use audio_director::AudioBgmDirector;
pub use boot::{BootConfig, BootSession, PauseMenuPress};
