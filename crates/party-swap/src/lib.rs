//! Party <-> Delilas battle-model swap kernels: a playable character's
//! assembled battle model rebuilt on a Delilas monster rig and a Delilas
//! model on a player rig (anatomy permutation, extras merge, pivot-anchored
//! rest-pose bake), plus the field form, win poses, movesets, weapon fuse
//! and enemy-animation remap the swap needs. Pure transforms over decoded
//! disc assets; the disc writes stay in `legaia-patcher`.
//!
//! `legaia-patcher` re-exports every module here at its old path. Doc links
//! that pointed back up at it are plain code spans, since rustdoc cannot
//! resolve a link into a dependent crate.

#![forbid(unsafe_code)]

pub mod party_swap;
