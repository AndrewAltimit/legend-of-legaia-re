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

/// The `+0x08` word every tile record is seeded with (`li v0,0x880`) - the z
/// component of the tile's world **position**.
///
/// `0x880` is [`GRID_Z`] plus `0x80`, which is exactly the front face's local
/// z offset - so the seeded position puts each tile's front face flat on the
/// grid plane. That arithmetic only works if `+0x04..+0x08` is a position,
/// which is what pins the reading.
pub const TILE_POS_Z_SEED: i16 = 0x880;

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
    /// [`TILE_POS_Z_SEED`], which is what gives every tile a different resting
    /// orientation before it starts to spin.
    pub pos: (i16, i16, i16),
    /// `+0x0A` - spawn delay, held against `elapsed * `[`TILE_DELAY_SCALE`].
    pub delay: i16,
    /// `+0x0C` / `+0x0E` / `+0x10` - the tile's **Euler angle triple**, seeded
    /// to zero.
    ///
    /// REF: FUN_80026988 - the `RotMatrix` wrapper the angles feed.
    ///
    /// `FUN_801D0D24` hands this to `RotMatrix` (`FUN_80026988`, which masks
    /// each angle `& 0xFFF`), and the matrix it builds becomes the per-tile
    /// **rotation**. `+0x10` is never integrated, so a tile only ever tumbles
    /// about x and y.
    pub angles: (i16, i16, i16),
    /// `+0x14..+0x33` - the four front corners.
    pub front: [TileCorner; 4],
    /// `+0x34..+0x53` - the four back corners.
    pub back: [TileCorner; 4],
}

