//! The field-to-battle transition's **tile-shatter** style: the captured field
//! screen breaks into a 16x16 grid of textured tiles that tumble away.
//!
//! Three retail routines make one style, and the numbers tie them together:
//!
//! | Retail | Here | Job |
//! |---|---|---|
//! | `FUN_801D081C` | [`seed_tile_grid`] | allocate + fill the vertex grid and the 256 tile records |
//! | `FUN_801D0D24` | [`tick_tile_grid`] | one frame: walk all 256 records, then advance the entity clock |
//! | `FUN_801D0E54` | [`step_tile`] | one record: the draw gate and the integration |
//!
//! The allocation sizes are what fix the shape independently of the loop
//! bounds. `FUN_801D081C` asks for `0x908` and `0x5C00` bytes:
//! `0x908 == 17 * 17 * 8` is the corner-vertex grid ([`GRID_DIM`] squared,
//! 8-byte `SVECTOR`s) and `0x5C00 == 256 * 0x5C` is the tile-record array
//! ([`TILE_DIM`] squared at [`TILE_STRIDE`]). `FUN_801D0D24` then walks exactly
//! `0x100` records at `0x5C` apart out of the same block, which is what pairs
//! the two beyond doubt - unlike the two particle styles, whose seeder pairing
//! is not established (see [`crate::battle_intro_particles`]).
//!
//! ## The velocities live in the corner vectors' pad halfwords
//!
//! A tile record carries eight 8-byte corner `SVECTOR`s - four front corners at
//! `+0x14` (z `-0x80`) and four back corners at `+0x34` (z `+0x80`), so the tile
//! has thickness and reads as a solid when it tumbles. Each `SVECTOR`'s fourth
//! halfword is padding the GTE ignores, and the seeder packs the tile's
//! angular and linear velocity into five of those pads:
//!
//! | pad | corner | holds |
//! |---|---|---|
//! | `+0x1A` | front 0 | angular velocity about x |
//! | `+0x22` | front 1 | angular velocity about y |
//! | `+0x2A` | front 2 | angular velocity about z |
//! | `+0x3A` | back 0 | linear velocity along x |
//! | `+0x42` | back 1 | linear velocity along y |
//!
//! Reading `+0x1A` as "corner 0's w" and as "the x spin rate" are the same
//! read; the port keeps them as named fields because nothing else would.
//!
//! ## Two of the three sub-styles have no tumble at all
//!
//! `FUN_801D081C` writes `sin >> 5` / `cos >> 5` into `+0x1A` / `+0x22` and
//! then **immediately stores zero over both** (`801d0bac` / `801d0bb0`, two
//! instructions after the stores that produced them). The `DAT_801D2464 == 2`
//! arm is the only one that writes them again. Since [`step_tile`] doubles both
//! pads every frame, a zero stays zero: sub-styles `0` and `1` spin only about
//! z, and only sub-style `2` tumbles. The dead stores are retail's, and they
//! are reproduced as *not* happening rather than as happening-then-undone.
//!
//! Provenance: `see ghidra/scripts/funcs/overlay_field_battle_intro_801d081c.txt`,
//! `..._801d0d24.txt` and `..._801d0e54.txt` - disassembly, not the C.

/// Corner-vertex grid dimension (`slti v0,s3,0x11` in both grid loops).
pub const GRID_DIM: usize = 0x11;
/// Tile grid dimension (`slti v0,s3,0x10`).
pub const TILE_DIM: usize = 0x10;
/// Byte stride of one tile record.
pub const TILE_STRIDE: usize = 0x5C;
/// Bytes `FUN_80017888` is asked for the corner grid: `17 * 17 * 8`.
pub const GRID_BLOCK_BYTES: usize = GRID_DIM * GRID_DIM * 8;
/// Bytes `FUN_80017888` is asked for the tile records: `256 * 0x5C`.
pub const TILE_BLOCK_BYTES: usize = TILE_DIM * TILE_DIM * TILE_STRIDE;

const _: () = assert!(GRID_BLOCK_BYTES == 0x908);
const _: () = assert!(TILE_BLOCK_BYTES == 0x5C00);

/// The value the seeder writes to the entity's `+0x74` before allocating - the
/// same word [`crate::battle_intro_particles`]'s two seeders write.
pub const TILE_ENTITY_MASK: u32 = 0x00FF_FFFF;

