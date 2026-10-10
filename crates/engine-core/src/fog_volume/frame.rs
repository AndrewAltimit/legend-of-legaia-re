//! `FogVolumeFrame`: the per-frame upload a renderer consumes, its header layout and mesh indices.

use super::*;

/// What a host draws for one frame: see [`FogVolume::frame`]. Positions are
/// world units in the bank's [`FogSpace`] (retail Y-down for heights); the
/// host composes its own world -> clip matrix for that space.
#[derive(Debug, Clone)]
pub struct FogVolumeFrame<'a> {
    pub space: FogSpace,
    /// World X / Z of the disturbance grid's corner.
    pub sim_origin: [f32; 2],
    pub sim_cell: f32,
    /// [`SIM_DIM`]² density bytes, row-major `[z][x]`, `255` = undisturbed.
    pub density: Vec<u8>,
    /// World X / Z of the sheet mesh's corner.
    pub mesh_origin: [f32; 2],
    pub mesh_cell: f32,
    /// `(MESH_DIM + 1)²` floor heights, retail Y-down, row-major.
    pub ground: &'a [f32],
    /// `(MESH_DIM + 1)²` per-vertex floor weights ([`FogVolume::ground_weight`]).
    pub ground_weight: &'a [f32],
    pub ground_gen: u32,
    /// Framebuffer-space colour, tint applied.
    pub color: [f32; 3],
    /// Summed floor opacity of the undisturbed bank, strength applied.
    pub opacity: f32,
    pub height: f32,
    /// Accumulated drift (world units) - the noise offset.
    pub drift: [f32; 2],
    pub ticks: u32,
}

/// Index layout of [`FogVolumeFrame::header`] - the browser page reads the
/// frame through this one packed array.
pub mod header {
    pub const SIM_ORIGIN_X: usize = 0;
    pub const SIM_ORIGIN_Z: usize = 1;
    pub const SIM_CELL: usize = 2;
    pub const SIM_DIM: usize = 3;
    pub const MESH_ORIGIN_X: usize = 4;
    pub const MESH_ORIGIN_Z: usize = 5;
    pub const MESH_CELL: usize = 6;
    pub const MESH_DIM: usize = 7;
    pub const COLOR_R: usize = 8;
    pub const COLOR_G: usize = 9;
    pub const COLOR_B: usize = 10;
    pub const OPACITY: usize = 11;
    pub const HEIGHT: usize = 12;
    pub const LAYERS: usize = 13;
    pub const DRIFT_X: usize = 14;
    pub const DRIFT_Z: usize = 15;
    pub const GROUND_GEN: usize = 16;
    /// `0` field, `1` battle.
    pub const SPACE: usize = 17;
    /// [`super::super::FogSpace::shader_constants`] start here.
    pub const SHADER_CONSTANTS: usize = 18;
    /// [`super::super::FogSpace::soft_distance`].
    pub const SOFT_DISTANCE: usize = 22;
    pub const LEN: usize = 23;
}

/// Horizontal sheets the bank is drawn as.
pub const FOG_LAYERS: u32 = 12;

impl FogVolumeFrame<'_> {
    /// The frame's scalars packed in [`header`] order.
    pub fn header(&self) -> Vec<f32> {
        let mut h = vec![0.0f32; header::LEN];
        h[header::SIM_ORIGIN_X] = self.sim_origin[0];
        h[header::SIM_ORIGIN_Z] = self.sim_origin[1];
        h[header::SIM_CELL] = self.sim_cell;
        h[header::SIM_DIM] = SIM_DIM as f32;
        h[header::MESH_ORIGIN_X] = self.mesh_origin[0];
        h[header::MESH_ORIGIN_Z] = self.mesh_origin[1];
        h[header::MESH_CELL] = self.mesh_cell;
        h[header::MESH_DIM] = MESH_DIM as f32;
        h[header::COLOR_R] = self.color[0];
        h[header::COLOR_G] = self.color[1];
        h[header::COLOR_B] = self.color[2];
        h[header::OPACITY] = self.opacity;
        h[header::HEIGHT] = self.height;
        h[header::LAYERS] = FOG_LAYERS as f32;
        h[header::DRIFT_X] = self.drift[0];
        h[header::DRIFT_Z] = self.drift[1];
        h[header::GROUND_GEN] = self.ground_gen as f32;
        h[header::SPACE] = match self.space {
            FogSpace::Field => 0.0,
            FogSpace::Battle => 1.0,
        };
        h[header::SHADER_CONSTANTS..header::SHADER_CONSTANTS + 4]
            .copy_from_slice(&self.space.shader_constants());
        h[header::SOFT_DISTANCE] = self.space.soft_distance();
        h
    }

    /// The sheet mesh's vertices, `[x, floor_y, z, floor_weight]` per vertex
    /// in row-major order (retail Y-down) - what both hosts upload when
    /// [`Self::ground_gen`] changes.
    pub fn mesh_positions(&self) -> Vec<f32> {
        let n = MESH_DIM + 1;
        let mut out = Vec::with_capacity(n * n * 4);
        for z in 0..n {
            for x in 0..n {
                out.push(self.mesh_origin[0] + x as f32 * self.mesh_cell);
                out.push(self.ground[z * n + x]);
                out.push(self.mesh_origin[1] + z as f32 * self.mesh_cell);
                out.push(self.ground_weight[z * n + x]);
            }
        }
        out
    }
}

/// The sheet mesh's triangle list (two per quad), shared by every frame.
pub fn mesh_indices() -> Vec<u32> {
    let n = (MESH_DIM + 1) as u32;
    let mut out = Vec::with_capacity(MESH_DIM * MESH_DIM * 6);
    for z in 0..MESH_DIM as u32 {
        for x in 0..MESH_DIM as u32 {
            let a = z * n + x;
            let b = a + 1;
            let c = a + n;
            let d = c + 1;
            out.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    out
}