impl TileRecord {
    /// Linear velocity along x - corner `front[0]`'s pad (`+0x1A`).
    pub fn vel_x(&self) -> i16 {
        self.front[0].pad
    }
    /// Linear velocity along y - corner `front[1]`'s pad (`+0x22`).
    pub fn vel_y(&self) -> i16 {
        self.front[1].pad
    }
    /// Linear velocity along z - corner `front[2]`'s pad (`+0x2A`).
    pub fn vel_z(&self) -> i16 {
        self.front[2].pad
    }
    /// Angular rate about x - corner `back[0]`'s pad (`+0x3A`).
    pub fn spin_x(&self) -> i16 {
        self.back[0].pad
    }
    /// Angular rate about y - corner `back[1]`'s pad (`+0x42`).
    pub fn spin_y(&self) -> i16 {
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
/// WIRED. `legaia_engine_render::battle_intro::BattleIntro` owns the
/// [`TileGrid`] between frames, ticks it from the live transition clock, and
/// **draws it**: [`tile_face_quads`] resolves each record's ten-primitive
/// packet and the render side projects it through the ported FT4-handler
/// chain (`battle_intro::emit_tile`). [`select_intro_style`] makes this the
/// style the **ordinary random encounter** takes - so this path runs on most
/// battles. The native host decodes the `0x801CE8BC` corner table off PROT
/// 0979 (`battle_intro::parse_tile_corner_table`), with the pinned
/// `[0, 1, 17, 18]` as the disc-free fallback.
///
/// [`select_intro_style`]: crate::battle_intro_styles::select_intro_style
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
                pos: (x as i16, stored_y as i16, TILE_POS_Z_SEED),
                delay: 0,
                angles: (0, 0, 0),
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

// ---------------------------------------------------------------------------
// The emitter's ten-primitive packet (`FUN_801D0E54`'s descriptor build)
// ---------------------------------------------------------------------------

/// Texture-page word of the four semi-transparent **shade** faces
/// (`lui a1,0x27; ori a1,a1,0x40` - the `0x0027` high half of the prim's
/// `+0x08` word): 4bpp page at VRAM `(448, 0)`, ABR bits `01` = additive.
///
/// The page is the top-left 64x64 texel corner of the **field-character
/// texture pack's entry 0** (`legaia_asset::field_char_textures`, PROT 0874
/// section 2) - a 256x256 4bpp TIM whose declared destination is `(448, 0)`,
/// resident for the whole field session. Pinned by mid-transition capture:
/// the VRAM rect is byte-identical to that TIM before the encounter, on the
/// first shatter frame and on the 24th, across two different scenes.
pub const SHADE_TPAGE: u16 = 0x0027;

/// CLUT word of the shade faces (`lui a2,0x7641`): `(x, y) = (16, 473)` -
/// CLUT index 1 of the same pack entry's 16-CLUT block, which the field
/// uploader lands as a 256x1 strip on row 473. A 16-entry black-to-bright
/// ramp (dark half black, bright half a slightly blue-tinted grey ladder),
/// every entry STP-set in the TIM itself.
pub const SHADE_CLUT: u16 = 0x7641;

/// The shade faces' four UV pairs (`0x76410000` / `0x270040` / `0x40404000`
/// unpack to `uv0..uv3`): the full 64x64 corner of the shade page, stretched
/// across each side face.
pub const SHADE_UVS: [(u8, u8); 4] = [(0, 0), (0x40, 0), (0, 0x40), (0x40, 0x40)];

/// One row of the emitter's ten-primitive face table.
///
/// `corners` index the record's 8-vector array (0..3 = front corners,
/// 4..7 = back corners; the packet stores them as byte offsets two per word
/// at prim `+0x10`/`+0x14`, low half first). `grey` is the flat colour byte
/// replicated across RGB; `semi_transparent` is bit 1 of the GP0 code
/// (`0x2E` vs `0x2C`); `shade` selects the fixed shade-page UV/tpage/CLUT
/// over the record's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileFace {
    /// Corner indices in `v0..v3` order.
    pub corners: [usize; 4],
    /// GP0 code bit 1 - `0x2E` (set) vs `0x2C` (clear).
    pub semi_transparent: bool,
    /// Flat colour byte (RGB all equal).
    pub grey: u8,
    /// `true` = shade-page UV set, `false` = record UV set.
    pub shade: bool,
}

/// The ten primitives `FUN_801D0E54` assembles, in packet order. Six box
/// faces become ten because the four sides are emitted twice: once
/// semi-transparent (additive) over the shade page, once opaque with the
/// tile's own captured-frame UVs. Packet order matters: the dispatcher links
/// the prims into their OT buckets in this order, and `AddPrim` prepends -
/// so within one bucket the *earlier* packet entry draws later, which is
/// what lands the shade set on top of its opaque siblings.
///
/// Read off the store sequence at `0x801D0F60..0x801D10A8` (colour words
/// `0x2E606060` / `0x2E404040` / `0x2E303030` / `0x2E202020` /
/// `0x2C808080` / `0x2C606060` x4 / `0x2C202020`; corner-pair words as
/// tabulated in `docs/subsystems/cutscene.md`).
pub const TILE_FACES: [TileFace; 10] = [
    // 0..3 - the four sides, semi-transparent over the shade page.
    TileFace {
        corners: [1, 5, 3, 7],
        semi_transparent: true,
        grey: 0x60,
        shade: true,
    },
    TileFace {
        corners: [4, 0, 6, 2],
        semi_transparent: true,
        grey: 0x40,
        shade: true,
    },
    TileFace {
        corners: [4, 5, 0, 1],
        semi_transparent: true,
        grey: 0x30,
        shade: true,
    },
    TileFace {
        corners: [2, 3, 6, 7],
        semi_transparent: true,
        grey: 0x20,
        shade: true,
    },
    // 4 - the front face, opaque, full brightness.
    TileFace {
        corners: [0, 1, 2, 3],
        semi_transparent: false,
        grey: 0x80,
        shade: false,
    },
    // 5..8 - the four sides again, opaque with the record's UVs.
    TileFace {
        corners: [1, 5, 3, 7],
        semi_transparent: false,
        grey: 0x60,
        shade: false,
    },
    TileFace {
        corners: [4, 0, 6, 2],
        semi_transparent: false,
        grey: 0x60,
        shade: false,
    },
    TileFace {
        corners: [4, 5, 0, 1],
        semi_transparent: false,
        grey: 0x60,
        shade: false,
    },
    TileFace {
        corners: [2, 3, 6, 7],
        semi_transparent: false,
        grey: 0x60,
        shade: false,
    },
    // 9 - the back face, opaque, darkest.
    TileFace {
        corners: [6, 7, 4, 5],
        semi_transparent: false,
        grey: 0x20,
        shade: false,
    },
];

/// One face of one tile, resolved against a record: local corner positions
/// (the tile's own `SVECTOR`s - world placement comes from the record's
/// `pos`/`angles` at projection time), the four UV pairs, and the texture
/// words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileFaceQuad {
    /// Local corner positions in `v0..v3` order.
    pub corners: [(i16, i16, i16); 4],
    /// UV pairs in `v0..v3` order, as raw texel bytes.
    pub uv: [(u8, u8); 4],
    /// GP0 texpage word.
    pub tpage: u16,
    /// GP0 CLUT word (`0` for the record-UV faces, matching the packet).
    pub clut: u16,
    /// Flat colour byte.
    pub grey: u8,
    /// GP0 code bit 1.
    pub semi_transparent: bool,
}

