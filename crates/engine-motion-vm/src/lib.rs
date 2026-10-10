//! The two per-actor motion VMs: the pursue / patrol / face-target VM
//! (`FUN_8003774C`, [`motion_vm`]) and the scripted-motion VM that walks the
//! bytecode in a scene MAN's tail-section 1 (`FUN_80038158`,
//! [`ambient_motion`], its effect arms in [`ambient_motion_ops`]).
//!
//! Ported from the routines' disassembly like the rest of
//! `legaia-engine-vm`, with no bytes from the original executable. Every
//! module names only its siblings, `legaia-asset` (the MAN motion-script
//! opcode widths) and `legaia-engine-battle-vm` (the PsyQ `rand` step and the
//! bearing LUT), so the crate sits below `legaia-engine-vm`, which re-exports
//! each module at its old path. Doc links that pointed up at it are plain
//! code spans, since rustdoc cannot resolve a link into a dependent crate.
//! See the crate README for the module map.

#![forbid(unsafe_code)]

// Modules these files name as `crate::...` / `super::...`, which
// `legaia-engine-vm` re-exports at its root; binding them here keeps the
// moved files' paths unchanged.
#[allow(unused_imports)] // named by rustdoc links only
use legaia_engine_battle_vm::battle_action;
use legaia_engine_battle_vm::battle_formulas;

pub mod ambient_motion;
pub mod ambient_motion_ops;
pub mod motion_vm;
