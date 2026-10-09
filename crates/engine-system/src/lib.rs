//! PSX runtime-system kernels: pad input and the retail pad pump, the
//! MDEC DMA sync, the streaming-chunk installer, the RAM-cell registry,
//! global sound state and the movie score policy, FMV helpers, BGM labels,
//! codified capture observations, the per-draw census, scene-name sync,
//! the fade actor and ramp, the pause wipe and the mode-entry initialisers:
//! the `World`-free system layer under the engine.
//!
//! Every module's whole dependency closure inside the engine is in this
//! crate or below it, so it sits strictly below `legaia-engine-core`,
//! which re-exports each module at its old path. Doc links that pointed
//! back up at `engine-core` are plain code spans, since rustdoc cannot
//! resolve a link into a dependent crate. See the crate README for the
//! module map.

#![forbid(unsafe_code)]

#[cfg(test)]
use legaia_engine_battle::retail_magic;
#[cfg(test)]
use legaia_engine_battle::spells;

pub mod capture_observations;
pub mod chunk_install;
pub mod cutscene;
pub mod draw_census;
pub mod fade;
pub mod fade_ramp;
pub mod input;
pub mod mdec_dma_sync;
pub mod mode_entry_init;
pub mod movie_audio;
pub mod music_labels;
pub mod pause_wipe;
pub mod ram_map;
pub mod retail_pad;
pub mod scene_name_sync;
pub mod sound_state;
