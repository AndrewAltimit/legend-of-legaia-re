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
    /// `(MESH_DIM + 1)²` per-vertex floor weights ([`FogVolume::ground_weight`]:
    /// negative on the distant haze layer).
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
    /// The scene's ground coverage field ([`FogGround::texels`]), `None` in
    /// battle and in a field scene the host built none for - the shaders
    /// then read full coverage everywhere, the bank as it was before.
    pub ground_tex: Option<FogGroundFrame<'a>>,
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
    /// `super::FogSpace::shader_constants` start here.
    pub const SHADER_CONSTANTS: usize = 18;
    /// `super::FogSpace::soft_distance`.
    pub const SOFT_DISTANCE: usize = 22;
    /// The spill streaks' flow phase (`super::FogVolumeFrame::spill_phase`):
    /// noise cells travelled since the clock last wrapped.
    pub const SPILL_PHASE: usize = 23;
    /// The ground field texture's world rect (`super::FogGroundFrame`):
    /// corner X / Z and span X / Z. A zero span = no field.
    pub const GROUND_TEX_ORIGIN_X: usize = 24;
    pub const GROUND_TEX_ORIGIN_Z: usize = 25;
    pub const GROUND_TEX_EXTENT_X: usize = 26;
    pub const GROUND_TEX_EXTENT_Z: usize = 27;
    /// Texels per side, and the generation to re-upload them on.
    pub const GROUND_TEX_W: usize = 28;
    pub const GROUND_TEX_H: usize = 29;
    pub const GROUND_TEX_GEN: usize = 30;
    /// `super::FOG_SPILL_CONSTANTS` start here (four values).
    pub const SPILL_CONSTANTS: usize = 31;
    /// `super::FOG_HAZE_CONSTANTS` start here (four values).
    pub const HAZE_CONSTANTS: usize = 35;
    pub const LEN: usize = 39;
}

/// The texture a host binds when the frame has no ground field: one texel
/// of full coverage and no spill ([`FogGround::texels`] channel layout).
pub const FOG_GROUND_NONE_TEXEL: [u8; 4] = [255, 0, 0, 255];

/// Ticks for a spill tendril to travel one noise cell down the fall: an
/// unhurried pour - with [`FOG_SPILL_CONSTANTS`]' fall scale, one cell is
/// 520 units of drop, so about five seconds at sixty ticks a second.
pub const SPILL_FLOW_TICKS: u32 = 300;

/// The tick clock's wrap for [`header::SPILL_PHASE`]: a multiple of
/// the spill streaks' period ([`SPILL_FLOW_TICKS`]), so the wrap is seamless.
pub const SPILL_TICK_WRAP: u32 = SPILL_FLOW_TICKS * 1000;

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
        h[header::SPILL_PHASE] = self.spill_phase();
        h[header::SPILL_CONSTANTS..header::SPILL_CONSTANTS + 4]
            .copy_from_slice(&FOG_SPILL_CONSTANTS);
        h[header::HAZE_CONSTANTS..header::HAZE_CONSTANTS + 4].copy_from_slice(&FOG_HAZE_CONSTANTS);
        if let Some(g) = &self.ground_tex {
            h[header::GROUND_TEX_ORIGIN_X] = g.origin[0];
            h[header::GROUND_TEX_ORIGIN_Z] = g.origin[1];
            h[header::GROUND_TEX_EXTENT_X] = g.extent[0];
            h[header::GROUND_TEX_EXTENT_Z] = g.extent[1];
            h[header::GROUND_TEX_W] = g.w as f32;
            h[header::GROUND_TEX_H] = g.h as f32;
            h[header::GROUND_TEX_GEN] = g.ground_gen as f32;
        }
        h
    }

    /// The spill tendrils' flow phase: noise cells a tendril has travelled
    /// down the fall, one per [`SPILL_FLOW_TICKS`]. Taken off the tick
    /// clock wrapped at [`SPILL_TICK_WRAP`], so it stays exact in an `f32`
    /// uniform and the wrap lands on a whole cell (seamless).
    pub fn spill_phase(&self) -> f32 {
        (self.ticks % SPILL_TICK_WRAP) as f32 / SPILL_FLOW_TICKS as f32
    }

    /// The ground field texture's world rect as the shaders take it:
    /// `[origin_x, origin_z, 1 / extent_x, 1 / extent_z]`, all zero without
    /// a field (every fragment then samples the one full-coverage texel of
    /// [`FOG_GROUND_NONE_TEXEL`]).
    pub fn ground_uv_rect(&self) -> [f32; 4] {
        match &self.ground_tex {
            Some(g) if g.extent[0] > 0.0 && g.extent[1] > 0.0 => [
                g.origin[0],
                g.origin[1],
                1.0 / g.extent[0],
                1.0 / g.extent[1],
            ],
            _ => [0.0; 4],
        }
    }

    /// The sheet mesh's vertices, `[x, floor_y, z, floor_weight]` per vertex
    /// in row-major order (retail Y-down; a negative weight marks the
    /// distant haze layer) - what both hosts upload when
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
