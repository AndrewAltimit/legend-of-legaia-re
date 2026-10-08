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

use crate::fog_particles::FogRegion;

/// Disturbance-grid cells per side.
pub const SIM_DIM: usize = 64;
/// Sheet-mesh quads per side (the mesh has `MESH_DIM + 1` vertices a side).
pub const MESH_DIM: usize = 48;

/// Floor steps between neighbouring sheet-mesh vertices (world units) over
/// which the bank fades out: below `LO` a slope or stair is still floor,
/// past `HI` it is a cliff or wall between two tiers.
pub const FLOOR_STEP_LO: f32 = 12.0;
pub const FLOOR_STEP_HI: f32 = 40.0;

/// Movement past this many units in one tick is a seat / warp, not a stride:
/// the mover's history restarts and it injects no velocity.
const TELEPORT_UNITS: f32 = 96.0;
/// Fraction of the remaining gap to an undisturbed bank refilled per tick:
/// a cleared cell is back to ~90% about seven seconds after it was carved.
const REFILL: f32 = 0.005;
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
const CARVE_MOVE: f32 = 0.8;
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
pub const FOG_SHADER_CONSTANTS: [f32; 4] = [1.0 / 380.0, 1.0 / 1400.0, 6.0, 2.2];

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
            FogSpace::Field => 84.0,
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
        // A battle's low, close framing looks through the bank at a grazing
        // angle across few sheets' worth of crest; it takes a larger gain to
        // read as the thick layer the field shows from above.
        let gain = match self {
            FogSpace::Field => 1.0,
            FogSpace::Battle => 2.6,
        };
        [c[0] / k, c[1] / k, c[2] * gain, c[3]]
    }

    /// Opacity relative to a style's: a battle is framed low and close on
    /// the fighters' legs, where the concept is a thick swirling layer.
    pub fn density_scale(self) -> f32 {
        match self {
            FogSpace::Field => 1.0,
            FogSpace::Battle => 2.2,
        }
    }

    /// View-depth span (clip `w` of the host's matrix for this space) over
    /// which a sheet fades into the surface behind it - the soft
    /// intersection that keeps walls, ledges and legs from cutting it with
    /// a hard line. The battle matrix carries the stage's world scale.
    pub fn soft_distance(self) -> f32 {
        match self {
            FogSpace::Field => 50.0,
            FogSpace::Battle => 160.0,
        }
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
            density: 0.8,
            height: 95.0,
            wind: [0.55, 0.22],
        },
    ),
    (
        "dolk",
        FogStyle {
            color: [0.52, 0.49, 0.60],
            density: 0.75,
            height: 85.0,
            wind: [0.35, -0.45],
        },
    ),
    (
        "vell",
        FogStyle {
            color: [0.56, 0.62, 0.58],
            density: 0.62,
            height: 85.0,
            wind: [0.40, 0.30],
        },
    ),
    (
        "vozz",
        FogStyle {
            color: [0.56, 0.62, 0.58],
            density: 0.62,
            height: 85.0,
            wind: [-0.30, 0.40],
        },
    ),
    (
        "keikoku",
        FogStyle {
            color: [0.62, 0.63, 0.70],
            density: 0.72,
            height: 110.0,
            wind: [0.60, 0.10],
        },
    ),
];