/// What an allocation failure adds to `_DAT_8007B828`, matching the particle
/// seeders.
pub const ALLOC_FAILURE_PENALTY: i32 = 10;

/// Corner-grid x of column 0 (`addiu s2,s4,-0xa00`), stepping by
/// [`GRID_X_STEP`]; the grid therefore spans `-0xA00..=0xA00`.
pub const GRID_X_ORIGIN: i32 = -0xA00;
/// Corner-grid x step per column (`addiu s4,s4,0x140`).
pub const GRID_X_STEP: i32 = 0x140;
/// Corner-grid y of row 0 (`addiu s0,v0,-0x800`), stepping by [`GRID_Y_STEP`].
pub const GRID_Y_ORIGIN: i32 = -0x800;
/// Corner-grid y step per row (`sll v0,s5,0x8`).
pub const GRID_Y_STEP: i32 = 0x100;
/// The z every corner vertex is seeded at (`li v1,0x800`).
pub const GRID_Z: i16 = 0x800;

/// Tile-origin x of column 0 (`li s2,-0x960`), stepping by [`TILE_X_STEP`].
pub const TILE_X_ORIGIN: i32 = -0x960;
/// Tile-origin x step (`addiu s2,s2,0x140`).
pub const TILE_X_STEP: i32 = 0x140;
/// The y **stored** in a tile record's `+0x06` for row 0 (`li s6,-0x6e0`).
pub const TILE_STORED_Y_ORIGIN: i32 = -0x6E0;
/// The y the tile's corners are made **relative to** for row 0
/// (`li s4,-0x780`). It is `0xA0` below [`TILE_STORED_Y_ORIGIN`], and both step
/// by [`TILE_Y_STEP`] - so the stored origin and the corner pivot are
/// deliberately offset from each other. That is retail's, not a transcription
/// slip: `s4` and `s6` are separate registers advanced in the same loop tail.
pub const TILE_PIVOT_Y_ORIGIN: i32 = -0x780;
/// Row step shared by [`TILE_STORED_Y_ORIGIN`] and [`TILE_PIVOT_Y_ORIGIN`].
pub const TILE_Y_STEP: i32 = 0x100;

/// The `+0x08` word every tile record is seeded with - the z component of the
/// rotation vector `FUN_801D0D24` feeds `RotMatrix` (`li v0,0x880`).
pub const TILE_ROT_Z_SEED: i16 = 0x880;

/// Texture-page word for tile columns `0..=8` (`li v0,0x135`).
pub const TILE_TPAGE_LEFT: i16 = 0x135;
/// Texture-page word for tile columns `9..=15` (`li v0,0x137`) - the captured
/// screen is wider than one 256-texel page, so the right-hand columns sample
/// the next page.
pub const TILE_TPAGE_RIGHT: i16 = 0x137;
/// Column at which the record switches to [`TILE_TPAGE_RIGHT`]
/// (`slti v0,s3,0x9`).
pub const TILE_TPAGE_SPLIT_COL: usize = 9;
/// The u bias subtracted on the right-hand page (`li t4,-0x80`, applied as
/// `u = (raw >> 4) - bias`, so it *adds* `0x80`).
pub const TILE_RIGHT_U_BIAS: i8 = -0x80;

/// Front-face corner z (`li v0,-0x80`).
pub const TILE_FRONT_Z: i16 = -0x80;
/// Back-face corner z (`li v0,0x80`).
pub const TILE_BACK_Z: i16 = 0x80;

/// Interior-vertex jitter step along x (`a3 * 5 << 4`).
pub const JITTER_X_STEP: i32 = 0x50;
/// Interior-vertex jitter step along y (`a3 << 6`).
pub const JITTER_Y_STEP: i32 = 0x40;

/// Per-frame increment of a tile's `+0x00` progress counter
/// (`sll v0,v1,0x6` on the frame step).
pub const TILE_PROGRESS_STEP: i32 = 0x40;

/// The tile is no longer drawn once `+0x00` reaches this
/// (`slti v0,v0,0x1000` at `801d0e84`).
pub const TILE_PROGRESS_LIMIT: i16 = 0x1000;

/// Scale applied to the entity clock before it is compared against a tile's
/// `+0x0A` spawn delay (`sll v0,v1,0x4; subu v0,v0,v1; sll s2,v0,0x2`, i.e.
/// `elapsed * 60`).
pub const TILE_DELAY_SCALE: i32 = 0x3C;

