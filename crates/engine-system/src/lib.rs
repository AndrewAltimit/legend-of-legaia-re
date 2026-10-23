//! PSX runtime-system kernels: pad input and the retail pad pump, the
//! MDEC DMA sync, the streaming-chunk installer, the RAM-cell registry,
//! global sound state and the movie score policy, FMV helpers, BGM labels,
//! codified capture observations, the per-draw census, scene-name sync,
//! the fade actor and ramp, the pause wipe, the mode-entry initialisers and
//! the options screen's config-word value enums:
//! the `World`-free system layer under the engine.
//!
//! Every module's whole dependency closure inside the engine is in this
//! crate or below it, so it sits strictly below `legaia-engine-core`. Doc
//! comments name `engine-core` items in plain code spans, since rustdoc
//! cannot resolve a link into a dependent crate. See the crate README for
//! the module map.

#![forbid(unsafe_code)]

pub mod chunk_install;
pub mod draw_census;
pub mod fade;
pub mod fade_ramp;
pub mod gamepad;
pub mod incense_window;
pub mod input;
pub mod menu_input;
pub mod mode_entry_init;
pub mod movie;
pub mod music_labels;
pub mod option_values;
pub mod pad;
pub mod pause_wipe;
pub mod ram_map;
pub mod rand;
pub mod retail_pad;
pub mod rotation_lut;
pub mod scene_name_sync;
pub mod scus_leaf_kernels;
pub mod sound_state;
pub mod story_flag_state;
pub mod trig;