/// The default bank for a field scene whose retail fog pool is live (gate
/// raised and at least one region enabled) but which has no tuned entry.
pub const POOL_STYLE: FogStyle = FogStyle {
    color: [0.58, 0.60, 0.68],
    density: 0.5,
    height: 85.0,
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
/// `0x801D6320..0x801D63B8`, [`crate::fog_particles::FogPool::spawn`]) - the
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

/// How bright the bank may be relative to the scene's measured luminance
/// `L`: at most `FOG_LUMA_FLOOR + FOG_LUMA_OVER_SCENE * L`. The floor keeps a
/// night bank readable as a moonlit haze against a dark stage (a mist reads
/// by contrast, it does not vanish); the slope keeps it below a lit scene's
/// brightness.
pub const FOG_LUMA_FLOOR: f32 = 0.17;
pub const FOG_LUMA_OVER_SCENE: f32 = 1.8;

/// Rec. 601 luma of a framebuffer-space colour.
pub fn luma(c: [f32; 3]) -> f32 {
    0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
}

/// The factor (`<= 1`) that dims `color` to the brightness a scene of
/// luminance `scene_luma` allows ([`FOG_LUMA_FLOOR`],
/// [`FOG_LUMA_OVER_SCENE`]).
pub fn luma_scale(color: [f32; 3], scene_luma: f32) -> f32 {
    let l = luma(color);
    if l <= 1.0e-6 {
        return 1.0;
    }
    ((FOG_LUMA_FLOOR + scene_luma.max(0.0) * FOG_LUMA_OVER_SCENE) / l).min(1.0)
}

/// The scene's luminance as the player sees it, measured from its meshes:
/// every textured triangle's texel at its UV centroid (through its CLUT),
/// modulated by the prim's baked colour word (`texel * colour / 128`, the
/// field shading), weighted by the triangle's world area and averaged.
/// Transparent texels (the zero word) are skipped. `None` when nothing
/// textured was found. Deterministic; both hosts call it on the same
/// meshes (the field scene's pool at entry, the battle stage dome at
/// battle entry).
pub fn scene_luminance<'a>(
    vram: &legaia_tim::Vram,
    meshes: impl IntoIterator<Item = &'a legaia_tmd::mesh::VramMesh>,
) -> Option<f32> {
    let (mut sum, mut weight) = (0.0f64, 0.0f64);
    for m in meshes {
        for tri in m.indices.as_chunks::<3>().0 {
            let [a, b, c] = tri.map(|i| i as usize);
            if a >= m.positions.len() || b >= m.positions.len() || c >= m.positions.len() {
                continue;
            }
            let (pa, pb, pc) = (m.positions[a], m.positions[b], m.positions[c]);
            let e1 = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
            let e2 = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
            let cr = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let area = 0.5 * ((cr[0] * cr[0] + cr[1] * cr[1] + cr[2] * cr[2]) as f64).sqrt();
            if area <= 0.0 {
                continue;
            }
            let u = (u32::from(m.uvs[a][0]) + u32::from(m.uvs[b][0]) + u32::from(m.uvs[c][0])) / 3;
            let v = (u32::from(m.uvs[a][1]) + u32::from(m.uvs[b][1]) + u32::from(m.uvs[c][1])) / 3;
            let [cba, tsb] = m.cba_tsb[a];
            let Some(word) = texel_word(vram, cba, tsb, u as usize, v as usize) else {
                continue;
            };
            let rgb = [
                f32::from(word & 31) / 31.0,
                f32::from((word >> 5) & 31) / 31.0,
                f32::from((word >> 10) & 31) / 31.0,
            ];
            let col = m.colors.get(a).copied().unwrap_or([0x80; 3]);
            let lit = [
                (rgb[0] * f32::from(col[0]) / 128.0).min(1.0),
                (rgb[1] * f32::from(col[1]) / 128.0).min(1.0),
                (rgb[2] * f32::from(col[2]) / 128.0).min(1.0),
            ];
            sum += f64::from(luma(lit)) * area;
            weight += area;
        }
    }
    (weight > 0.0).then(|| (sum / weight) as f32)
}

/// [`scene_luminance`] of a battle stage shell as drawn: the backdrop TMD's
/// drawn objects (`SceneHost::battle_stage_object_indices`), textured prims,
/// over the battle VRAM. The one measurement both hosts take at battle entry
/// and store in [`FogVolume::battle_luma`]; the shell's second copy repeats
/// the first, so it is not measured twice.
pub fn stage_luminance(
    vram: &legaia_tim::Vram,
    tmd: &legaia_tmd::Tmd,
    raw: &[u8],
    objects: &[usize],
) -> Option<f32> {
    let shell = legaia_asset::battle_backdrop::objects_tmd(tmd, objects);
    let mesh = legaia_tmd::mesh::tmd_to_vram_mesh(&shell, raw);
    scene_luminance(vram, [&mesh])
}