/// Resolve the ten-face packet for one record. `FUN_801D0E54`'s descriptor
/// build, minus the GTE work its dispatcher does afterwards: corner `k`
/// reads `front[k]` / `back[k - 4]`, every record-UV face shares the
/// record's four stored UV pairs (`+0x54..+0x5B` - `front[j].uv` here), and
/// the shade faces take the three literals above.
///
/// Returns `None` for a retired record (`+0x00 >= `[`TILE_PROGRESS_LIMIT`],
/// the same gate [`step_tile`] applies) - retail's emitter builds no packet
/// for those.
pub fn tile_face_quads(rec: &TileRecord) -> Option<[TileFaceQuad; 10]> {
    if rec.progress >= TILE_PROGRESS_LIMIT {
        return None;
    }
    let corner = |i: usize| -> (i16, i16, i16) {
        let c = if i < 4 { rec.front[i] } else { rec.back[i - 4] };
        (c.x, c.y, c.z)
    };
    let record_uv: [(u8, u8); 4] =
        std::array::from_fn(|j| (rec.front[j].uv.0 as u8, rec.front[j].uv.1 as u8));
    Some(std::array::from_fn(|f| {
        let face = &TILE_FACES[f];
        TileFaceQuad {
            corners: std::array::from_fn(|j| corner(face.corners[j])),
            uv: if face.shade { SHADE_UVS } else { record_uv },
            tpage: if face.shade {
                SHADE_TPAGE
            } else {
                rec.tpage as u16
            },
            clut: if face.shade { SHADE_CLUT } else { 0 },
            grey: face.grey,
            semi_transparent: face.semi_transparent,
        }
    }))
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
/// | `+0x08` | `+= vel_z * frame_step` (no shift) |
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
/// WIRED. [`tick_tile_grid`] calls this for every record on every frame of
/// the shatter transition the native window runs, and the packet half of the
/// same retail function is [`tile_face_quads`] + the render-side projection
/// ([`tick_tile_grid_emit`]'s hook) - so both the integration and the draw
/// are live.
pub fn step_tile(rec: &mut TileRecord, frame_step: u8, scaled_clock: i32) -> TileStep {
    if rec.progress >= TILE_PROGRESS_LIMIT {
        return TileStep::Retired;
    }
    let step = i32::from(frame_step);
    if i32::from(rec.delay) >= scaled_clock {
        return TileStep::Drawn { moved: false };
    }
    rec.progress = (rec.progress as u16).wrapping_add((step * TILE_PROGRESS_STEP) as u16) as i16;

    let vel_x = i32::from(rec.vel_x());
    let vel_y = i32::from(rec.vel_y());
    let vel_z = i32::from(rec.vel_z());
    let spin_x = i32::from(rec.spin_x());
    let spin_y = i32::from(rec.spin_y());

    rec.pos.0 = (rec.pos.0 as u16).wrapping_add(((vel_x * step) >> 4) as u16) as i16;
    rec.pos.1 = (rec.pos.1 as u16).wrapping_add(((vel_y * step) >> 4) as u16) as i16;
    rec.pos.2 = (rec.pos.2 as u16).wrapping_add((vel_z * step) as u16) as i16;
    rec.angles.0 = (rec.angles.0 as u16).wrapping_add((spin_x * step) as u16) as i16;
    rec.front[0].pad = ((rec.front[0].pad as u16) << 1) as i16;
    rec.front[1].pad = ((rec.front[1].pad as u16) << 1) as i16;
    rec.angles.1 = (rec.angles.1 as u16).wrapping_add((spin_y * step) as u16) as i16;

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
/// The per-record GPU work retail does around [`step_tile`] is the transform
/// staging plus the submit: load the view matrix from `0x1F8003C8`, push the
/// record's `+0x04` **position** through `MVMVA` into the matrix translation,
/// build the rotation from the `+0x0C` **angles** with `RotMatrix`, load that,
/// then hand the eight corner vectors to `FUN_80043390`. `FUN_801D0E54`
/// itself contains no coprocessor instructions - it assembles a synthetic
/// 10-primitive TMD object and delegates.
///
/// REF: FUN_80043390 - the object dispatcher the tile emitter delegates to.
///
/// PORT: FUN_801D0D24
///
/// WIRED - the native window's transition emitter drives the
/// [`tick_tile_grid_emit`] form with a projecting hook, so the walk both
/// integrates and draws. During the transition the `0x1F8003C8` view matrix
/// is identity rotation with zero translation (pinned live from the second
/// frame on; frame one still holds the field camera's value and every tile
/// projects behind the near plane, so retail's first frame draws no tiles).
pub fn tick_tile_grid(grid: &mut TileGrid, elapsed: &mut i16, frame_step: u8) -> TileTick {
    tick_tile_grid_emit(grid, elapsed, frame_step, |_| {})
}

/// [`tick_tile_grid`] with a per-record emit hook, called with each record's
/// **pre-integration** state - the same ordering retail has, where
/// `FUN_801D0E54` builds and dispatches the packet from the record as loaded
/// and only then integrates it. The hook receives every record (including
/// retired ones); [`tile_face_quads`] applies the retire gate itself, so a
/// consumer that goes through it reproduces the emitter's skip.
pub fn tick_tile_grid_emit(
    grid: &mut TileGrid,
    elapsed: &mut i16,
    frame_step: u8,
    mut emit: impl FnMut(&TileRecord),
) -> TileTick {
    let mut out = TileTick {
        not_first_frame: *elapsed != 0,
        ..Default::default()
    };
    let scaled_clock = i32::from(*elapsed) * TILE_DELAY_SCALE;
    for rec in grid.tiles.iter_mut() {
        emit(rec);
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
        assert_eq!(g.tiles[0].pos.1 as i32, TILE_STORED_Y_ORIGIN);
        assert_eq!(g.tiles[0].pos.2, TILE_POS_Z_SEED);
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
                g.tiles.iter().all(|t| t.vel_x() == 0 && t.vel_y() == 0),
                "the dead stores at 801d0bac/801d0bb0 win"
            );
            assert!(g.tiles.iter().all(|t| t.vel_z() != 0));
        }
        let g = seeded_tile_grid(TileSubStyle::RadialDelayWithTumble);
        assert!(g.tiles.iter().any(|t| t.vel_x() != 0 || t.vel_y() != 0));
        assert!(g.tiles.iter().all(|t| t.vel_z() == -0x20));
    }

    #[test]
    fn the_unhandled_sub_style_leaves_the_spin_and_delay_alone() {
        let g = seeded_tile_grid(TileSubStyle::None);
        assert!(g.tiles.iter().all(|t| t.vel_z() == 0 && t.delay == 0));
        // The linear velocities are written before the sub-style switch, so
        // they survive.
        assert!(g.tiles.iter().any(|t| t.spin_x() != 0 || t.spin_y() != 0));
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
        assert_eq!(rec.pos.2, 0);

        assert_eq!(
            step_tile(&mut rec, 1, 6000),
            TileStep::Drawn { moved: true }
        );
        assert_eq!(rec.progress, TILE_PROGRESS_STEP as i16);
        assert_eq!(rec.pos.2, 0x60);
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
        assert_eq!(rec.vel_x(), 24);
        assert_eq!(rec.vel_y(), -40);
        // A zero rate stays zero no matter how long it doubles - which is what
        // makes sub-styles 0 and 1 pure z-spinners.
        let mut flat = TileRecord::default();
        for _ in 0..64 {
            step_tile(&mut flat, 1, i32::MAX);
        }
        assert_eq!((flat.vel_x(), flat.vel_y()), (0, 0));
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

#[cfg(test)]
mod face_table {
    use super::*;

    /// The documented ten-primitive table: shade sides first (code `0x2E`,
    /// descending grey), then front / opaque sides / back.
    #[test]
    fn the_packet_shape_matches_the_emitter() {
        assert_eq!(TILE_FACES.len(), 10);
        // 0..3: shade set - semi-transparent, shade page.
        for f in &TILE_FACES[0..4] {
            assert!(f.semi_transparent && f.shade);
        }
        assert_eq!(
            [0, 1, 2, 3].map(|i| TILE_FACES[i].grey),
            [0x60, 0x40, 0x30, 0x20]
        );
        // 4..9: record set - opaque, record UVs.
        for f in &TILE_FACES[4..10] {
            assert!(!f.semi_transparent && !f.shade);
        }
        assert_eq!(TILE_FACES[4].corners, [0, 1, 2, 3], "front");
        assert_eq!(TILE_FACES[9].corners, [6, 7, 4, 5], "back");
        assert_eq!(TILE_FACES[4].grey, 0x80);
        assert_eq!(TILE_FACES[9].grey, 0x20);
        // The opaque sides repeat the shade sides' corner rows.
        for k in 0..4 {
            assert_eq!(TILE_FACES[5 + k].corners, TILE_FACES[k].corners);
            assert_eq!(TILE_FACES[5 + k].grey, 0x60);
        }
    }

    /// Every corner index appears in the table, and each face mixes front
    /// (0..3) and back (4..7) only on the sides.
    #[test]
    fn side_faces_span_front_and_back() {
        for face in &TILE_FACES[0..4] {
            let c = face.corners;
            assert!(c.iter().any(|&i| i < 4) && c.iter().any(|&i| i >= 4));
        }
        assert!(TILE_FACES[4].corners.iter().all(|&i| i < 4));
        assert!(TILE_FACES[9].corners.iter().all(|&i| i >= 4));
    }

    #[test]
    fn resolved_quads_carry_the_three_shade_literals() {
        let mut rec = TileRecord {
            tpage: TILE_TPAGE_LEFT,
            ..Default::default()
        };
        for (k, c) in rec.front.iter_mut().enumerate() {
            c.x = k as i16;
            c.y = 10 + k as i16;
            c.z = TILE_FRONT_Z;
            c.uv = (k as i8 * 2, k as i8 * 3);
        }
        for (k, c) in rec.back.iter_mut().enumerate() {
            c.x = k as i16;
            c.y = 10 + k as i16;
            c.z = TILE_BACK_Z;
            c.uv = (k as i8 * 2, k as i8 * 3);
        }
        let quads = tile_face_quads(&rec).expect("not retired");
        // Shade face 0 = corners (1,5,3,7): front1, back1, front3, back3.
        let q0 = &quads[0];
        assert_eq!(q0.tpage, SHADE_TPAGE);
        assert_eq!(q0.clut, SHADE_CLUT);
        assert_eq!(q0.uv, SHADE_UVS);
        assert_eq!(q0.corners[0], (1, 11, TILE_FRONT_Z));
        assert_eq!(q0.corners[1], (1, 11, TILE_BACK_Z));
        assert_eq!(q0.corners[2], (3, 13, TILE_FRONT_Z));
        assert_eq!(q0.corners[3], (3, 13, TILE_BACK_Z));
        // Record face 5 shares q0's corners but takes the record's UVs,
        // tpage, and CLUT 0 - the four stored pairs in v0..v3 order,
        // independent of which corners the face uses.
        let q5 = &quads[5];
        assert_eq!(q5.corners, q0.corners);
        assert_eq!(q5.tpage, TILE_TPAGE_LEFT as u16);
        assert_eq!(q5.clut, 0);
        assert_eq!(q5.uv, [(0, 0), (2, 3), (4, 6), (6, 9)]);
    }

    #[test]
    fn a_retired_record_builds_no_packet() {
        let rec = TileRecord {
            progress: TILE_PROGRESS_LIMIT,
            ..Default::default()
        };
        assert_eq!(tile_face_quads(&rec), None);
    }

    /// The emit hook sees the record before the integration moves it.
    #[test]
    fn the_emit_hook_runs_on_the_pre_step_state() {
        let mut rec = TileRecord::default();
        rec.front[2].pad = 0x60; // pos-z rate
        let mut grid = TileGrid {
            vertices: Vec::new(),
            tiles: vec![rec],
        };
        let mut elapsed: i16 = 1; // past the delay gate at scale 0x3C
        let mut seen_z = None;
        tick_tile_grid_emit(&mut grid, &mut elapsed, 1, |r| {
            seen_z = Some(r.pos.2);
        });
        assert_eq!(seen_z, Some(0), "hook sees the pre-integration position");
        assert_eq!(grid.tiles[0].pos.2, 0x60, "the step still ran after it");
    }
}

#[cfg(test)]
mod record_semantics {
    use super::*;

    /// The seeded `+0x08` word puts each tile's FRONT face exactly on the
    /// grid plane. That only balances if `+0x04..+0x08` is a world position,
    /// which is the arithmetic pinning the field's meaning independently of
    /// the call order in `FUN_801D0D24`.
    #[test]
    fn the_seeded_position_lands_the_front_face_on_the_grid_plane() {
        assert_eq!(TILE_POS_Z_SEED, GRID_Z - TILE_FRONT_Z);
        assert_eq!(TILE_POS_Z_SEED + TILE_FRONT_Z, GRID_Z);
        // ...and the back face sits one full box depth behind it.
        assert_eq!(
            TILE_POS_Z_SEED + TILE_BACK_Z - (TILE_POS_Z_SEED + TILE_FRONT_Z),
            TILE_BACK_Z - TILE_FRONT_Z
        );
    }

    /// The velocity accessors read the FRONT corner pads and the angular
    /// accessors the BACK ones. Swapping them is the specific latent trap
    /// this naming closes: the offsets are identical either way, so nothing
    /// misbehaves until an emitter consumes the semantics.
    #[test]
    fn linear_rates_come_from_the_front_pads_and_angular_from_the_back() {
        let mut rec = TileRecord::default();
        rec.front[0].pad = 11;
        rec.front[1].pad = 22;
        rec.front[2].pad = 33;
        rec.back[0].pad = 44;
        rec.back[1].pad = 55;
        assert_eq!((rec.vel_x(), rec.vel_y(), rec.vel_z()), (11, 22, 33));
        assert_eq!((rec.spin_x(), rec.spin_y()), (44, 55));
    }

    /// The integration moves the POSITION by the linear rates and the ANGLES
    /// by the angular ones - and only the linear x/y pads double.
    #[test]
    fn the_step_moves_position_by_linear_and_angles_by_angular() {
        let mut rec = TileRecord {
            pos: (0, 0, 0),
            angles: (0, 0, 0),
            ..Default::default()
        };
        rec.front[0].pad = 0x100; // linear x
        rec.front[2].pad = 7; // linear z
        rec.back[0].pad = 9; // angular x
        let before = (rec.front[0].pad, rec.back[0].pad);
        assert_eq!(step_tile(&mut rec, 1, 1), TileStep::Drawn { moved: true });
        assert_eq!(rec.pos.0, 0x10, "position x moves by the linear rate >> 4");
        assert_eq!(
            rec.pos.2, 7,
            "position z moves by the linear rate, unshifted"
        );
        assert_eq!(rec.angles.0, 9, "angles x moves by the angular rate");
        assert_eq!(rec.front[0].pad, before.0 << 1, "linear x pad doubles");
        assert_eq!(rec.back[0].pad, before.1, "the angular pad does not");
    }
}
