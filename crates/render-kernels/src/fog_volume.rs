//! Volumetric ground fog - an **enhancement**, not a retail system.
//!
//! Retail's only fog is the `fog_set` puff pool (`legaia_engine_field::fog_particles`):
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
//! `World` owns one (`World::fog_volume`) and steps it at
//! the end of every `World::tick` while
//! `WorldToggles::volumetric_fog` is raised (the one
//! engine-side setting both play hosts flip, from
//! `legaia_engine_menus::options::OptionsState::volumetric_fog`). Each host draws
//! `World::fog_volume_frame` after its 3D scene and before
//! its HUD: the native renderer's `fog_volume` pass and the browser play
//! page's `webgl-fog-volume.js`, two transcriptions of one shading recipe
//! (see [`FOG_SHADER_CONSTANTS`]).

mod frame;
mod ground;
#[cfg(test)]
mod ground_tests;
mod interior;
mod luminance;
mod region;
mod style;
#[cfg(test)]
mod tests;
mod volume;

pub use frame::*;
pub use ground::*;
pub use interior::*;
pub use luminance::*;
pub use region::*;
pub use style::*;
pub use volume::*;

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

/// The rim spill's shading constants, shared by both shader transcriptions
/// through the frame header ([`header::SPILL_CONSTANTS`]):
///
/// ```text
/// [0] tendril scale along the rim (1 / world units) - how wide a tendril is
/// [1] tendril cells down the whole fall (SPILL_DEPTH / a tendril's length)
/// [2] pour-mask scale along the rim (1 / world units) - how long a stretch
///     of rim pours (or barely does) before the next
/// [3] how far outward the sheets spread on the curtain, world units - its
///     depth as a slab
/// ```
pub const FOG_SPILL_CONSTANTS: [f32; 4] = [1.0 / 220.0, SPILL_DEPTH / 520.0, 1.0 / 1300.0, 120.0];

/// The distant haze's shading constants ([`header::HAZE_CONSTANTS`]):
///
/// ```text
/// [0] layer stack depth relative to the bank's height (a thicker layer)
/// [1] alpha gain relative to the bank's
/// [2] noise scale (1 / world units) - broad, soft banks
/// [3] share of the bank's drift it moves at (slower than the bank)
/// ```
pub const FOG_HAZE_CONSTANTS: [f32; 4] = [8.0, 4.0, 1.0 / 2400.0, 0.3];