/// The 15-bit word a textured prim samples at `(u, v)`, through its CLUT
/// for the indexed depths; `None` for the transparent zero word.
fn texel_word(vram: &legaia_tim::Vram, cba: u16, tsb: u16, u: usize, v: usize) -> Option<u16> {
    let cx = usize::from(cba & 0x3F) * 16;
    let cy = usize::from((cba >> 6) & 0x1FF);
    let tx = usize::from(tsb & 0xF) * 64;
    let ty = usize::from((tsb >> 4) & 1) * 256;
    let px = |x: usize, y: usize| -> u16 {
        if x >= legaia_tim::VRAM_WIDTH || y >= legaia_tim::VRAM_HEIGHT {
            0
        } else {
            vram.pixel(x, y)
        }
    };
    let w = match (tsb >> 7) & 3 {
        0 => {
            let i = (px(tx + (u >> 2), ty + v) >> ((u & 3) * 4)) & 0xF;
            px(cx + usize::from(i), cy)
        }
        1 => {
            let i = (px(tx + (u >> 1), ty + v) >> ((u & 1) * 8)) & 0xFF;
            px(cx + usize::from(i), cy)
        }
        _ => px(tx + u, ty + v),
    };
    (w != 0).then_some(w)
}

/// Walk areas (in 64-unit sub-cells) up to this size can be interiors; a
/// larger one is always open ground. House rooms, castle halls and the
/// corridors between them run from a few dozen sub-cells to about 750
/// across the scenes that raise a bank; the streets, plazas and forest
/// floors they hang off run past 1,300.
pub const INTERIOR_MAX_SUBCELLS: u32 = 800;

/// Sub-cells per side of the walk-area lattice (four per 128-unit tile).
const AREA_STRIDE: usize = 0x100;

/// Which walk areas of the current field scene are interiors, learned from
/// how the player moves between them.
///
/// Legaia lays a town's house rooms (and a castle's halls) out in the same
/// scene map as its streets, each an island of floor at its own tiles, and
/// a door is an **intra-scene warp** to it rather than a scene change
/// (`docs/formats/encounter.md`). The fog-region table bounds some scenes'
/// fog to their streets ([`region_weight`]), but many keep one region over
/// the whole map. So the bank also tracks the walk areas - the 4-connected
/// open floor components the locomotion collision leaves - and classifies
/// them:
///
/// - the area the player is first seen standing in after entering the scene
///   is **open ground** (an entrance from another scene, a picker seat);
/// - walking (no warp) into another area carries the current class over -
///   a stair whose tiles carry no floor bit splits one street into two
///   areas, and that is still the street;
/// - a **warp** (a jump past [`TELEPORT_UNITS`] in one tick) into an area
///   not yet classified makes it an **interior** when it is room-sized
///   ([`INTERIOR_MAX_SUBCELLS`]), open ground otherwise; a warp into a
///   classified area takes its class.
///
/// The bank fades out while the player is in an interior and snaps off on
/// the warp in (the door is a cut). Nothing here is retail: retail's pool
/// spawns wherever its region table lets it, interior or not.
#[derive(Debug, Clone, Default)]
pub struct InteriorTracker {
    /// Area id per sub-cell (`sz * 0x100 + sx`, `0` closed, ids 1-based);
    /// empty until seeded.
    labels: Vec<u16>,
    /// Sub-cell count per area (`sizes[id - 1]`).
    sizes: Vec<u32>,
    /// Per area: `0` unknown, `1` open ground, `2` interior.
    class: Vec<u8>,
    /// The player's last observed position and area.
    last: Option<([i32; 2], u16)>,
    /// Whether the player stands in an interior.
    pub indoors: bool,
}

impl InteriorTracker {
    /// Forget everything - a new scene.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Whether [`Self::seed`] has run for this scene.
    pub fn seeded(&self) -> bool {
        !self.labels.is_empty()
    }

    /// Install the scene's walk areas (`labels` over the `0x100 x 0x100`
    /// sub-cell lattice, `sizes` per 1-based id).
    pub fn seed(&mut self, labels: Vec<u16>, sizes: Vec<u32>) {
        self.class = vec![0; sizes.len()];
        self.labels = labels;
        self.sizes = sizes;
        self.last = None;
        self.indoors = false;
    }

