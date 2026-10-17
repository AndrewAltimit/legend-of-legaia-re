//! The slot-B cast / summon modules' tick bodies, their own camera and
//! countdown, and the cast-arm kernels the battle action SM pages them
//! through.
//!
//! Everything here is `World`-free and sits on `legaia-engine-battle-vm`:
//! the action SM, the battle formulas and the battle camera script the cast
//! bodies call are that crate's, and nothing in it names a module here.
//! `engine-core`'s cast band is the caller.
//!
//! Doc comments name modules of crates above this one in plain code spans,
//! since rustdoc cannot resolve a link into a dependent crate. See the crate
//! README for the module map.

#![forbid(unsafe_code)]

pub mod cast_arm_ticks;
pub mod cast_fatal_decision;
pub mod cast_module_camera;
pub mod cast_module_ticks;
pub mod seru_magic_ticks;
