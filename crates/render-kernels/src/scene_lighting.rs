//! **Enhanced lighting** - the one engine-side source of truth both hosts
//! light a scene from: the derived point-light list, the emissive surface
//! set, the scene mood (ambient / key light / time of day) and the glow
//! sprites that stand in for bloom.
//!
//! # This is NOT retail
//!
//! Retail field rendering has no light source at all: both TMD renderers
//! issue exactly one GTE colour op (`DPCS`, the depth cue) and never an
//! `NC*` lighting op, so all field shading is baked into the TMD colour
//! words and applied as `texel * colour / 128`. Everything in this module
//! feeds a deliberate enhancement layered over that baked shading, behind
//! one runtime toggle per host (`I` in `play-window`, the "Enhanced
//! lighting" checkbox on the browser play page). With the toggle off no
//! host reads any of it and the render is pixel-identical to the faithful
//! path.
//!
//! The native renderer (`engine-render`) and the browser play page
//! (`site/js/webgl-*.js`, fed by `web-viewer`) consume the *same* values
//! from here: the light list and emissive tags are derived by these
//! functions on both hosts, and the mood a frame is lit under is
//! [`LightingMood::for_scene`] / [`TimeOfDay::mood`] on both hosts. The
//! shading law itself is expressed twice (WGSL and GLSL) and is mirrored on
//! the CPU by [`shade`], whose constants are paired with both shader
//! twins by tests and by `check-ui-host-drift.py`.
//!
//! # Light derivation
//!
//! What the player reads as candles, lamps and lit windows is
//! emissive-looking *geometry*: small additive-blended (ABE, ABR mode 1)
//! glow prims and bright-modulated lamp meshes whose glow is baked into the
//! art. [`vram_mesh_emitters`] / [`color_mesh_emitters`] read that
//! authoring signal back out of the mesh data, and the curated
//! [`EMISSIVE_MESHES`] table adds the surfaces whose glow the art implies
//! but no blend mode carries (the Genesis Tree). Samples are instanced into
//! world space by the host's own placement transforms and clustered
//! ([`cluster_all_scene_lights`]); the [`nearest_lights`] to the player
//! shade each frame.
//!
//! # Emissive tagging
//!
//! A prim that emits is also drawn as **emissive**: it ignores the mood's
//! darkening and draws at [`LightingMood::emissive_gain`], so a lamp stays
//! lit at night while the wall around it falls into shadow. The tag rides
//! bit 13 ([`EMISSIVE_BIT`]) of the per-vertex TSB word (textured prims)
//! and of the colour mesh's blend word (untextured prims) - bits retail's
//! words never use (a TSB uses bits 0..=8, a blend word bits 5..=6 and 15,
//! the double-sided flag bit 14) - and every shader masks it out of every
//! decode. Shaders read it only while the enhancement is on.
//!
//! # Lit windows
//!
//! A glazed town window is an opaque prim on a wall tile, so neither rule
//! above finds it. [`LIT_WINDOWS`] curates the window *art* (CLUT, page,
//! UV rectangle, texel hash); a prim sampling one carries [`WINDOW_BIT`]
//! (TSB bit 12), and the shaders turn its glass texels ([`window_glass`])
//! to lamp light by the mood's [`LightingMood::window_glow`]
//! ([`shade_window`]).

mod emissive;
mod glow;
mod mood;
mod point_lights;
mod windows;

pub use emissive::*;
pub use glow::*;
pub use mood::*;
pub use point_lights::*;
pub use windows::*;

#[cfg(test)]
mod tests;
