//! `legaia-engine-session` - the per-frame game session both play hosts run.
//!
//! The host-neutral modules are free of wgpu, winit and cpal, so the native
//! window (`engine-shell`) and the browser play page (`web-play`) link the
//! same code. A host supplies only what it owns: the audio output (any
//! [`legaia_engine_audio::AudioSink`]), the GPU, and its input source.
//!
//! - [`battle_fx`] - the move-FX spawn-to-sound step.
//! - [`battle_stage`] - a battle's stage, resolved on the battle edge, and the
//!   world state it leaves (stage class, fog cap, monster texture slots).
//! - [`battle_vram`] - the battle VRAM's per-tick re-stamps: facial animation,
//!   the status / effect CLUT steps, the stage module's moves.
//! - [`front_end`] - the boot front end (publisher logos, title card,
//!   attract hand-off) both play hosts and the soak harness step.
//! - [`title_continue`] - the title's Continue screen (the save-select over
//!   the two card ports) both play hosts and the soak harness step.
//! - [`audio_director`] - the BGM / SFX / CD-XA director over an `AudioSink`.
//! - [`scene_actors`] - the scene-entry actor binding both hosts run before
//!   building meshes (the lead's locomotion clips, NPC clip cursors, floor
//!   snap).
//! - [`session`] - [`BootSession`], the scene host plus the per-frame order
//!   (mode seat, menus, camera halves, audio routing) around its tick.
//! - [`hotkeys`] - the option / cheat / camera keys both play hosts bind
//!   outside the pad vocabulary, and the options edit each one makes.
//! - [`host_trace`] - one host-agnostic sample of the simulation, for
//!   comparing two hosts tick by tick.
//! - [`mode_trace`] - one sample of a session's dispatch state.
//! - [`xa_banks`] - the CD-XA voice banks' staging, shared by both hosts.
//! - [`native`] - native targets only: the session over the cpal output,
//!   opened from a path, and the CD-XA banks read off a disc image on disk.

pub mod audio_director;
pub mod battle_fx;
pub mod battle_intro;
pub mod battle_presentation;
pub mod battle_stage;
pub mod battle_vram;
pub mod front_end;
pub mod host_trace;
pub mod hotkeys;
pub mod mode_trace;
#[cfg(not(target_arch = "wasm32"))]
pub mod native;
pub mod party_spec;
pub mod scene_actors;
pub mod session;
pub mod session_switches;
pub mod title_continue;
pub mod xa_banks;

pub use audio_director::AudioDirector;
pub use front_end::{AttractLane, FrontEnd, FrontEndScreen, FrontEndStep, NoMovieLane};
pub use session::{
    BootConfig, BootSession, NewGameHost, PauseMenuPress, TickClaims, TickOwner, start_new_game_on,
};
pub use title_continue::{CardPorts, ContinueLoad, ContinueScreen, ContinueStep};