    /// The area id at world `(x, z)`, `0` off the open floor.
    pub fn area_at(&self, x: i32, z: i32) -> u16 {
        if x < 0 || z < 0 {
            return 0;
        }
        let (sx, sz) = ((x >> 6) as usize, (z >> 6) as usize);
        if sx >= AREA_STRIDE || sz >= AREA_STRIDE {
            return 0;
        }
        self.labels.get(sz * AREA_STRIDE + sx).copied().unwrap_or(0)
    }

    /// Observe the player at world `(x, z)` this tick. Returns `true` on the
    /// tick a warp carries the player into an interior.
    pub fn observe(&mut self, x: i32, z: i32) -> bool {
        if !self.seeded() {
            return false;
        }
        let area = self.area_at(x, z);
        let Some((prev, prev_area)) = self.last else {
            // First sight in this scene: where the player came in.
            if area != 0 {
                self.set_class(area, 1);
                self.indoors = false;
                self.last = Some(([x, z], area));
            }
            return false;
        };
        let (dx, dz) = ((x - prev[0]) as f32, (z - prev[1]) as f32);
        let warped = (dx * dx + dz * dz).sqrt() > TELEPORT_UNITS;
        let mut entered = false;
        if area != 0 && area != prev_area {
            let known = self.class[usize::from(area) - 1];
            let class = match known {
                0 if warped => {
                    if self.sizes[usize::from(area) - 1] <= INTERIOR_MAX_SUBCELLS {
                        2
                    } else {
                        1
                    }
                }
                0 => {
                    if self.indoors {
                        2
                    } else {
                        1
                    }
                }
                k => k,
            };
            self.set_class(area, class);
            let indoors = class == 2;
            entered = warped && indoors && !self.indoors;
            self.indoors = indoors;
        }
        // A closed sub-cell (a doorway, a stair without a floor bit) keeps
        // the last area, so the next open one is compared against it.
        let keep = if area == 0 { prev_area } else { area };
        self.last = Some(([x, z], keep));
        entered
    }

    fn set_class(&mut self, area: u16, class: u8) {
        if let Some(c) = self.class.get_mut(usize::from(area).wrapping_sub(1)) {
            *c = class;
        }
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
    /// [`super::FogSpace::shader_constants`] start here.
    pub const SHADER_CONSTANTS: usize = 18;
    /// [`super::FogSpace::soft_distance`].
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
        let fa = a.frame(None, None).unwrap();
        let fb = b.frame(None, None).unwrap();
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
        assert!(f.frame(None, None).is_none());
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
    fn a_dark_scene_dims_the_bank_and_a_bright_one_does_not() {
        let f = raised();
        let mut f = f;
        f.recentre([0.0, 0.0], flat);
        let bright = f.frame(None, Some(0.9)).unwrap().color;
        let dark = f.frame(None, Some(0.05)).unwrap().color;
        assert_eq!(bright, POOL_STYLE.color);
        assert!(luma(dark) <= FOG_LUMA_FLOOR + 0.05 * FOG_LUMA_OVER_SCENE + 1.0e-5);
        assert!(luma(dark) < luma(bright));
        // Hue kept: the channels scale together.
        let r = dark[0] / POOL_STYLE.color[0];
        assert!((dark[2] / POOL_STYLE.color[2] - r).abs() < 1.0e-5);
    }

    #[test]
    fn scene_luminance_reads_lit_texels() {
        let mut vram = legaia_tim::Vram::new();
        // A 15-bit page at (0, 0): mid-grey 16/31 everywhere it is sampled.
        let grey: u16 = 16 | (16 << 5) | (16 << 10);
        let bytes: Vec<u8> = std::iter::repeat_n(grey.to_le_bytes(), 64 * 4)
            .flatten()
            .collect();
        vram.write_block(0, 0, 64, 4, &bytes);
        let mesh = legaia_tmd::mesh::VramMesh {
            positions: vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [0.0, 0.0, 100.0]],
            uvs: vec![[0, 0], [2, 0], [0, 2]],
            cba_tsb: vec![[0, 2 << 7]; 3],
            indices: vec![0, 1, 2],
            normals: vec![[0.0; 3]; 3],
            colors: vec![[0x40; 3]; 3],
        };
        let l = scene_luminance(&vram, [&mesh]).unwrap();
        // grey 16/31 at half modulation.
        assert!((l - 16.0 / 31.0 * 0.5).abs() < 1.0e-3, "{l}");
    }