/// The transition sub-style, `DAT_801D2464`. It selects only how the seeder
/// fills a tile's z-spin rate and spawn delay; every other field is shared.
///
/// Values outside `0..=2` reach no arm at all: the dispatch is
/// `== 1` / `< 2 && == 0` / `>= 2 && == 2`, so a fourth value leaves `+0x0A`
/// and `+0x2A` at whatever the corner writes left there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileSubStyle {
    /// `DAT_801D2464 == 0`: z spin `-0x60`, delay `rand() % 5000`.
    NegSpinRandomDelay,
    /// `DAT_801D2464 == 1`: z spin `+0x60`, delay `rand() % 4000`.
    PosSpinRandomDelay,
    /// `DAT_801D2464 == 2`: z spin `-0x20`, delay `sqrt(x^2 + y^2) >> 5`, and
    /// the only arm that leaves the x/y tumble rates non-zero.
    RadialDelayWithTumble,
    /// Anything else - no arm runs.
    None,
}

/// One 8-byte corner vertex of the `17 x 17` grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GridVertex {
    /// `+0x00`.
    pub x: i16,
    /// `+0x02`.
    pub y: i16,
    /// `+0x04` - always [`GRID_Z`].
    pub z: i16,
}

/// One corner of a tile: an `SVECTOR` whose pad halfword doubles as a velocity
/// slot on five of the eight corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TileCorner {
    /// `+0x00` - x, relative to the tile's pivot.
    pub x: i16,
    /// `+0x02` - y, relative to the tile's pivot.
    pub y: i16,
    /// `+0x04` - [`TILE_FRONT_Z`] or [`TILE_BACK_Z`].
    pub z: i16,
    /// `+0x06` - the GTE pad. See the module docs for which five carry a
    /// velocity.
    pub pad: i16,
    /// The `(u, v)` texel this corner samples, from the record's `+0x54..+0x5B`
    /// byte block. Front and back corner `k` share one pair; the record stores
    /// four, not eight.
    pub uv: (i8, i8),
}

/// One `0x5C`-byte tile record, in the fields the two consumers touch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TileRecord {
    /// `+0x00` - progress counter; the draw gate is `< `[`TILE_PROGRESS_LIMIT`].
    pub progress: i16,
    /// `+0x02` - texture-page word.
    pub tpage: i16,
    /// `+0x04` / `+0x06` / `+0x08` - the rotation vector `FUN_801D0D24` hands
    /// `RotMatrix`. Seeded from the tile's own grid position plus
    /// [`TILE_ROT_Z_SEED`], which is what gives every tile a different resting
    /// orientation before it starts to spin.
    pub rot: (i16, i16, i16),
    /// `+0x0A` - spawn delay, held against `elapsed * `[`TILE_DELAY_SCALE`].
    pub delay: i16,
    /// `+0x0C` / `+0x0E` / `+0x10` - translation, seeded to zero.
    pub trans: (i16, i16, i16),
    /// `+0x14..+0x33` - the four front corners.
    pub front: [TileCorner; 4],
    /// `+0x34..+0x53` - the four back corners.
    pub back: [TileCorner; 4],
}

impl TileRecord {
    /// Angular velocity about x - corner `front[0]`'s pad (`+0x1A`).
    pub fn spin_x(&self) -> i16 {
        self.front[0].pad
    }
    /// Angular velocity about y - corner `front[1]`'s pad (`+0x22`).
    pub fn spin_y(&self) -> i16 {
        self.front[1].pad
    }
    /// Angular velocity about z - corner `front[2]`'s pad (`+0x2A`).
    pub fn spin_z(&self) -> i16 {
        self.front[2].pad
    }
    /// Linear velocity along x - corner `back[0]`'s pad (`+0x3A`).
    pub fn vel_x(&self) -> i16 {
        self.back[0].pad
    }
    /// Linear velocity along y - corner `back[1]`'s pad (`+0x42`).
    pub fn vel_y(&self) -> i16 {
        self.back[1].pad
    }
}

/// The whole style-2 working set: what `FUN_801D081C`'s two allocations hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileGrid {
    /// `DAT_801D246C` - the `17 x 17` corner grid, row-major.
    pub vertices: Vec<GridVertex>,
    /// `DAT_801D2468` - the `16 x 16` tile records, row-major.
    pub tiles: Vec<TileRecord>,
}

