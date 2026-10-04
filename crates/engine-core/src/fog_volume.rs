//! Volumetric ground fog - an **enhancement**, not a retail system.
//!
//! Retail's only fog is the `fog_set` puff pool ([`crate::fog_particles`]):
//! two additive billboards per particle, drifting on fixed per-region
//! headings, blind to every actor. This module layers a low, drifting mist
//! bank over the scenes that already read as misty, and lets the characters
//! walking through it part it: each mover carves the density around its
//! feet and pushes the bank along its stride, and the wake it leaves refills
//! slowly. It never touches the retail pool, and nothing in the simulation
//! reads it back - the toggle off is the untouched frame.
//!
//! # Two grids
//!
//! - The **disturbance grid** ([`SIM_DIM`] x [`SIM_DIM`] cells of
//!   [`FogSpace::sim_cell`] world units, recentred on the focus in whole-cell
//!   steps): a density multiplier per cell (`1.0` = undisturbed bank, `0.0` =
//!   cleared) plus a 2D velocity. Everything outside it reads as an
//!   undisturbed bank, so a recentre shifts the field rather than restarting
//!   it.
//! - The **sheet mesh** ([`MESH_DIM`] quads per side of
//!   [`FogSpace::mesh_cell`] units, recentred the same way): the ground
//!   heights the hosts draw the fog's horizontal sheets over, so the bank
//!   hugs the walk ground's floor tiers instead of floating at one height.
//!
//! # Determinism
//!
//! One [`FogVolume::step`] per sim tick, nothing per rendered frame: the
//! bank's motion is tied to the tick count (the hosts draw the noise drift
//! off [`FogVolumeFrame::drift`], accumulated per tick too), so two hosts at
//! different frame rates show the same bank for the same input. The step
//! uses only `+ - * /`, `sqrt` and comparisons on `f32` - all IEEE-exact -
//! so native and `wasm32` agree bit for bit.
//!
//! # Hosts
//!
//! `World` owns one ([`crate::world::World::fog_volume`]) and steps it at
//! the end of every [`crate::world::World::tick`] while
//! [`crate::world::WorldToggles::volumetric_fog`] is raised (the one
//! engine-side setting both play hosts flip, from
//! [`crate::options::OptionsState::volumetric_fog`]). Each host draws
//! [`crate::world::World::fog_volume_frame`] after its 3D scene and before
//! its HUD: the native renderer's `fog_volume` pass and the browser play
//! page's `webgl-fog-volume.js`, two transcriptions of one shading recipe
//! (see [`FOG_SHADER_CONSTANTS`]).

/// Disturbance-grid cells per side.
pub const SIM_DIM: usize = 64;
/// Sheet-mesh quads per side (the mesh has `MESH_DIM + 1` vertices a side).
pub const MESH_DIM: usize = 48;

/// Movement past this many units in one tick is a seat / warp, not a stride:
/// the mover's history restarts and it injects no velocity.
const TELEPORT_UNITS: f32 = 96.0;
/// Fraction of the remaining gap to an undisturbed bank refilled per tick:
/// a cleared cell is back to ~90% about five seconds after it was carved.
const REFILL: f32 = 0.007;
/// Share of each cell's density / velocity mixed with its four neighbours'
/// mean per tick - what softens a footprint into a wake.
const DENSITY_DIFFUSE: f32 = 0.10;
const VELOCITY_DIFFUSE: f32 = 0.30;
/// Per-tick velocity damping.
const VELOCITY_DAMP: f32 = 0.93;
/// Velocity cap, units per tick.
const VELOCITY_MAX: f32 = 10.0;
/// Carve rate for a mover standing still (it slowly opens a pocket) and the
/// extra a full stride adds.
const CARVE_IDLE: f32 = 0.02;
const CARVE_MOVE: f32 = 0.5;
/// How deep a standing mover's pocket gets at its centre (`0..=1`).
const IDLE_POCKET: f32 = 0.35;
/// A stride's speed (units per tick) at which the carve saturates.
const STRIDE_FULL: f32 = 5.0;
/// How much of a mover's own velocity it hands the cells under it, and how
/// hard it shoves them outward per unit of speed.
const PUSH_ALONG: f32 = 0.30;
const PUSH_RADIAL: f32 = 0.30;
/// Strength easing per tick (the bank fades in over ~1.5 s when a scene or
/// its gate raises it, and out the same way).
const STRENGTH_STEP: f32 = 1.0 / 90.0;

