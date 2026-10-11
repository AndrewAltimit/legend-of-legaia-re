//! The two per-actor motion VMs: the pursue / patrol / face-target VM
//! (`FUN_8003774C`, [`motion_vm`]) and the scripted-motion VM that walks the
//! bytecode in a scene MAN's tail-section 1 (`FUN_80038158`,
//! [`ambient_motion`], its effect arms in [`ambient_motion_ops`]).
//!
//! Ported from the routines' disassembly like the rest of
//! `legaia-engine-vm`, with no bytes from the original executable. Every
//! module names only its siblings, `legaia-scene-formats` (the MAN
//! motion-script opcode widths) and `legaia-engine-system` (the PsyQ `rand`
//! step), so the crate sits below `legaia-engine-vm`. Doc comments name its
//! items in plain code spans, since rustdoc cannot resolve a link into a
//! dependent crate. See the crate README for the module map.

#![forbid(unsafe_code)]

pub mod ambient_motion;
pub mod ambient_motion_ops;
pub mod motion_vm;