/// What [`seed_tile_grid`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileSeedOutcome {
    /// Either allocation came back null. The caller adds
    /// [`ALLOC_FAILURE_PENALTY`] to `_DAT_8007B828` and nothing is written.
    OutOfMemory,
    /// The working set, ready for [`tick_tile_grid`].
    Seeded(Box<TileGrid>),
}

/// The trig tables, integer square root and PRNG the seeder reaches into -
/// the same set [`crate::battle_intro_particles::ParticleEnv`] abstracts, so a
/// host implements one trait for both.
pub use crate::battle_intro_particles::ParticleEnv;

/// Seed the tile-shatter working set. `FUN_801D081C`.
///
/// `allocated` is the answer to *both* `FUN_80017888` calls (`0x908` then
/// `0x5C00`); retail bails on either failing, and the second is not attempted
/// if the first fails.
///
/// `corner_offsets` is the four-entry table at overlay VA `0x801CE8BC`, copied
/// onto the stack at entry and used as `grid[col + row * 17 + offset[k]]` to
/// pick a tile's four corners out of the shared vertex grid. Its contents are
/// overlay data, not code, so they are not in the dump; a host reads them off
/// PROT 0979. The obvious `[0, 1, 0x11, 0x12]` is *not* asserted here.
///
/// PORT: FUN_801D081C
/// REF: FUN_80019B28 (heading), FUN_8005AF0C (sqrt), FUN_80056798 (rand)
///
/// NOT WIRED: `legaia_engine_core::World` models the transition as the phase
/// counter alone (`World::battle_intro`, driven by
/// `battle_intro_transition::tick_transition`); it carries no per-style working
/// set, and `legaia-engine-render` has no transition pass to draw 256 textured
/// quads into. Wiring needs both, plus the `0x801CE8BC` corner table off the
/// disc - seeding a grid nothing draws would be an inert allocation.
pub fn seed_tile_grid(
    sub_style: TileSubStyle,
    allocated: bool,
    corner_offsets: [i32; 4],
    env: &mut dyn ParticleEnv,
) -> TileSeedOutcome {
    if !allocated {
        return TileSeedOutcome::OutOfMemory;
    }

    // --- the 17 x 17 corner grid -------------------------------------------
    let mut vertices = vec![GridVertex::default(); GRID_DIM * GRID_DIM];
    for row in 0..GRID_DIM {
        for col in 0..GRID_DIM {
            let mut x = GRID_X_ORIGIN + col as i32 * GRID_X_STEP;
            let mut y = GRID_Y_ORIGIN + row as i32 * GRID_Y_STEP;
            // Interior vertices only, so the outline stays a clean rectangle.
            let interior = col != 0 && col < GRID_DIM - 1 && row != 0 && row < GRID_DIM - 1;
            if interior {
                x += (2 - env.rand() % 3) * JITTER_X_STEP;
                y += (2 - env.rand() % 3) * JITTER_Y_STEP;
            }
            vertices[col + row * GRID_DIM] = GridVertex {
                x: x as i16,
                y: y as i16,
                z: GRID_Z,
            };
        }
    }

    // --- the 16 x 16 tile records ------------------------------------------
    let mut tiles = vec![TileRecord::default(); TILE_DIM * TILE_DIM];
    for row in 0..TILE_DIM {
        let stored_y = TILE_STORED_Y_ORIGIN + row as i32 * TILE_Y_STEP;
        let pivot_y = TILE_PIVOT_Y_ORIGIN + row as i32 * TILE_Y_STEP;
        for col in 0..TILE_DIM {
            let x = TILE_X_ORIGIN + col as i32 * TILE_X_STEP;
            let right = col >= TILE_TPAGE_SPLIT_COL;
            let mut rec = TileRecord {
                progress: 0,
                tpage: if right {
                    TILE_TPAGE_RIGHT
                } else {
                    TILE_TPAGE_LEFT
                },
                rot: (x as i16, stored_y as i16, TILE_ROT_Z_SEED),
                delay: 0,
                trans: (0, 0, 0),
                ..Default::default()
            };
            let u_bias = if right { TILE_RIGHT_U_BIAS } else { 0 };

            for (k, &off) in corner_offsets.iter().enumerate() {
                let idx = (col as i32 + row as i32 * GRID_DIM as i32 + off) as usize;
                let v = vertices.get(idx).copied().unwrap_or_default();
                // The texel is the corner's grid position lifted back into
                // [0, ..] by the grid's own origin, then >> 4. Both biases are
                // pre-added before the shift, so the shift rounds toward zero.
                let u =
                    (shr4_toward_zero(i32::from(v.x) - GRID_X_ORIGIN) as i8).wrapping_sub(u_bias);
                let vv = shr4_toward_zero(i32::from(v.y) - GRID_Y_ORIGIN) as i8;
                let rel = (v.x.wrapping_sub(x as i16), v.y.wrapping_sub(pivot_y as i16));
                rec.front[k] = TileCorner {
                    x: rel.0,
                    y: rel.1,
                    z: TILE_FRONT_Z,
                    pad: 0,
                    uv: (u, vv),
                };
                rec.back[k] = TileCorner {
                    x: rel.0,
                    y: rel.1,
                    z: TILE_BACK_Z,
                    pad: 0,
                    uv: (u, vv),
                };
            }

            // Velocity pads. `+0x1A` / `+0x22` are written `sin >> 5` /
            // `cos >> 5` and immediately zeroed again; only the radial
            // sub-style writes them back, so the port skips the dead pair.
            let heading = env.heading(x, pivot_y);
            let (sin, cos) = (env.sin(heading), env.cos(heading));
            rec.back[0].pad = sin >> 6;
            rec.back[1].pad = cos >> 6;

            match sub_style {
                TileSubStyle::NegSpinRandomDelay => {
                    rec.front[2].pad = -0x60;
                    rec.delay = (env.rand() % 5000) as i16;
                }
                TileSubStyle::PosSpinRandomDelay => {
                    rec.front[2].pad = 0x60;
                    rec.delay = (env.rand() % 4000) as i16;
                }
                TileSubStyle::RadialDelayWithTumble => {
                    rec.front[0].pad = sin >> 5;
                    rec.front[1].pad = cos >> 5;
                    rec.front[2].pad = -0x20;
                    rec.delay = (env.sqrt(x * x + pivot_y * pivot_y) >> 5) as i16;
                }
                TileSubStyle::None => {}
            }

            tiles[col + row * TILE_DIM] = rec;
        }
    }

    TileSeedOutcome::Seeded(Box::new(TileGrid { vertices, tiles }))
}