/// The constants the two shader transcriptions share, packed for the
/// browser page (indices in the doc). The native WGSL and the page's GLSL
/// both read these from the frame, so the recipe has one set of numbers:
///
/// ```text
/// [0] noise scale, fine (1 / world units)
/// [1] noise scale, banks (1 / world units)
/// [2] per-sheet alpha gain (the summed opacity of all sheets at the floor)
/// [3] vertical profile exponent
/// ```
pub const FOG_SHADER_CONSTANTS: [f32; 4] = [1.0 / 360.0, 1.0 / 1300.0, 5.0, 1.2];

/// The coordinate space a bank lives in: the field's world units, or the
/// battle stage's raw units (the frame both battle hosts draw the stage
/// from before their `BATTLE_WORLD_SCALE` model factor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FogSpace {
    Field,
    Battle,
}

impl FogSpace {
    /// World units per disturbance cell.
    pub fn sim_cell(self) -> f32 {
        match self {
            FogSpace::Field => 40.0,
            FogSpace::Battle => 64.0,
        }
    }
    /// World units per sheet-mesh quad.
    pub fn mesh_cell(self) -> f32 {
        match self {
            FogSpace::Field => 128.0,
            FogSpace::Battle => 160.0,
        }
    }
    /// The radius a mover parts the bank over, in world units.
    pub fn mover_radius(self) -> f32 {
        match self {
            FogSpace::Field => 64.0,
            FogSpace::Battle => 180.0,
        }
    }
    /// Bank depth relative to a style's (field-unit) height: the battle
    /// forms stand several times taller in stage units than the field forms
    /// do in world units, so the same knee-to-waist bank is deeper there.
    /// [`FOG_SHADER_CONSTANTS`] for this space: the noise scales follow the
    /// bank's size ([`Self::height_scale`]), so a battle arena shows as many
    /// wisps across it as a field screen does.
    pub fn shader_constants(self) -> [f32; 4] {
        let k = self.height_scale();
        let c = FOG_SHADER_CONSTANTS;
        [c[0] / k, c[1] / k, c[2], c[3]]
    }

    pub fn height_scale(self) -> f32 {
        match self {
            FogSpace::Field => 1.0,
            FogSpace::Battle => 2.5,
        }
    }
}

/// One scene's look: what colour the bank is, how thick, how deep, and
/// which way it drifts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FogStyle {
    /// Framebuffer-space colour, `0..=1` per channel (the hosts write PSX
    /// framebuffer bytes, so this is not linear light).
    pub color: [f32; 3],
    /// Summed opacity at the floor for an undisturbed bank, `0..=1`.
    pub density: f32,
    /// Bank depth above the floor, world units (retail Y-down frame: the
    /// top sheet sits at `floor - height`).
    pub height: f32,
    /// Ambient drift, world units per tick on X / Z.
    pub wind: [f32; 2],
}

/// Field scenes whose look the bank is tuned for, keyed by CDNAME label.
/// Each is a scene whose own art reads as mist or night: the Mist's attack
/// on Rim Elm (`town0b`, a night scene), the Mist-wrapped Drake Castle
/// (`dolk`), the two Voz forests and the Ravine the walkthroughs call the
/// Valleys of Mist.
const SCENE_STYLES: &[(&str, FogStyle)] = &[
    (
        "town0b",
        FogStyle {
            color: [0.70, 0.74, 0.88],
            density: 0.55,
            height: 120.0,
            wind: [0.55, 0.22],
        },
    ),
    (
        "dolk",
        FogStyle {
            color: [0.52, 0.49, 0.60],
            density: 0.50,
            height: 130.0,
            wind: [0.35, -0.45],
        },
    ),
    (
        "vell",
        FogStyle {
            color: [0.56, 0.62, 0.58],
            density: 0.42,
            height: 100.0,
            wind: [0.40, 0.30],
        },
    ),
    (
        "vozz",
        FogStyle {
            color: [0.56, 0.62, 0.58],
            density: 0.42,
            height: 100.0,
            wind: [-0.30, 0.40],
        },
    ),
    (
        "keikoku",
        FogStyle {
            color: [0.62, 0.63, 0.70],
            density: 0.50,
            height: 140.0,
            wind: [0.60, 0.10],
        },
    ),
];

