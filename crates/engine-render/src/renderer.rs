//! GPU-resident render resources ([`UploadedTexture`], [`UploadedMesh`],
//! [`UploadedVram`], ...) and the [`Renderer`] pipeline host. Extracted
//! from the crate root; see the crate-level docs for the pipeline overview.

use crate::shaders::*;
use crate::*;
use anyhow::{Context, Result};
use glam::Mat4;
use legaia_tim::{VRAM_HEIGHT, VRAM_WIDTH, Vram};
use std::sync::Arc;
use wgpu::util::DeviceExt;

// --- lane L ---
pub mod bloom;
mod construct;
mod depth;
mod fog_volume;
mod letterbox;
mod render;
mod state;
mod upload;
mod uploaded;

/// Re-exported for `tests::color_space`; `new_async` calls it via `construct`.
#[cfg(test)]
pub(crate) use construct::choose_surface_format;
use depth::*;
pub use depth::{FAR_BUCKET_DEPTH, pin_to_far_bucket};
pub use fog_volume::{FogGroundTexture, FogVolumeDraw};
// The fog sheets' WGSL, for the host-parity test that evaluates it.
#[cfg(test)]
pub(crate) use fog_volume::FOG_SHADER_SRC;
// The bloom passes' WGSL, for the host-parity test.
#[cfg(test)]
pub(crate) use bloom::BLOOM_SHADER_SRC;
pub(crate) use letterbox::letterbox_scale;
pub use render::CaptureImage;
pub use state::*;
pub use uploaded::*;

/// Bytes per vertex of a VRAM mesh: position, UV, CBA/TSB, normal, prim
/// colour, the two-vec4 flat bucket-depth reference
/// ([`Renderer::upload_vram_mesh_with_flat_refs`]), and the primitive's
/// corners for retail's per-primitive near reject
/// ([`legaia_render_kernels::prim_near_reject`], [`PRIM_REF_BYTES`]).
pub(crate) const VRAM_VERTEX_STRIDE: u64 = 68 + PRIM_REF_BYTES;

/// Byte stride of one glow-sprite vertex: position (3 x f32) + falloff uv
/// (2 x f32) + additive colour / kind (4 x f32).
const GLOW_VERTEX_STRIDE: u64 = 36;

/// Bytes per vertex of a colour mesh: position, colour, blend word, and the
/// primitive-corner record.
pub(crate) const COLOR_VERTEX_STRIDE: u64 = 20 + PRIM_REF_BYTES;

/// Bytes of one per-vertex primitive-corner record: `vec4` (corner 0 +
/// count) and three `vec3` corners.
pub(crate) const PRIM_REF_BYTES: u64 = 52;

/// The four vertex attributes (locations 7..=10) carrying a primitive-corner
/// record that starts at byte `base` of the vertex.
pub(crate) const fn prim_ref_attributes(base: u64) -> [wgpu::VertexAttribute; 4] {
    [
        wgpu::VertexAttribute {
            offset: base,
            shader_location: 7,
            format: wgpu::VertexFormat::Float32x4,
        },
        wgpu::VertexAttribute {
            offset: base + 16,
            shader_location: 8,
            format: wgpu::VertexFormat::Float32x3,
        },
        wgpu::VertexAttribute {
            offset: base + 28,
            shader_location: 9,
            format: wgpu::VertexFormat::Float32x3,
        },
        wgpu::VertexAttribute {
            offset: base + 40,
            shader_location: 10,
            format: wgpu::VertexFormat::Float32x3,
        },
    ]
}
