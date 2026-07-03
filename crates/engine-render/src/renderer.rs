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

pub(crate) use helpers::*;
pub use state::*;
pub use uploaded::*;
