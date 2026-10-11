//! `FogVolume`: the density grid, the movers that carve it, and its per-frame simulation.

use super::*;

/// One thing that parts the bank this tick: where it stands, and the key
/// that matches it to its last position (the stride is the difference).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FogMover {
    pub key: u32,
    pub x: f32,
    pub z: f32,
}

/// The per-tick simulation state - see the module docs.
#[derive(Debug, Clone)]
pub struct FogVolume {
    /// The bank being drawn, `None` while no scene raises one (the
    /// strength then eases out on the last style).
    pub style: Option<FogStyle>,
    /// The last style a scene raised, kept so the bank can fade out on it
    /// and so a battle inherits its field scene's look.
    pub last_style: Option<FogStyle>,
    /// Eased `0..=1` presence of the bank.
    pub strength: f32,
    pub space: FogSpace,
    /// Disturbance-grid corner, in whole cells of `space.sim_cell()`.
    pub sim_origin: [i32; 2],
    /// Density multiplier per cell, row-major `[z][x]`.
    pub density: Vec<f32>,
    /// Velocity per cell, world units per tick.
    pub velocity: Vec<[f32; 2]>,
    /// Sheet-mesh corner, in whole quads of `space.mesh_cell()`.
    pub mesh_origin: [i32; 2],
    /// Floor height per sheet-mesh vertex (retail Y-down), row-major.
    pub ground: Vec<f32>,
    /// Per sheet-mesh vertex, how much bank it may carry (`0..=1`): `0` where
    /// the floor steps by more than [`FLOOR_STEP_HI`] to a neighbour, so the
    /// sheets never stand up as fins down a cliff or wall between two floor
    /// tiers - the bank lies on floors only.
    pub ground_weight: Vec<f32>,
    /// Bumped whenever [`Self::ground`] changes, so a host re-uploads only
    /// then.
    pub ground_gen: u32,
    /// Ticks stepped since the last reset - the drift clock.
    pub ticks: u32,
    /// Accumulated drift, world units (the shader's noise offset).
    pub drift: [f32; 2],
    /// The CDNAME label of the field scene the bank belongs to - a new
    /// label starts a new bank.
    pub scene: String,
    /// The field scene's measured luminance ([`scene_luminance`]) and the
    /// label it was measured for.
    pub field_luma: Option<(String, f32)>,
    /// The battle stage's measured luminance, set by the host that built the
    /// stage at battle entry.
    pub battle_luma: Option<f32>,
    /// Last tick's mover positions, by key.
    prev: Vec<FogMover>,
    /// Whether [`Self::sim_origin`] / [`Self::mesh_origin`] have been seated.
    seated: bool,
    /// The field scene's live fog-region table (MAN section 4, the retail
    /// pool spawner's own gate - [`region_weight`]), folded into
    /// [`Self::ground_weight`] so the bank lies only where retail's pool can
    /// spawn. Empty in battle and in scenes with no table.
    regions: Vec<FogRegion>,
    /// [`Self::regions`] changed since the sheet mesh was last sampled.
    regions_dirty: bool,
    /// The current field scene's interior walk areas ([`InteriorTracker`]).
    pub interiors: InteriorTracker,
}

/// How much bank the field tile `(tile_x, tile_z)` may carry under the
/// scene's fog-region table: retail's spawner rule (`FUN_801D629C`,
/// `0x801D6320..0x801D63B8`, `legaia_engine_core::fog_particles::FogPool::spawn`) - the
/// **first** region whose open box holds the tile decides, and a disabled
/// hit ends the search, so an earlier disabled box carves a hole out of a
/// later enabled one. No containing region means no fog. An empty table
/// (a tuned scene with no section 4) leaves the whole scene to the bank.
///
/// This is what keeps the bank out of the interiors a town lays out beside
/// its streets: `town0b`'s one region covers the Rim Elm streets and none of
/// the house rooms, which sit in the same scene at their own tiles (a door
/// is an intra-scene warp), and the boxes several scenes key on flag `0x007`
/// sit ahead of their area-wide region and switch off once that flag is set.
pub fn region_weight(regions: &[FogRegion], tile_x: i32, tile_z: i32) -> f32 {
    if regions.is_empty() {
        return 1.0;
    }
    match regions.iter().find(|r| r.contains(tile_x, tile_z)) {
        Some(r) if r.enabled => 1.0,
        _ => 0.0,
    }
}

