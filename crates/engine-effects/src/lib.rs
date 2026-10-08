//! Effect kernels: the action-effect script, the effect arms and ribbons,
//! the summon creature's effect side, the screen-wide effects, the object
//! effect table and the per-part motion blocks - the `World`-free half of the
//! engine's effect layer.
//!
//! Every module's whole dependency closure inside the engine is in this
//! crate (plus `legaia-engine-vm` and the asset crates), so it sits strictly
//! below `legaia-engine-core`, which re-exports each module at its old path
//! (`legaia_engine_core::summon`, ...). Doc links that point back up at
//! `engine-core` are plain code spans, since rustdoc cannot resolve a link
//! into a dependent crate. See the crate README for the module map.

#![forbid(unsafe_code)]

// Kernels these modules name as `crate::...`, which engine-core re-exports at
// its root; binding them here keeps the moved files' paths unchanged.
use legaia_engine_battle::retail_magic;
use legaia_engine_minigames::baka_impact_fx;

pub mod action_effect_script;
pub mod effect_default_arm;
pub mod effect_ribbon;
pub mod effect_sprite_arm;
pub mod object_effect;
pub mod part_motion;
pub mod screen_fx;
pub mod summon;