/// `(v + bias) >> 4` with retail's toward-zero pre-bias (`addiu v0,v1,0x...f`
/// on the negative arm).
fn shr4_toward_zero(v: i32) -> i32 {
    if v < 0 { v + 0xF } else { v }.wrapping_shr(4)
}

/// What one [`step_tile`] call decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileStep {
    /// `+0x00` reached [`TILE_PROGRESS_LIMIT`]: the record is retired and this
    /// frame emits no primitive for it.
    Retired,
    /// The tile draws. `moved` reports whether the delay gate also let the
    /// integration run this frame.
    Drawn {
        /// `+0x0A < elapsed * `[`TILE_DELAY_SCALE`] - the tile has started.
        moved: bool,
    },
}

/// One tile, one frame. `FUN_801D0E54`.
///
/// Two independent gates, in retail's order:
///
/// 1. `+0x00 >= `[`TILE_PROGRESS_LIMIT`] retires the record - no packet, no
///    integration, and `+0x00` itself stops advancing.
/// 2. `+0x0A >= scaled_clock` holds the tile at its seeded pose. The packet is
///    still built, so an unstarted tile draws in place; only the integration
///    waits.
///
/// The integration itself:
///
/// | field | update |
/// |---|---|
/// | `+0x00` | `+= frame_step * `[`TILE_PROGRESS_STEP`] |
/// | `+0x04` / `+0x06` | `+= (spin * frame_step) >> 4` |
/// | `+0x08` | `+= spin_z * frame_step` (no shift) |
/// | `+0x0C` / `+0x0E` | `+= vel * frame_step` (no shift) |
/// | `+0x1A` / `+0x22` | `<<= 1` |
///
/// The last row is the interesting one: the x and y spin rates **double every
/// frame**, so a tumbling tile accelerates geometrically rather than spinning
/// at a constant rate. It is also why the zeroed pads of sub-styles `0` and `1`
/// stay zero forever.
///
/// PORT: FUN_801D0E54
///
/// NOT WIRED: called only by [`tick_tile_grid`], which is itself inert - see
/// the tag there.
pub fn step_tile(rec: &mut TileRecord, frame_step: u8, scaled_clock: i32) -> TileStep {
    if rec.progress >= TILE_PROGRESS_LIMIT {
        return TileStep::Retired;
    }
    let step = i32::from(frame_step);
    if i32::from(rec.delay) >= scaled_clock {
        return TileStep::Drawn { moved: false };
    }
    rec.progress = (rec.progress as u16).wrapping_add((step * TILE_PROGRESS_STEP) as u16) as i16;

    let spin_x = i32::from(rec.spin_x());
    let spin_y = i32::from(rec.spin_y());
    let spin_z = i32::from(rec.spin_z());
    let vel_x = i32::from(rec.vel_x());
    let vel_y = i32::from(rec.vel_y());

    rec.rot.0 = (rec.rot.0 as u16).wrapping_add(((spin_x * step) >> 4) as u16) as i16;
    rec.rot.1 = (rec.rot.1 as u16).wrapping_add(((spin_y * step) >> 4) as u16) as i16;
    rec.rot.2 = (rec.rot.2 as u16).wrapping_add((spin_z * step) as u16) as i16;
    rec.trans.0 = (rec.trans.0 as u16).wrapping_add((vel_x * step) as u16) as i16;
    rec.front[0].pad = ((rec.front[0].pad as u16) << 1) as i16;
    rec.front[1].pad = ((rec.front[1].pad as u16) << 1) as i16;
    rec.trans.1 = (rec.trans.1 as u16).wrapping_add((vel_y * step) as u16) as i16;

    TileStep::Drawn { moved: true }
}

