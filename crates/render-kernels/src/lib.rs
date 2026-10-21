//! The wgpu-free render kernels both play hosts share: GTE fixed-point
//! math, the screen-space PSX primitive record with its ordering-table sort
//! and vertex builder, a CPU rasteriser for it, the frame-to-VRAM capture,
//! retail's per-primitive near reject, billboard / afterimage / streak /
//! weapon-trail / cast-beam / scanline-strip emitters, the battle-intro
//! transition emitter, the battle numerals and the enhanced scene lighting.
//!
//! `legaia-engine-ui` sits one crate up. Doc comments here name its items in
//! plain code spans, since rustdoc cannot resolve a link into a dependent
//! crate.

#![forbid(unsafe_code)]

pub mod afterimage;
pub mod battle_intro;
pub mod battle_intro_particles;
pub mod battle_intro_styles;
pub mod battle_intro_swirl;
pub mod battle_intro_tiles;
pub mod battle_intro_transition;
pub mod battle_trail;
pub mod billboard;
// --- lane L ---
pub mod bloom;
pub mod cast_aluru;
pub mod cast_beam;
pub mod cast_colour_wash;
pub mod cast_rain;
pub mod cast_swordie;
pub mod cast_theeder_packet;
pub mod display_mirror;
pub mod drop_shadow;
pub mod effect_billboard;
pub mod field_light;
pub mod fog_volume;
pub mod gte;
pub mod move_strip;
pub mod overworld_curvature;
pub mod prim_dispatch;
pub mod prim_near_reject;
pub mod scene_lighting;
pub mod screen_prim;
pub mod screen_prim_raster;
pub mod shatter;
pub mod shutter_bars;
pub mod sprite_arm;
pub mod streak_pass;
pub mod ui_tint;
pub mod vram_capture;
pub mod world_map_sky;