impl Default for FogVolume {
    fn default() -> Self {
        Self::new()
    }
}

impl FogVolume {
    pub fn new() -> Self {
        Self {
            style: None,
            last_style: None,
            strength: 0.0,
            space: FogSpace::Field,
            sim_origin: [0; 2],
            density: vec![1.0; SIM_DIM * SIM_DIM],
            velocity: vec![[0.0; 2]; SIM_DIM * SIM_DIM],
            mesh_origin: [0; 2],
            ground: vec![0.0; (MESH_DIM + 1) * (MESH_DIM + 1)],
            ground_weight: vec![1.0; (MESH_DIM + 1) * (MESH_DIM + 1)],
            ground_gen: 0,
            ticks: 0,
            drift: [0.0; 2],
            scene: String::new(),
            field_luma: None,
            battle_luma: None,
            prev: Vec::new(),
            seated: false,
            regions: Vec::new(),
            regions_dirty: false,
            interiors: InteriorTracker::default(),
        }
    }

    /// Drop every disturbance and the clock: a fresh, undisturbed bank.
    /// The style and strength survive (a scene change re-evaluates them).
    pub fn clear_field(&mut self) {
        self.density.iter_mut().for_each(|d| *d = 1.0);
        self.velocity.iter_mut().for_each(|v| *v = [0.0; 2]);
        self.prev.clear();
        self.seated = false;
    }

    /// Full reset - what the toggle going off leaves behind.
    pub fn reset(&mut self) {
        *self = Self {
            ground_gen: self.ground_gen.wrapping_add(1),
            ..Self::new()
        };
    }

    /// Install the field scene's live fog-region table ([`region_weight`]).
    /// A change re-samples the sheet mesh on the next step, so a script that
    /// rewrites the region enables (op `4C C1`) moves the bank with it.
    pub fn set_regions(&mut self, regions: &[FogRegion]) {
        if self.regions != regions {
            self.regions.clear();
            self.regions.extend_from_slice(regions);
            self.regions_dirty = true;
        }
    }

    /// Switch coordinate space (field <-> battle). The disturbances of one
    /// space mean nothing in the other, so the grid restarts.
    pub fn set_space(&mut self, space: FogSpace) {
        if self.space != space {
            self.space = space;
            self.clear_field();
        }
    }

    /// Ease the strength toward the style this tick asks for. A `Some` style
    /// becomes [`Self::last_style`] too.
    pub fn set_style(&mut self, style: Option<FogStyle>) {
        if style.is_some() {
            self.last_style = style;
        }
        self.style = style;
        let target = if style.is_some() { 1.0 } else { 0.0 };
        if self.strength < target {
            self.strength = (self.strength + STRENGTH_STEP).min(target);
        } else if self.strength > target {
            self.strength = (self.strength - STRENGTH_STEP).max(target);
        }
    }

    /// Whether anything would draw: some style seen and a non-zero strength.
    pub fn visible(&self) -> bool {
        self.strength > 0.0 && self.last_style.is_some()
    }

    fn idx(x: usize, z: usize) -> usize {
        z * SIM_DIM + x
    }