/// What one [`tick_tile_grid`] frame reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TileTick {
    /// `_DAT_8007B6CC` - retail writes `elapsed != 0` here on entry. It is the
    /// "not the first frame of the transition" flag the shared draw setup
    /// reads.
    pub not_first_frame: bool,
    /// Tiles that still drew this frame.
    pub drawn: usize,
    /// Tiles whose delay had expired, so they also moved.
    pub moved: usize,
}

/// One frame of the tile-shatter style. `FUN_801D0D24`.
///
/// Walks all `0x100` records at `0x5C` apart, then advances the entity's
/// `+0x1A` clock by the frame step. The per-record delay gate is the entity
/// clock scaled by [`TILE_DELAY_SCALE`], computed **once** before the loop -
/// so every tile in a frame is measured against the same instant.
///
/// The per-record GPU work retail does around [`step_tile`] - `SetRotMatrix`
/// on the record's `+0x04` vector, `SetTransMatrix` on `+0x0C`, and the
/// `FUN_80043390` mesh submit over the eight corner vectors - is the
/// clean-room boundary and stays with the renderer.
///
/// PORT: FUN_801D0D24
///
/// NOT WIRED: same missing host as [`seed_tile_grid`] - nothing owns a
/// [`TileGrid`] and nothing draws one.
pub fn tick_tile_grid(grid: &mut TileGrid, elapsed: &mut i16, frame_step: u8) -> TileTick {
    let mut out = TileTick {
        not_first_frame: *elapsed != 0,
        ..Default::default()
    };
    let scaled_clock = i32::from(*elapsed) * TILE_DELAY_SCALE;
    for rec in grid.tiles.iter_mut() {
        match step_tile(rec, frame_step, scaled_clock) {
            TileStep::Retired => {}
            TileStep::Drawn { moved } => {
                out.drawn += 1;
                out.moved += usize::from(moved);
            }
        }
    }
    *elapsed = (*elapsed as u16).wrapping_add(u16::from(frame_step)) as i16;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestEnv {
        seq: i32,
    }

    impl ParticleEnv for TestEnv {
        fn heading(&mut self, x: i32, z: i32) -> i32 {
            (x + z) & 0xFFF
        }
        fn sin(&mut self, heading: i32) -> i16 {
            (heading as i16).wrapping_mul(2)
        }
        fn cos(&mut self, heading: i32) -> i16 {
            -(heading as i16)
        }
        fn sqrt(&mut self, v: i32) -> i32 {
            (v as f64).sqrt() as i32
        }
        fn rand(&mut self) -> i32 {
            self.seq += 1;
            self.seq * 7
        }
    }

    fn env() -> TestEnv {
        TestEnv { seq: 0 }
    }

    fn seeded_tile_grid(style: TileSubStyle) -> TileGrid {
        let TileSeedOutcome::Seeded(g) =
            seed_tile_grid(style, true, [0, 1, 0x11, 0x12], &mut env())
        else {
            panic!("expected a grid");
        };
        *g
    }

    #[test]
    fn the_allocation_sizes_fix_the_two_grid_shapes() {
        assert_eq!(GRID_BLOCK_BYTES, 0x908);
        assert_eq!(TILE_BLOCK_BYTES, 0x5C00);
        assert_eq!(TILE_BLOCK_BYTES / TILE_STRIDE, 0x100);
    }

    #[test]
    fn allocation_failure_seeds_nothing() {
        assert_eq!(
            seed_tile_grid(
                TileSubStyle::NegSpinRandomDelay,
                false,
                [0, 1, 0x11, 0x12],
                &mut env()
            ),
            TileSeedOutcome::OutOfMemory
        );
    }

    #[test]
    fn the_corner_vertices_are_a_clean_rectangle_and_the_interior_is_jittered() {
        let g = seeded_tile_grid(TileSubStyle::NegSpinRandomDelay);
        assert_eq!(g.vertices.len(), GRID_DIM * GRID_DIM);
        // Row 0 and the last row / column keep their exact lattice positions.
        for col in 0..GRID_DIM {
            assert_eq!(
                g.vertices[col].x as i32,
                GRID_X_ORIGIN + col as i32 * GRID_X_STEP
            );
            assert_eq!(g.vertices[col].y as i32, GRID_Y_ORIGIN);
            assert_eq!(g.vertices[col].z, GRID_Z);
        }
        // The lattice is symmetric about the origin.
        assert_eq!(g.vertices[0].x as i32, -0xA00);
        assert_eq!(g.vertices[GRID_DIM - 1].x as i32, 0xA00);
        // An interior vertex is displaced, and only in the positive direction
        // (the jitter is `(2 - rand % 3) * step`, never negative).
        let interior = g.vertices[GRID_DIM + 1];
        let lattice_x = GRID_X_ORIGIN + GRID_X_STEP;
        assert!(i32::from(interior.x) >= lattice_x);
        assert!(i32::from(interior.x) <= lattice_x + 2 * JITTER_X_STEP);
    }

    #[test]
    fn the_texture_page_splits_at_column_nine() {
        let g = seeded_tile_grid(TileSubStyle::NegSpinRandomDelay);
        assert_eq!(g.tiles[8].tpage, TILE_TPAGE_LEFT);
        assert_eq!(g.tiles[9].tpage, TILE_TPAGE_RIGHT);
        // The right-hand page shifts u by 0x80 (subtracting a -0x80 bias).
        let left_u = g.tiles[8].front[0].uv.0;
        let right_u = g.tiles[9].front[0].uv.0;
        assert_eq!(right_u, ((left_u as i32 + 0x140 / 16 + 0x80) as i8));
    }

    #[test]
    fn the_stored_origin_sits_a0_above_the_corner_pivot() {
        assert_eq!(TILE_STORED_Y_ORIGIN - TILE_PIVOT_Y_ORIGIN, 0xA0);
        let g = seeded_tile_grid(TileSubStyle::NegSpinRandomDelay);
        // Tile (0,0): rot.y is the stored origin, and its corner 0 is relative
        // to the pivot, which is 0xA0 lower.
        assert_eq!(g.tiles[0].rot.1 as i32, TILE_STORED_Y_ORIGIN);
        assert_eq!(g.tiles[0].rot.2, TILE_ROT_Z_SEED);
        assert_eq!(
            g.tiles[0].front[0].y as i32,
            GRID_Y_ORIGIN - TILE_PIVOT_Y_ORIGIN
        );
    }

    #[test]
    fn front_and_back_faces_differ_only_in_z() {
        let g = seeded_tile_grid(TileSubStyle::RadialDelayWithTumble);
        for k in 0..4 {
            assert_eq!(g.tiles[5].front[k].x, g.tiles[5].back[k].x);
            assert_eq!(g.tiles[5].front[k].y, g.tiles[5].back[k].y);
            assert_eq!(g.tiles[5].front[k].uv, g.tiles[5].back[k].uv);
        }
        assert_eq!(g.tiles[5].front[0].z, TILE_FRONT_Z);
        assert_eq!(g.tiles[5].back[0].z, TILE_BACK_Z);
    }

    #[test]
    fn only_the_radial_sub_style_leaves_a_tumble_rate() {
        for style in [
            TileSubStyle::NegSpinRandomDelay,
            TileSubStyle::PosSpinRandomDelay,
        ] {
            let g = seeded_tile_grid(style);
            assert!(
                g.tiles.iter().all(|t| t.spin_x() == 0 && t.spin_y() == 0),
                "the dead stores at 801d0bac/801d0bb0 win"
            );
            assert!(g.tiles.iter().all(|t| t.spin_z() != 0));
        }
        let g = seeded_tile_grid(TileSubStyle::RadialDelayWithTumble);
        assert!(g.tiles.iter().any(|t| t.spin_x() != 0 || t.spin_y() != 0));
        assert!(g.tiles.iter().all(|t| t.spin_z() == -0x20));
    }

    #[test]
    fn the_unhandled_sub_style_leaves_the_spin_and_delay_alone() {
        let g = seeded_tile_grid(TileSubStyle::None);
        assert!(g.tiles.iter().all(|t| t.spin_z() == 0 && t.delay == 0));
        // The linear velocities are written before the sub-style switch, so
        // they survive.
        assert!(g.tiles.iter().any(|t| t.vel_x() != 0 || t.vel_y() != 0));
    }

    #[test]
    fn the_delay_gate_holds_a_tile_in_place() {
        let mut rec = TileRecord {
            delay: 100,
            ..Default::default()
        };
        rec.front[2].pad = 0x60;
        assert_eq!(step_tile(&mut rec, 1, 0), TileStep::Drawn { moved: false });
        assert_eq!(rec.progress, 0, "the progress counter waits too");
        assert_eq!(rec.rot.2, 0);

        assert_eq!(
            step_tile(&mut rec, 1, 6000),
            TileStep::Drawn { moved: true }
        );
        assert_eq!(rec.progress, TILE_PROGRESS_STEP as i16);
        assert_eq!(rec.rot.2, 0x60);
    }

    #[test]
    fn a_retired_tile_stops_entirely() {
        let mut rec = TileRecord {
            progress: TILE_PROGRESS_LIMIT,
            ..Default::default()
        };
        assert_eq!(step_tile(&mut rec, 1, i32::MAX), TileStep::Retired);
        assert_eq!(rec.progress, TILE_PROGRESS_LIMIT);
    }

    #[test]
    fn the_tumble_rates_double_every_frame() {
        let mut rec = TileRecord::default();
        rec.front[0].pad = 3;
        rec.front[1].pad = -5;
        for _ in 0..3 {
            step_tile(&mut rec, 1, i32::MAX);
        }
        assert_eq!(rec.spin_x(), 24);
        assert_eq!(rec.spin_y(), -40);
        // A zero rate stays zero no matter how long it doubles - which is what
        // makes sub-styles 0 and 1 pure z-spinners.
        let mut flat = TileRecord::default();
        for _ in 0..64 {
            step_tile(&mut flat, 1, i32::MAX);
        }
        assert_eq!((flat.spin_x(), flat.spin_y()), (0, 0));
    }

    #[test]
    fn the_tick_measures_every_tile_against_one_instant_and_then_advances() {
        let mut g = seeded_tile_grid(TileSubStyle::RadialDelayWithTumble);
        // Force a spread of delays so the gate is observable.
        for (i, t) in g.tiles.iter_mut().enumerate() {
            t.delay = (i as i16) * 4;
        }
        let mut elapsed: i16 = 5;
        let tick = tick_tile_grid(&mut g, &mut elapsed, 2);
        assert!(tick.not_first_frame);
        assert_eq!(tick.drawn, TILE_DIM * TILE_DIM);
        // 5 * 0x3C == 300, so delays 0..299 (indices 0..=74) moved.
        assert_eq!(tick.moved, 75);
        assert_eq!(elapsed, 7);
    }

    #[test]
    fn the_first_frame_reports_the_first_frame_flag_clear() {
        let mut g = seeded_tile_grid(TileSubStyle::NegSpinRandomDelay);
        let mut elapsed: i16 = 0;
        assert!(!tick_tile_grid(&mut g, &mut elapsed, 1).not_first_frame);
        assert!(tick_tile_grid(&mut g, &mut elapsed, 1).not_first_frame);
    }
}