    #[test]
    fn a_floor_step_clears_the_bank_off_the_cliff() {
        let mut f = raised();
        // A 200-unit cliff along x = 0: upper tier west of it.
        f.recentre([0.0, 0.0], |x, _| if x < 0.0 { -200.0 } else { 0.0 });
        let fr = f.frame(None, None).unwrap();
        let pos = fr.mesh_positions();
        for v in pos.chunks(4) {
            let near_cliff = v[0] > -2.0 * fr.mesh_cell && v[0] < 2.0 * fr.mesh_cell;
            if (v[0] + fr.mesh_cell / 2.0).abs() < fr.mesh_cell || v[0].abs() < 1.0 {
                assert_eq!(v[3], 0.0, "vertex at x {} stands on the step", v[0]);
            } else if !near_cliff {
                assert_eq!(v[3], 1.0, "vertex at x {} is open floor", v[0]);
            }
        }
    }

    #[test]
    fn mesh_follows_the_floor_heights() {
        let mut f = raised();
        f.recentre([1000.0, 2000.0], |x, z| -(x + z) * 0.01);
        let fr = f.frame(None, None).unwrap();
        let pos = fr.mesh_positions();
        assert_eq!(pos.len(), (MESH_DIM + 1) * (MESH_DIM + 1) * 4);
        for v in pos.chunks(4) {
            assert!((v[1] - (-(v[0] + v[2]) * 0.01)).abs() < 1.0e-3);
            // A gentle slope (under 3 units per quad) is floor throughout.
            assert_eq!(v[3], 1.0);
        }
        assert_eq!(mesh_indices().len(), MESH_DIM * MESH_DIM * 6);
    }

    fn region(enabled: bool, x0: u8, z0: u8, x1: u8, z1: u8) -> FogRegion {
        FogRegion {
            enabled,
            x0,
            z0,
            x1,
            z1,
            angle_base: 0,
            angle_spread: 0,
            speed: 0,
            byte_8: 0,
            flag_index: 0,
        }
    }

    /// A lattice with a street (area 1, large), a room (area 2, small) and a
    /// second street strip (area 3, small) reached on foot.
    fn town() -> InteriorTracker {
        let mut labels = vec![0u16; AREA_STRIDE * AREA_STRIDE];
        let mut fill = |x0: usize, z0: usize, x1: usize, z1: usize, id: u16| {
            for z in z0..z1 {
                for x in x0..x1 {
                    labels[z * AREA_STRIDE + x] = id;
                }
            }
        };
        fill(0, 0, 60, 40, 1); // street, 2400 sub-cells
        fill(200, 200, 210, 210, 2); // room, 100
        fill(61, 0, 70, 10, 3); // strip past a closed stair column, 90
        let mut t = InteriorTracker::default();
        t.seed(labels, vec![2400, 100, 90]);
        t
    }

    fn at(sx: i32, sz: i32) -> (i32, i32) {
        (sx * 64 + 32, sz * 64 + 32)
    }

    #[test]
    fn a_door_warp_into_a_room_is_indoors_and_back_out_is_not() {
        let mut t = town();
        let (x, z) = at(10, 10);
        assert!(!t.observe(x, z));
        assert!(!t.indoors);
        // Door: one tick, far away, into the small area.
        let (x, z) = at(205, 205);
        assert!(t.observe(x, z), "the warp in reports the cut");
        assert!(t.indoors);
        // Walking about the room stays indoors and reports nothing new.
        let (x, z) = at(206, 205);
        assert!(!t.observe(x, z));
        assert!(t.indoors);
        // Door back out onto the street.
        let (x, z) = at(10, 12);
        assert!(!t.observe(x, z));
        assert!(!t.indoors);
        // And in again: the room is remembered.
        let (x, z) = at(205, 205);
        assert!(t.observe(x, z));
        assert!(t.indoors);
    }