    /// Recentre both grids on `focus` (world X / Z). The disturbance grid
    /// shifts its contents so a wake stays where it was carved; the cells
    /// that scroll in are undisturbed. The sheet mesh re-samples `floor`
    /// (world X / Z -> Y-down height) when it moves. Returns whether the
    /// mesh moved.
    pub fn recentre(&mut self, focus: [f32; 2], floor: impl Fn(f32, f32) -> f32) -> bool {
        let sc = self.space.sim_cell();
        let half = (SIM_DIM / 2) as i32;
        let want = [
            (focus[0] / sc).floor() as i32 - half,
            (focus[1] / sc).floor() as i32 - half,
        ];
        if !self.seated {
            self.sim_origin = want;
        } else if want != self.sim_origin {
            let dx = want[0] - self.sim_origin[0];
            let dz = want[1] - self.sim_origin[1];
            let n = SIM_DIM as i32;
            let mut d = vec![1.0f32; SIM_DIM * SIM_DIM];
            let mut v = vec![[0.0f32; 2]; SIM_DIM * SIM_DIM];
            if dx.abs() < n && dz.abs() < n {
                for z in 0..n {
                    let sz = z + dz;
                    if !(0..n).contains(&sz) {
                        continue;
                    }
                    for x in 0..n {
                        let sx = x + dx;
                        if !(0..n).contains(&sx) {
                            continue;
                        }
                        let src = Self::idx(sx as usize, sz as usize);
                        let dst = Self::idx(x as usize, z as usize);
                        d[dst] = self.density[src];
                        v[dst] = self.velocity[src];
                    }
                }
            }
            self.density = d;
            self.velocity = v;
            self.sim_origin = want;
        }

        let mc = self.space.mesh_cell();
        let mhalf = (MESH_DIM / 2) as i32;
        let mwant = [
            (focus[0] / mc).floor() as i32 - mhalf,
            (focus[1] / mc).floor() as i32 - mhalf,
        ];
        let moved = !self.seated || mwant != self.mesh_origin || self.regions_dirty;
        if moved {
            self.mesh_origin = mwant;
            self.resample_ground(floor);
        }
        self.seated = true;
        moved
    }

