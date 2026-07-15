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
mod helpers;
mod render;
mod state;
mod upload;
mod uploaded;

/// Re-exported for `tests::color_space`; `new_async` calls it via `core`.
#[cfg(test)]
pub(crate) use core::choose_surface_format;
pub(crate) use helpers::*;
pub use render::CaptureImage;
pub use state::*;
pub use uploaded::*;
