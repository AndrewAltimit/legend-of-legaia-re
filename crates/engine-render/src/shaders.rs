//! WGSL shader sources for the renderer's pipelines, plus the PSX
//! 24->15-bit ordered-dither compose helper. Extracted from the crate root.

mod bare_mesh;
mod blit;
mod color_mesh;
mod glow;
mod lines;
mod prim_reject;
mod psx_prelude;
mod scene_lights;
mod screen_overlay;
mod shadow_mesh;
mod text;
mod vram_mesh;

pub(crate) use bare_mesh::*;
pub(crate) use blit::*;
pub(crate) use color_mesh::*;
pub(crate) use glow::*;
pub(crate) use lines::*;
pub(crate) use prim_reject::*;
pub(crate) use psx_prelude::*;
pub(crate) use scene_lights::*;
pub(crate) use screen_overlay::*;
pub(crate) use shadow_mesh::*;
pub(crate) use text::*;
pub(crate) use vram_mesh::*;

/// The real scene-lights WGSL, exposed for the lockstep tests in
/// [`crate::scene_lights`].
#[cfg(test)]
pub(crate) fn scene_lights_wgsl_for_tests() -> &'static str {
    SCENE_LIGHTS_REAL_WGSL
}

/// Prepend [`PSX_DITHER_WGSL`] (and the stub `scene_point_gain`) to a
/// shaded 3D shader source so its fragment stage can call `psx_dither` /
/// `dyn_light`. WGSL module-scope declarations are order-independent, so
/// the leading helpers are valid ahead of the shader's own structs and
/// entry points. Single-mesh pipelines compile this variant - their
/// layouts carry no lights group, so the point-light layer is the zero
/// stub.
pub(crate) fn compose_psx_shader(base: &str) -> String {
    format!(
        "{PSX_DITHER_WGSL}\n{OVERWORLD_CURVE_WGSL}\n{PRIM_NEAR_WGSL}\n{EFFECT_CLIP_WGSL}\n{SCENE_LIGHTS_STUB_WGSL}\n{base}"
    )
}

/// The scene-pipeline twin of [`compose_psx_shader`]: same prelude but
/// with the REAL per-scene point-light layer ([`SCENE_LIGHTS_REAL_WGSL`] -
/// lights uniform + shadow-map array at group 2) in place of the stub.
pub(crate) fn compose_scene_lit_shader(base: &str) -> String {
    format!(
        "{PSX_DITHER_WGSL}\n{OVERWORLD_CURVE_WGSL}\n{PRIM_NEAR_WGSL}\n{EFFECT_CLIP_WGSL}\n{SCENE_LIGHTS_REAL_WGSL}\n{base}"
    )
}