    #[test]
    fn walking_into_a_small_area_keeps_the_street_class() {
        let mut t = town();
        let (x, z) = at(58, 5);
        t.observe(x, z);
        // Two sub-cells east, across the closed column, on foot.
        for sx in [59, 60, 61, 62] {
            let (x, z) = at(sx, 5);
            assert!(!t.observe(x, z));
        }
        assert!(!t.indoors, "a stair-split strip is still the street");
        // Even a later warp into it keeps it open ground.
        let (x, z) = at(10, 30);
        t.observe(x, z);
        let (x, z) = at(65, 5);
        assert!(!t.observe(x, z));
        assert!(!t.indoors);
    }

    #[test]
    fn a_warp_into_a_large_area_is_open_ground() {
        let mut t = town();
        // Enter the scene in the room (a card load): that is where the
        // player came in, so it counts as open ground.
        let (x, z) = at(205, 205);
        t.observe(x, z);
        assert!(!t.indoors);
        let (x, z) = at(10, 10);
        assert!(!t.observe(x, z));
        assert!(!t.indoors);
    }

    #[test]
    fn region_weight_is_the_spawner_rule() {
        // No table: the whole scene.
        assert_eq!(region_weight(&[], 90, 90), 1.0);
        // A town box: open bounds, nothing outside it.
        let town = [region(true, 0, 0, 56, 48)];
        assert_eq!(region_weight(&town, 20, 20), 1.0);
        assert_eq!(region_weight(&town, 0, 20), 0.0, "open lower bound");
        assert_eq!(region_weight(&town, 56, 20), 0.0, "open upper bound");
        assert_eq!(region_weight(&town, 97, 54), 0.0, "a house room beside it");
        // A disabled box ahead of an area-wide one is a hole; the same box
        // enabled is fog like the rest.
        let holed = [region(false, 24, 0, 45, 22), region(true, 0, 0, 126, 126)];
        assert_eq!(region_weight(&holed, 30, 10), 0.0);
        assert_eq!(region_weight(&holed, 60, 60), 1.0);
        let filled = [region(true, 24, 0, 45, 22), region(true, 0, 0, 126, 126)];
        assert_eq!(region_weight(&filled, 30, 10), 1.0);
        // A disabled first hit ends the search even under an enabled box.
        let off = [region(false, 0, 0, 126, 126)];
        assert_eq!(region_weight(&off, 60, 60), 0.0);
    }

    #[test]
    fn regions_mask_the_sheet_mesh_and_a_change_resamples_it() {
        let mut f = raised();
        let focus = [64.0 * 128.0, 64.0 * 128.0];
        f.recentre(focus, flat);
        let gen0 = f.ground_gen;
        let all = |f: &FogVolume| f.ground_weight.iter().all(|w| *w == 1.0);
        assert!(all(&f), "no table: every floor vertex carries bank");

        // A box over the west half of the mesh only.
        f.set_regions(&[region(true, 0, 0, 64, 127)]);
        assert!(f.recentre(focus, flat), "a new table re-samples in place");
        assert_ne!(f.ground_gen, gen0);
        let n = MESH_DIM + 1;
        for z in 0..n {
            for x in 0..n {
                let tx = f.mesh_origin[0] + x as i32;
                let tz = f.mesh_origin[1] + z as i32;
                let want = if 0 < tx && tx < 64 && 0 < tz && tz < 127 {
                    1.0
                } else {
                    0.0
                };
                assert_eq!(f.ground_weight[z * n + x], want, "tile ({tx}, {tz})");
            }
        }
        // The same table again does not re-sample.
        let gen1 = f.ground_gen;
        f.set_regions(&[region(true, 0, 0, 64, 127)]);
        assert!(!f.recentre(focus, flat));
        assert_eq!(f.ground_gen, gen1);

        // Battle space ignores the field table.
        f.set_space(FogSpace::Battle);
        f.recentre([0.0, 0.0], flat);
        assert!(all(&f));
    }
}
