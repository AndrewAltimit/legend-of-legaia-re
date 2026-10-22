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
//! - [`cast_sequence`] - one cast scored against retail frame by frame,
//!   paired on the action-SM clock (`legaia-engine cast-sequence-score`,
//!   `docs/tooling/cast-sequence-oracle.md`).
//! - [`window_child`] - spawn a `play-window` child, retrying once the X11
//!   startup race (`SIGSEGV` inside libXi before the event loop opens).
//!
//! Tool code, never shipped in a play host: it reads emulator states and links
//! the wgpu renderer for frame captures.

pub mod audio_trace_oracle;
pub mod capture_observations;
pub mod cast_sequence;
pub mod mode_trace_oracle;
pub mod pcm_oracle;
pub mod retail_compare;
pub mod retail_compare_battle;
pub mod retail_compare_cli;
pub mod retail_compare_image;
pub mod retail_compare_script;
pub mod sim_trace;
pub mod vram_oracle;
pub mod window_child;
