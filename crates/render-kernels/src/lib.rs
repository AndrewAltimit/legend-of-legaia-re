//! The wgpu-free render kernels both play hosts share: GTE fixed-point
//! math, the screen-space PSX primitive record with its ordering-table sort
//! and vertex builder, a CPU rasteriser for it, the frame-to-VRAM capture,
//! retail's per-primitive near reject, billboard / afterimage / streak /
//! weapon-trail / cast-beam / scanline-strip emitters, the battle-intro
//! transition emitter, the battle numerals and the enhanced scene lighting.
//!
//! `legaia-engine-ui` re-exports every module here at its old path. Doc links
//! that pointed back up at it are plain code spans, since rustdoc cannot
//! resolve a link into a dependent crate.

#![forbid(unsafe_code)]

pub mod afterimage;
pub mod battle_intro;
pub mod battle_numerals;
pub mod battle_trail;
pub mod billboard;
pub mod cast_beam;
pub mod cast_theeder;
pub mod effect_billboard;
pub mod gte;
pub mod move_strip;
pub mod prim_near_reject;
pub mod scene_lighting;
pub mod screen_prim;
pub mod screen_prim_raster;
pub mod streak_pass;
pub mod vram_capture;
