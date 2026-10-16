//! Effect kernels: the action-effect script, the effect arms and ribbons,
//! the summon creature's effect side, the screen-wide effects, the object
//! effect table and the per-part motion blocks - the `World`-free half of the
//! engine's effect layer.
//!
//! Every module's whole dependency closure inside the engine is in this
//! crate (plus `legaia-engine-vm` and the asset crates), so it sits strictly
//! below `legaia-engine-core`. Doc comments name `engine-core` items in
//! plain code spans, since rustdoc cannot resolve a link into a dependent
//! crate. See the crate README for the module map.

#![forbid(unsafe_code)]

pub mod action_effect_script;
pub mod effect_default_arm;
pub mod effect_ribbon;
pub mod effect_sprite_arm;
pub mod object_effect;
pub mod part_motion;
pub mod part_shatter;
pub mod screen_fx;
pub mod summon;