    /// Re-sample every sheet-mesh vertex's floor height.
    pub fn resample_ground(&mut self, floor: impl Fn(f32, f32) -> f32) {
        let mc = self.space.mesh_cell();
        let n = MESH_DIM + 1;
        for z in 0..n {
            for x in 0..n {
                let wx = (self.mesh_origin[0] + x as i32) as f32 * mc;
                let wz = (self.mesh_origin[1] + z as i32) as f32 * mc;
                self.ground[z * n + x] = floor(wx, wz);
            }
        }
        for z in 0..n {
            for x in 0..n {
                let h = self.ground[z * n + x];
                let mut step = 0.0f32;
                for dz in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let (nx, nz) = (x as i32 + dx, z as i32 + dz);
                        if nx < 0 || nz < 0 || nx >= n as i32 || nz >= n as i32 {
                            continue;
                        }
                        step = step.max((self.ground[nz as usize * n + nx as usize] - h).abs());
                    }
                }
                let t = ((step - FLOOR_STEP_LO) / (FLOOR_STEP_HI - FLOOR_STEP_LO)).clamp(0.0, 1.0);
                let region = match self.space {
                    // One sheet-mesh quad is one 128-unit tile, so vertex
                    // `(x, z)` sits on the corner of tile `mesh_origin + (x,
                    // z)` - the point retail seats a particle spawned there.
                    FogSpace::Field => region_weight(
                        &self.regions,
                        self.mesh_origin[0] + x as i32,
                        self.mesh_origin[1] + z as i32,
                    ),
                    FogSpace::Battle => 1.0,
                };
                self.ground_weight[z * n + x] = (1.0 - t * t * (3.0 - 2.0 * t)) * region;
            }
        }
        self.regions_dirty = false;
        self.ground_gen = self.ground_gen.wrapping_add(1);
    }

    /// The density at world `(x, z)` (bilinear over cell centres; `1.0`
    /// outside the grid) - the value the hosts' density texture samples.
    pub fn density_at(&self, x: f32, z: f32) -> f32 {
        let sc = self.space.sim_cell();
        let gx = x / sc - self.sim_origin[0] as f32 - 0.5;
        let gz = z / sc - self.sim_origin[1] as f32 - 0.5;
        self.sample_grid(&self.density, gx, gz)
    }

    fn sample_grid(&self, field: &[f32], gx: f32, gz: f32) -> f32 {
        let x0 = gx.floor();
        let z0 = gz.floor();
        let fx = gx - x0;
        let fz = gz - z0;
        let at = |x: i32, z: i32| -> f32 {
            if x < 0 || z < 0 || x >= SIM_DIM as i32 || z >= SIM_DIM as i32 {
                1.0
            } else {
                field[Self::idx(x as usize, z as usize)]
            }
        };
        let (x0, z0) = (x0 as i32, z0 as i32);
        let a = at(x0, z0) + (at(x0 + 1, z0) - at(x0, z0)) * fx;
        let b = at(x0, z0 + 1) + (at(x0 + 1, z0 + 1) - at(x0, z0 + 1)) * fx;
        a + (b - a) * fz
    }

    /// One sim tick: recentre on `focus`, let every mover part the bank,
    /// advect + diffuse + refill. `floor` samples the walk ground (retail
    /// Y-down) for the sheet mesh. Does nothing while the bank is invisible
    /// and has nothing left to fade.
    pub fn step(&mut self, focus: [f32; 2], movers: &[FogMover], floor: impl Fn(f32, f32) -> f32) {
        let Some(style) = self.last_style else {
            return;
        };
        if self.strength <= 0.0 {
            // Fully faded: keep nothing that would replay a stale wake on the
            // next fade-in.
            if self.seated {
                self.clear_field();
            }
            return;
        }
        self.recentre(focus, floor);
        self.ticks = self.ticks.wrapping_add(1);
        // The drift grows without bound; at the styles' sub-unit winds it
        // takes hours to reach a magnitude where the shaders' noise input
        // (drift times the fine scale) loses sub-lattice precision.
        self.drift[0] += style.wind[0];
        self.drift[1] += style.wind[1];
        self.part(movers);
        self.advect(style.wind);
    }

    fn part(&mut self, movers: &[FogMover]) {
        let sc = self.space.sim_cell();
        let r = self.space.mover_radius();
        let reach = (r / sc).ceil() as i32 + 1;
        for m in movers {
            let prev = self.prev.iter().find(|p| p.key == m.key).copied();
            let (vx, vz) = match prev {
                Some(p) => (m.x - p.x, m.z - p.z),
                None => (0.0, 0.0),
            };
            let speed = (vx * vx + vz * vz).sqrt();
            let (vx, vz, speed) = if speed > TELEPORT_UNITS {
                (0.0, 0.0, 0.0)
            } else {
                (vx, vz, speed)
            };
            let stride = (speed / STRIDE_FULL).min(1.0);
            let carve = CARVE_IDLE + CARVE_MOVE * stride;
            let gx = m.x / sc - self.sim_origin[0] as f32;
            let gz = m.z / sc - self.sim_origin[1] as f32;
            let cx = gx.floor() as i32;
            let cz = gz.floor() as i32;
            for z in (cz - reach)..=(cz + reach) {
                if z < 0 || z >= SIM_DIM as i32 {
                    continue;
                }
                for x in (cx - reach)..=(cx + reach) {
                    if x < 0 || x >= SIM_DIM as i32 {
                        continue;
                    }
                    let dx = (x as f32 + 0.5 - gx) * sc;
                    let dz = (z as f32 + 0.5 - gz) * sc;
                    let dist2 = dx * dx + dz * dz;
                    if dist2 >= r * r {
                        continue;
                    }
                    let q = 1.0 - dist2 / (r * r);
                    let falloff = q * q;
                    let i = Self::idx(x as usize, z as usize);
                    // A stride clears toward zero; standing still only
                    // opens a shallow pocket ([`IDLE_POCKET`]), so a
                    // character who waits keeps the bank about its legs.
                    let floor = (1.0 - stride) * (1.0 - IDLE_POCKET * falloff);
                    let d = &mut self.density[i];
                    if *d > floor {
                        *d -= (*d - floor) * carve * falloff;
                    }
                    if speed > 0.0 {
                        let dist = dist2.sqrt();
                        let (rx, rz) = if dist > 1.0e-3 {
                            (dx / dist, dz / dist)
                        } else {
                            (0.0, 0.0)
                        };
                        let v = &mut self.velocity[i];
                        v[0] += falloff * (vx * PUSH_ALONG + rx * speed * PUSH_RADIAL);
                        v[1] += falloff * (vz * PUSH_ALONG + rz * speed * PUSH_RADIAL);
                        let mag2 = v[0] * v[0] + v[1] * v[1];
                        if mag2 > VELOCITY_MAX * VELOCITY_MAX {
                            let s = VELOCITY_MAX / mag2.sqrt();
                            v[0] *= s;
                            v[1] *= s;
                        }
                    }
                }
            }
        }
        self.prev.clear();
        self.prev.extend_from_slice(movers);
    }

    fn advect(&mut self, wind: [f32; 2]) {
        let sc = self.space.sim_cell();
        let n = SIM_DIM as i32;
        let old_d = self.density.clone();
        let old_v = self.velocity.clone();
        let at_d = |x: i32, z: i32| -> f32 {
            if x < 0 || z < 0 || x >= n || z >= n {
                1.0
            } else {
                old_d[Self::idx(x as usize, z as usize)]
            }
        };
        let at_v = |x: i32, z: i32| -> [f32; 2] {
            if x < 0 || z < 0 || x >= n || z >= n {
                [0.0; 2]
            } else {
                old_v[Self::idx(x as usize, z as usize)]
            }
        };
        for z in 0..n {
            for x in 0..n {
                let i = Self::idx(x as usize, z as usize);
                let v = old_v[i];
                // Semi-Lagrangian back-trace through the stirred velocity
                // plus the ambient drift, in cells.
                let bx = x as f32 - (v[0] + wind[0]) / sc;
                let bz = z as f32 - (v[1] + wind[1]) / sc;
                let adv = self.sample_grid(&old_d, bx, bz);
                let nb = (at_d(x - 1, z) + at_d(x + 1, z) + at_d(x, z - 1) + at_d(x, z + 1)) * 0.25;
                let mut d = adv + (nb - adv) * DENSITY_DIFFUSE;
                d += (1.0 - d) * REFILL;
                self.density[i] = d.clamp(0.0, 1.0);

                let (l, r, u, w) = (
                    at_v(x - 1, z),
                    at_v(x + 1, z),
                    at_v(x, z - 1),
                    at_v(x, z + 1),
                );
                let mx = (l[0] + r[0] + u[0] + w[0]) * 0.25;
                let mz = (l[1] + r[1] + u[1] + w[1]) * 0.25;
                self.velocity[i] = [
                    (v[0] + (mx - v[0]) * VELOCITY_DIFFUSE) * VELOCITY_DAMP,
                    (v[1] + (mz - v[1]) * VELOCITY_DIFFUSE) * VELOCITY_DAMP,
                ];
            }
        }
    }

    /// The render description of this tick's bank, scaled by `tint` (the
    /// scripted screen tint, so a fade to black takes the fog with it).
    /// `None` when nothing would draw.
    ///
    /// `light` is the brightness the bank must not outshine - the scene's
    /// measured luminance ([`scene_luminance`], already scaled by any live
    /// ambient): the style's colour keeps its hue and is dimmed to
    /// [`luma_scale`]'s ceiling, so a night scene gets a moonlit haze rather
    /// than glowing snow. A style already darker than that is
    /// left alone; `None` (nothing measured) keeps the authored colour.
    pub fn frame(&self, tint: Option<[f32; 3]>, light: Option<f32>) -> Option<FogVolumeFrame<'_>> {
        if !self.visible() || !self.seated {
            return None;
        }
        let style = self.last_style?;
        let k = light.map_or(1.0, |l| luma_scale(style.color, l));
        let t = tint.unwrap_or([1.0; 3]).map(|c| c * k);
        let sc = self.space.sim_cell();
        let mc = self.space.mesh_cell();
        Some(FogVolumeFrame {
            space: self.space,
            sim_origin: [
                self.sim_origin[0] as f32 * sc,
                self.sim_origin[1] as f32 * sc,
            ],
            sim_cell: sc,
            density: self
                .density
                .iter()
                .map(|d| (d.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
                .collect(),
            mesh_origin: [
                self.mesh_origin[0] as f32 * mc,
                self.mesh_origin[1] as f32 * mc,
            ],
            mesh_cell: mc,
            ground: &self.ground,
            ground_weight: &self.ground_weight,
            ground_gen: self.ground_gen,
            color: [
                style.color[0] * t[0],
                style.color[1] * t[1],
                style.color[2] * t[2],
            ],
            opacity: (style.density * self.space.density_scale()).min(1.0) * self.strength,
            height: style.height * self.space.height_scale(),
            drift: self.drift,
            ticks: self.ticks,
        })
    }
}
