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

mod core;
mod fog_volume;
mod helpers;
mod render;
mod state;
mod upload;
mod uploaded;

/// Re-exported for `tests::color_space`; `new_async` calls it via `core`.
#[cfg(test)]
pub(crate) use core::choose_surface_format;
pub use fog_volume::FogVolumeDraw;
pub(crate) use helpers::*;
pub use render::CaptureImage;
pub use state::*;
pub use uploaded::*;

/// Bytes per vertex of a VRAM mesh: position, UV, CBA/TSB, normal, prim
/// colour, and the two-vec4 flat bucket-depth reference
/// ([`Renderer::upload_vram_mesh_with_flat_refs`]).
pub(crate) const VRAM_VERTEX_STRIDE: u64 = 68;