/// The default bank for a field scene whose retail fog pool is live (gate
/// raised and at least one region enabled) but which has no tuned entry.
pub const POOL_STYLE: FogStyle = FogStyle {
    color: [0.58, 0.60, 0.68],
    density: 0.34,
    height: 100.0,
    wind: [0.45, 0.20],
};

/// The bank a field scene gets: its tuned entry, else [`POOL_STYLE`] when
/// the retail fog pool is live, else none. `pool_live` is the retail gate
/// (`_DAT_8007B854`) raised with at least one enabled section-4 region.
pub fn scene_style(label: &str, pool_live: bool) -> Option<FogStyle> {
    let key = label.trim().to_ascii_lowercase();
    SCENE_STYLES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, s)| *s)
        .or(pool_live.then_some(POOL_STYLE))
}

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
    /// Last tick's mover positions, by key.
    prev: Vec<FogMover>,
    /// Whether [`Self::sim_origin`] / [`Self::mesh_origin`] have been seated.
    seated: bool,
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
            ground_gen: 0,
            ticks: 0,
            drift: [0.0; 2],
            scene: String::new(),
            prev: Vec::new(),
            seated: false,
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
        let moved = !self.seated || mwant != self.mesh_origin;
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
    pub fn frame(&self, tint: Option<[f32; 3]>) -> Option<FogVolumeFrame<'_>> {
        if !self.visible() || !self.seated {
            return None;
        }
        let style = self.last_style?;
        let t = tint.unwrap_or([1.0; 3]);
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
            ground_gen: self.ground_gen,
            color: [
                style.color[0] * t[0],
                style.color[1] * t[1],
                style.color[2] * t[2],
            ],
            opacity: style.density * self.strength,
            height: style.height * self.space.height_scale(),
            drift: self.drift,
            ticks: self.ticks,
        })
    }
}

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
    /// [`super::FogSpace::shader_constants`] start here.
    pub const SHADER_CONSTANTS: usize = 18;
    pub const LEN: usize = 22;
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
        h
    }

    /// The sheet mesh's vertex positions, `[x, floor_y, z]` per vertex in
    /// row-major order (retail Y-down) - what both hosts upload when
    /// [`Self::ground_gen`] changes.
    pub fn mesh_positions(&self) -> Vec<f32> {
        let n = MESH_DIM + 1;
        let mut out = Vec::with_capacity(n * n * 3);
        for z in 0..n {
            for x in 0..n {
                out.push(self.mesh_origin[0] + x as f32 * self.mesh_cell);
                out.push(self.ground[z * n + x]);
                out.push(self.mesh_origin[1] + z as f32 * self.mesh_cell);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(_: f32, _: f32) -> f32 {
        0.0
    }

    fn raised() -> FogVolume {
        let mut f = FogVolume::new();
        // Full strength immediately, so the tests need no fade-in ticks.
        f.set_style(Some(POOL_STYLE));
        f.strength = 1.0;
        f
    }

    /// A mover walking +X across the grid for `ticks` ticks at `speed`
    /// units per tick, starting at `x0`.
    fn walk(f: &mut FogVolume, x0: f32, speed: f32, ticks: u32) -> f32 {
        let mut x = x0;
        for _ in 0..ticks {
            f.step([0.0, 0.0], &[FogMover { key: 1, x, z: 0.0 }], flat);
            x += speed;
        }
        x
    }

    #[test]
    fn same_input_same_bank_bit_for_bit() {
        let mut a = raised();
        let mut b = raised();
        walk(&mut a, -600.0, 4.0, 200);
        walk(&mut b, -600.0, 4.0, 200);
        assert_eq!(a.density, b.density);
        assert_eq!(a.velocity, b.velocity);
        let fa = a.frame(None).unwrap();
        let fb = b.frame(None).unwrap();
        assert_eq!(fa.density, fb.density);
        assert_eq!(fa.header(), fb.header());
    }

    #[test]
    fn a_walker_leaves_a_wake_and_untouched_bank_ahead() {
        let mut f = raised();
        let end = walk(&mut f, -600.0, 4.0, 150);
        // Behind the walker (its path), well cleared; ahead, untouched.
        let behind = f.density_at(end - 120.0, 0.0);
        let ahead = f.density_at(end + 400.0, 0.0);
        let beside = f.density_at(end - 120.0, 400.0);
        assert!(behind < 0.6, "wake behind the walker: {behind}");
        assert!(ahead > 0.97, "bank ahead untouched: {ahead}");
        assert!(
            beside > 0.95,
            "bank well beside the path untouched: {beside}"
        );
    }

    #[test]
    fn the_wake_refills_once_the_walker_is_gone() {
        let mut f = raised();
        let end = walk(&mut f, -600.0, 4.0, 150);
        let probe = end - 120.0;
        let carved = f.density_at(probe, 0.0);
        // Walk away out of the grid's reach, then wait.
        for _ in 0..600 {
            f.step([0.0, 0.0], &[], flat);
        }
        let refilled = f.density_at(probe, 0.0);
        assert!(refilled > carved + 0.3, "{carved} -> {refilled}");
        assert!(refilled > 0.9, "refilled {refilled}");
    }

    #[test]
    fn a_stride_carves_harder_than_standing_still() {
        let mut idle = raised();
        let mut moving = raised();
        for _ in 0..30 {
            idle.step(
                [0.0, 0.0],
                &[FogMover {
                    key: 1,
                    x: 0.0,
                    z: 0.0,
                }],
                flat,
            );
        }
        walk(&mut moving, -60.0, 4.0, 30);
        // Compare the darkest cell each one leaves.
        let min = |f: &FogVolume| f.density.iter().cloned().fold(1.0f32, f32::min);
        assert!(
            min(&moving) < min(&idle),
            "{} vs {}",
            min(&moving),
            min(&idle)
        );
        assert!(min(&idle) < 1.0, "standing still opens a pocket");
    }

    #[test]
    fn a_teleport_injects_no_velocity() {
        let mut f = raised();
        f.step(
            [0.0, 0.0],
            &[FogMover {
                key: 7,
                x: -500.0,
                z: 0.0,
            }],
            flat,
        );
        f.step(
            [0.0, 0.0],
            &[FogMover {
                key: 7,
                x: 500.0,
                z: 0.0,
            }],
            flat,
        );
        let vmax = f
            .velocity
            .iter()
            .map(|v| (v[0] * v[0] + v[1] * v[1]).sqrt())
            .fold(0.0f32, f32::max);
        assert!(vmax < 1.0e-6, "teleport stirred the bank: {vmax}");
    }

    #[test]
    fn recentring_keeps_the_wake_where_it_was_carved() {
        let mut f = raised();
        let end = walk(&mut f, -200.0, 4.0, 60);
        let probe = end - 60.0;
        let before = f.density_at(probe, 0.0);
        // Shift the focus by several cells without stepping the sim.
        f.recentre([400.0, 120.0], flat);
        let after = f.density_at(probe, 0.0);
        assert!((before - after).abs() < 1.0e-6, "{before} vs {after}");
    }

    #[test]
    fn the_frame_is_none_while_no_scene_raises_a_bank() {
        let mut f = FogVolume::new();
        f.set_style(None);
        f.step([0.0, 0.0], &[], flat);
        assert!(f.frame(None).is_none());
        assert!(scene_style("town01", false).is_none());
        assert!(scene_style("town0b", false).is_some());
        assert_eq!(scene_style("anything", true), Some(POOL_STYLE));
    }

    #[test]
    fn strength_eases_in_and_out() {
        let mut f = FogVolume::new();
        f.set_style(Some(POOL_STYLE));
        assert!(f.strength > 0.0 && f.strength < 1.0);
        for _ in 0..200 {
            f.set_style(Some(POOL_STYLE));
        }
        assert_eq!(f.strength, 1.0);
        f.set_style(None);
        assert!(
            f.strength < 1.0 && f.visible(),
            "fades out on the last style"
        );
        for _ in 0..200 {
            f.set_style(None);
        }
        assert!(!f.visible());
    }

    #[test]
    fn mesh_follows_the_floor_heights() {
        let mut f = raised();
        f.recentre([1000.0, 2000.0], |x, z| -(x + z) * 0.01);
        let fr = f.frame(None).unwrap();
        let pos = fr.mesh_positions();
        assert_eq!(pos.len(), (MESH_DIM + 1) * (MESH_DIM + 1) * 3);
        for v in pos.chunks(3) {
            assert!((v[1] - (-(v[0] + v[2]) * 0.01)).abs() < 1.0e-3);
        }
        assert_eq!(mesh_indices().len(), MESH_DIM * MESH_DIM * 6);
    }
}
