//! The slot-A minigame overlays' **scene-floor kernels**: the ground-height
//! solver every floor actor is settled onto, the 16-entry height ramp the
//! solver indexes, and the per-frame floor pass that spawns one tile actor per
//! drawn grid cell.
//!
//! These are *not* dance- or fishing-specific. `FUN_801D6028` and
//! `FUN_801D6BBC` are byte-identical in the fishing, slot-machine and
//! debug-menu overlay images (only the `[overlay_*.bin]` header line of the
//! dump differs), so they are shared library code in the overlay band above
//! `0x801D0018`; `FUN_801D3A2C` is the dance overlay's private copy of the
//! same floor pass, differing only in which overlay-local globals it writes.
//!
//! ## The scene floor buffer
//!
//! All of them read one buffer, the per-scene block whose pointer lives at
//! `_DAT_1F8003EC` (the scratchpad word the field subsystem installs at scene
//! load). Three regions matter:
//!
//! | Offset | Shape | What it is |
//! |---|---|---|
//! | `+0x0000` | `0x20`-byte records, indexed by tile id | [`TileRecord`] - the per-tile placement + flags |
//! | `+0x4000` | `u8`, row pitch `0x80` | [`FloorGrid::height_index`] (low nibble) + the wall nibble |
//! | `+0x8000` | `u16`, row pitch `0x100` | [`FloorGrid::cell`] - tile id in bits `0..8`, flags above |
//!
//! The `+0x4000` byte is **two fields in one**. `FUN_801D6028` /
//! `FUN_801D3A2C` / `FUN_801D2A10` take its **low** nibble as an index into
//! the 16-entry height ramp; the field-locomotion collision probe
//! (`FUN_801CFE4C`) takes its **high** nibble as the four sub-cell wall bits
//! (`>> 4 & quadrant_mask`). Reading the byte whole - or masking the wrong
//! nibble - conflates the terrain height with the walkability.
//!
//! ## The two grid resolutions
//!
//! A floor cell is 128 world units. An actor's `+0x14` / `+0x18` world XZ pair
//! is first reduced to a **half-cell** index (`>> 6`, so 64-unit steps); the
//! grid index is that halved again toward zero, and the half-cell's low bit
//! selects which quadrant of the cell the actor stands in. The low seven bits
//! of the raw coordinate are the sub-cell fraction the bilinear blend
//! interpolates over, which is why the fraction runs `0 ..= 0x7F` while the
//! coarse step is `>> 6`.
//!
//! See [`docs/subsystems/minigame-fishing.md`](../../../docs/subsystems/minigame-fishing.md)
//! and [`docs/subsystems/minigame-dance.md`](../../../docs/subsystems/minigame-dance.md);
//! dumps `overlay_fishing_801d6028.txt`, `overlay_dance_801d3a2c.txt`,
//! `overlay_dance_801d2a10.txt`, `overlay_dance_801d6bbc.txt`.

/// Byte offset of the height / wall nibble grid inside the scene floor buffer.
pub const HEIGHT_GRID_OFF: usize = 0x4000;
/// Row pitch, in bytes, of the height / wall nibble grid.
pub const HEIGHT_GRID_PITCH: usize = 0x80;
/// Byte offset of the `u16` cell grid inside the scene floor buffer.
pub const CELL_GRID_OFF: usize = 0x8000;
/// Row pitch, in bytes, of the `u16` cell grid.
pub const CELL_GRID_PITCH: usize = 0x100;
/// Bytes per tile record at the head of the scene floor buffer.
pub const TILE_RECORD_STRIDE: usize = 0x20;
/// Mask that extracts the tile id out of a cell word.
pub const CELL_TILE_ID_MASK: u16 = 0x1FF;
/// Cell bit that marks a cell as carrying a step-layer patch, switching the
/// height solver from the bilinear blend to the layer lookup.
pub const CELL_STEP_LAYER: u16 = 0x800;
/// Cell bit that suppresses the neighbour cell a tile record points at.
pub const CELL_NEIGHBOUR_BLOCK: u16 = 0x400;
/// Cell bit whose *absence* raises the actor's `0x800000` flag.
pub const CELL_ON_FLOOR: u16 = 0x1000;
/// Actor flag word bit the height solver maintains (`actor + 0x10`).
pub const ACTOR_FLAG_OFF_FLOOR: u32 = 0x0080_0000;
/// Tile-record flag that admits the cell to the floor pass (`rec + 0x12`).
pub const TILE_FLAG_DRAWN: u16 = 0x4;
/// Tile-record flag that selects the alternate draw mode (`rec + 0x12`).
pub const TILE_FLAG_ALT_MODE: u16 = 0x2;
/// Tile-record flag that sets the spawned actor's `+0x74` bit `0x10000000`.
pub const TILE_FLAG_ACTOR_74: u16 = 0x800;
/// Tile-record flag that sets the spawned actor's `+0x10` bit `0x4`.
pub const TILE_FLAG_ACTOR_10: u16 = 0x1000;
/// Grid extent both floor passes bound their neighbour probe against.
pub const GRID_EXTENT: i32 = 0x80;
/// World units per floor cell.
pub const CELL_WORLD_UNITS: i32 = 0x80;

/// Height step per ramp entry (`FUN_801D2A10`: `0x1E0` down to `0` in
/// sixteen `-0x20` steps).
pub const HEIGHT_RAMP_STEP: i16 = 0x20;
/// Entries in the height ramp - one per value the `+0x4000` low nibble takes.
pub const HEIGHT_RAMP_LEN: usize = 16;

// PORT: FUN_801d2a10 (the scratchpad height-ramp install, `0x1F80035C`)
// NOT WIRED: the ramp is the scratchpad table `DAT_1F80035C` the *retail*
// solver indexes; the port's [`ground_height`] takes the ramp as a slice so a
// caller can pass the scene's own table when a scene carries one. Nothing in
// the engine models the PSX scratchpad, so no host installs it - this is the
// value the dance overlay installs, kept so a floor host can start from it.
/// The 16-entry height ramp `FUN_801D2A10` writes into scratchpad before it
/// walks the floor rect: `ramp[i] = i * 0x20`.
///
/// Retail builds it backwards (`0x1E0` at `0x1F80037A`, stepping `-0x20` and
/// `-2` down to `0` at `0x1F80035C`), which is the *same* table
/// `FUN_801D6028` / `FUN_801D3A2C` / `FUN_801D6BBC` later index by the height
/// nibble - the ramp install and the height solve are two halves of one
/// mechanism, not two tables that happen to share an address.
pub fn height_ramp() -> [i16; HEIGHT_RAMP_LEN] {
    let mut ramp = [0i16; HEIGHT_RAMP_LEN];
    for (i, v) in ramp.iter_mut().enumerate() {
        *v = i as i16 * HEIGHT_RAMP_STEP;
    }
    ramp
}

/// One `0x20`-byte tile record at the head of the scene floor buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TileRecord {
    /// `+0x00` - world-x bias added to `cell_x * 0x80 + 0x40`.
    pub off_x: u16,
    /// `+0x02` - world-y bias added to the ramp height.
    pub off_y: u16,
    /// `+0x04` - world-z bias; the pass subtracts `off_z - 0x40`.
    pub off_z: u16,
    /// `+0x06` - signed neighbour-cell delta on x.
    pub nbr_dx: i8,
    /// `+0x07` - signed neighbour-cell delta on z.
    pub nbr_dz: i8,
    /// `+0x08` / `+0x0A` / `+0x0C` - the rotation trio copied into the
    /// spawned actor's `+0x24` / `+0x26` / `+0x28`.
    pub rot: [u16; 3],
    /// `+0x12` - the flag halfword (`TILE_FLAG_*`).
    pub flags: u16,
    /// `+0x1E` - non-zero sets the spawned actor's `+0x74` bit `0x40000000`.
    pub tag: u8,
}

/// A borrowed view over one scene floor buffer.
///
/// Every accessor is bounds-checked and returns a default rather than
/// panicking: retail indexes the buffer with masked coordinates that cannot
/// leave it, and a truncated buffer in the port must not abort a frame.
#[derive(Debug, Clone, Copy)]
pub struct FloorGrid<'a> {
    buf: &'a [u8],
}

impl<'a> FloorGrid<'a> {
    /// Wrap a scene floor buffer (`*_DAT_1F8003EC`).
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    fn u16_at(&self, off: usize) -> u16 {
        match self.buf.get(off..off + 2) {
            Some(b) => u16::from_le_bytes([b[0], b[1]]),
            None => 0,
        }
    }

    /// The `u16` cell word at grid `(gx, gz)` (`+0x8000`, pitch `0x100`).
    pub fn cell(&self, gx: i32, gz: i32) -> u16 {
        if !(0..GRID_EXTENT).contains(&gx) || !(0..GRID_EXTENT).contains(&gz) {
            return 0;
        }
        self.u16_at(CELL_GRID_OFF + gz as usize * CELL_GRID_PITCH + gx as usize * 2)
    }

    /// The tile id a cell word carries (bits `0..8`).
    pub fn tile_id(&self, gx: i32, gz: i32) -> u16 {
        self.cell(gx, gz) & CELL_TILE_ID_MASK
    }

    /// The raw `+0x4000` byte at grid `(gx, gz)` - height nibble in the low
    /// four bits, wall nibble in the high four.
    pub fn terrain_byte(&self, gx: i32, gz: i32) -> u8 {
        if !(0..GRID_EXTENT).contains(&gx) || !(0..GRID_EXTENT).contains(&gz) {
            return 0;
        }
        self.buf
            .get(HEIGHT_GRID_OFF + gz as usize * HEIGHT_GRID_PITCH + gx as usize)
            .copied()
            .unwrap_or(0)
    }

    /// The height-ramp index of a cell - the **low** nibble of
    /// [`terrain_byte`](Self::terrain_byte).
    pub fn height_index(&self, gx: i32, gz: i32) -> usize {
        (self.terrain_byte(gx, gz) & 0xF) as usize
    }

    /// The height-ramp index at a *flat* grid offset `gz * 0x80 + gx`, with no
    /// per-axis clamp. The height solver reads its `+x` / `+z` corners as
    /// `ptr[1]` / `ptr[0x80]` / `ptr[0x81]` off one pointer, so a cell on the
    /// right edge takes its `+x` corner from the head of the next row. Keeping
    /// that here rather than clamping is what makes the port's edge cells
    /// agree with retail's.
    fn height_index_flat(&self, gx: i32, gz: i32) -> usize {
        let off = gz * HEIGHT_GRID_PITCH as i32 + gx;
        if off < 0 {
            return 0;
        }
        let b = self
            .buf
            .get(HEIGHT_GRID_OFF + off as usize)
            .copied()
            .unwrap_or(0);
        (b & 0xF) as usize
    }

    /// The four sub-cell wall bits of a cell - the **high** nibble. The floor
    /// kernels never read these; the field collision probe does.
    pub fn wall_nibble(&self, gx: i32, gz: i32) -> u8 {
        self.terrain_byte(gx, gz) >> 4
    }

    /// The `0x20`-byte tile record for a tile id.
    pub fn tile(&self, id: u16) -> TileRecord {
        let base = id as usize * TILE_RECORD_STRIDE;
        let b = |o: usize| self.buf.get(base + o).copied().unwrap_or(0);
        TileRecord {
            off_x: self.u16_at(base),
            off_y: self.u16_at(base + 2),
            off_z: self.u16_at(base + 4),
            nbr_dx: b(6) as i8,
            nbr_dz: b(7) as i8,
            rot: [
                self.u16_at(base + 8),
                self.u16_at(base + 0xA),
                self.u16_at(base + 0xC),
            ],
            flags: self.u16_at(base + 0x12),
            tag: b(0x1E),
        }
    }
}

/// A step-layer patch record, the four bytes `FUN_801D79E0` returns from the
/// `+0x10000` / `+0x12000` layers when a cell carries [`CELL_STEP_LAYER`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepPatch {
    /// `+0x02` - a signed whole-step bias, scaled by `0x20`.
    pub step: i8,
    /// `+0x03` - four 2-bit sub-cell biases, scaled by `0x10`.
    pub quadrants: u8,
}

/// What [`ground_height`] resolves for one actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroundSample {
    /// The solved height, in world units.
    pub height: i32,
    /// The actor's flag word after the solver's `0x800000` maintenance.
    pub flags: u32,
    /// The cell word the solve read.
    pub cell: u16,
}

/// Solve the ground height under an actor and maintain its off-floor flag.
///
/// `world_x` / `world_z` are the actor's `+0x14` / `+0x18` halfwords, `flags`
/// its `+0x10` word, `ramp` the 16-entry table [`height_ramp`] builds, and
/// `step_layer` the `FUN_801D79E0` lookup for a cell carrying
/// [`CELL_STEP_LAYER`] (the `+0x10000` layer first, `+0x12000` as fallback -
/// retail tries them in that order and the port hands the caller the already
/// resolved record).
///
/// Two solve paths, chosen by [`CELL_STEP_LAYER`]:
///
/// - **plain** - if all four corner nibbles of the cell are equal the height
///   is that ramp entry exactly (retail returns early, skipping the blend);
///   otherwise the four corners are bilinearly blended over the sub-cell
///   fraction `world & 0x7F` in `0x80` units per axis and the `>> 14` is
///   biased `+0x3FFF` when negative (round toward zero).
/// - **step layer** - the four corners are averaged with a plain `>> 2` (no
///   rounding bias at all, unlike the blend path) and the patch's whole-step
///   and sub-cell biases are subtracted.
///
/// The flag maintenance runs first and is *not* skipped by either path: a
/// negative flag word only ORs [`ACTOR_FLAG_OFF_FLOOR`] in, while a
/// non-negative one clears it and re-raises it when the cell lacks
/// [`CELL_ON_FLOOR`].
// PORT: FUN_801d6028
// NOT WIRED: the engine has no scene floor buffer. `engine-core::field_env`
// assembles a scene's meshes, not the `_DAT_1F8003EC` grid the solver reads,
// and the port settles actors from mesh geometry instead. Wiring it needs the
// per-scene floor block decoded into a [`FloorGrid`] at scene load, which is a
// field-scene concern rather than a minigame one.
pub fn ground_height(
    grid: FloorGrid<'_>,
    world_x: i16,
    world_z: i16,
    flags: u32,
    ramp: &[i16],
    step_layer: impl FnOnce(i32, i32) -> Option<StepPatch>,
) -> GroundSample {
    // Half-cell indices: `>> 6` on the sign-extended world coordinate.
    let hx = (world_x >> 6) as i32;
    let hz = (world_z >> 6) as i32;
    // Grid indices: halve toward zero (retail's `srl 31; addu; sra 1`).
    let gx = hx / 2;
    let gz = hz / 2;
    let cell = grid.cell(gx, gz);

    let flags = if (flags as i32) < 0 {
        flags | ACTOR_FLAG_OFF_FLOOR
    } else {
        // The `(cell & 0x1800) == 0x800` arm that follows in retail re-ORs the
        // same bit it has just stored, so it changes nothing here.
        let cleared = flags & !ACTOR_FLAG_OFF_FLOOR;
        if cell & CELL_ON_FLOOR == 0 {
            cleared | ACTOR_FLAG_OFF_FLOOR
        } else {
            cleared
        }
    };

    let idx = |dx: i32, dz: i32| grid.height_index_flat(gx + dx, gz + dz);
    let corner = |dx: i32, dz: i32| -> i32 { ramp.get(idx(dx, dz)).copied().unwrap_or(0) as i32 };
    let n00 = corner(0, 0);
    let n01 = corner(1, 0);
    let n10 = corner(0, 1);
    let n11 = corner(1, 1);

    let height = if cell & CELL_STEP_LAYER != 0 {
        let bias = match step_layer(gx, gz) {
            Some(p) => {
                let shift = (hx & 1) * 2 + (hz & 1) * 4;
                let quad = ((p.quadrants >> shift) & 3) as i32;
                -quad * 0x10 - p.step as i32 * 0x20
            }
            None => 0,
        };
        ((n00 + n01 + n10 + n11) >> 2) + bias
    } else if idx(0, 0) == idx(1, 0) && idx(0, 0) == idx(0, 1) && idx(0, 0) == idx(1, 1) {
        n00
    } else {
        let fx = (world_x as u16 & 0x7F) as i32;
        let fz = (world_z as u16 & 0x7F) as i32;
        let acc =
            (n01 * fx + n00 * (0x80 - fx)) * (0x80 - fz) + n10 * (0x80 - fx) * fz + n11 * fx * fz;
        if acc < 0 {
            (acc + 0x3FFF) >> 14
        } else {
            acc >> 14
        }
    };

    GroundSample {
        height,
        flags,
        cell,
    }
}

/// One tile actor the floor pass places, in the order retail stores its
/// fields into the spawned record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloorTileSpawn {
    /// The grid cell the spawn came from.
    pub cell_x: i32,
    pub cell_z: i32,
    /// Tile id (`+0x60` on the spawned actor).
    pub tile_id: u16,
    /// World position handed to the spawn call.
    pub pos: [i16; 3],
    /// The draw-mode word staged next to the transform template
    /// (`DAT_801D4310`): `5` for a [`TILE_FLAG_ALT_MODE`] tile, else `0`.
    pub draw_mode: i16,
    /// `true` when the tile record's `+0x1E` tag sets `+0x74` bit
    /// `0x40000000`.
    pub actor_74_tag: bool,
    /// `true` when [`TILE_FLAG_ACTOR_74`] sets `+0x74` bit `0x10000000`.
    pub actor_74_flag: bool,
    /// `true` when [`TILE_FLAG_ACTOR_10`] sets `+0x10` bit `0x4`.
    pub actor_10_flag: bool,
    /// The tile record's rotation trio, copied into `+0x24` / `+0x26` /
    /// `+0x28`.
    pub rot: [u16; 3],
}

/// Walk the floor rect and resolve the tile actors it spawns.
///
/// `x0 .. x0 + width` by `z0 .. z0 + height` in grid cells. A cell is spawned
/// only when its tile record carries [`TILE_FLAG_DRAWN`], its neighbour probe
/// `(gx + nbr_dx, gz + nbr_dz)` lands inside the `0 .. 0x80` grid, and - when
/// `neighbour_block` is set - that neighbour's cell word lacks
/// [`CELL_NEIGHBOUR_BLOCK`].
///
/// The world position is `(gx * 0x80 + off_x + 0x40, ramp[height] + off_y,
/// gz * 0x80 - (off_z - 0x40))`; note the **z** term is a subtraction, so a
/// tile's `off_z` pushes it toward the camera, not away from it.
// PORT: FUN_801d3a2c (the dance overlay's per-frame floor pass)
// PORT: FUN_801d6bbc (the same pass in the shared overlay band; identical
// bytes in the fishing and dance images, differing only in which overlay-local
// globals it writes and in the `x0/z0/x1/z1` debug print it opens with)
// NOT WIRED: the pass spawns world actors into the shared actor list
// (`FUN_80024C88` against `*_DAT_8007C36C`) off a scene floor buffer the
// engine does not decode - the same missing [`FloorGrid`] source
// [`ground_height`] needs. `World::refresh_tile_board_draw_list` is the
// engine's only floor-shaped draw pass and it walks the *field-VM* tile board
// (a `width x height` byte cell array), which is a different grid with a
// different cell encoding.
pub fn floor_tile_spawns(
    grid: FloorGrid<'_>,
    ramp: &[i16],
    x0: i32,
    z0: i32,
    width: i32,
    height: i32,
    neighbour_block: bool,
) -> Vec<FloorTileSpawn> {
    let mut out = Vec::new();
    for gz in z0..z0 + height {
        for gx in x0..x0 + width {
            let tile_id = grid.tile_id(gx, gz);
            let rec = grid.tile(tile_id);
            if rec.flags & TILE_FLAG_DRAWN == 0 {
                continue;
            }
            let nx = gx + rec.nbr_dx as i32;
            let nz = gz + rec.nbr_dz as i32;
            if !(0..GRID_EXTENT).contains(&nx) || !(0..GRID_EXTENT).contains(&nz) {
                continue;
            }
            if neighbour_block && grid.cell(nx, nz) & CELL_NEIGHBOUR_BLOCK != 0 {
                continue;
            }
            let h = ramp
                .get(grid.height_index_flat(gx, gz))
                .copied()
                .unwrap_or(0) as i32;
            out.push(FloorTileSpawn {
                cell_x: gx,
                cell_z: gz,
                tile_id,
                pos: [
                    (gx * CELL_WORLD_UNITS + rec.off_x as i32 + 0x40) as i16,
                    (h + rec.off_y as i32) as i16,
                    (gz * CELL_WORLD_UNITS - (rec.off_z as i32 - 0x40)) as i16,
                ],
                draw_mode: if rec.flags & TILE_FLAG_ALT_MODE != 0 {
                    5
                } else {
                    0
                },
                actor_74_tag: rec.tag != 0,
                actor_74_flag: rec.flags & TILE_FLAG_ACTOR_74 != 0,
                actor_10_flag: rec.flags & TILE_FLAG_ACTOR_10 != 0,
                rot: rec.rot,
            });
        }
    }
    out
}

/// Which spawn template the dance floor's step-marker pass picks for a cell,
/// and the sub-index it stamps into the spawned actor's `+0x50`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerTemplate {
    /// Clip indices `6 ..= 9`: the alternate template (`DAT_801D4314`), with
    /// `clip - 6` stored into the actor's `+0x50`.
    Marker { sub_index: u16 },
    /// Any other non-zero clip index: the plain floor template
    /// (`DAT_801D42FC`), `+0x50` untouched.
    Plain,
}

/// Resolve the dance floor's step-marker template for one cell.
///
/// `marker` is `FUN_801D3EC0`'s per-cell record byte `+0x02` **plus one** -
/// retail spells this as `s2 = rec[2] + 1` with `s2 = 0` standing for "no
/// record here", so a record whose byte is `0xFF` and an absent record are
/// distinguishable and clip index `0` never occurs. `None` means the cell
/// draws no marker at all.
///
/// The `6 ..= 9` window is an unsigned `clip - 6 < 4` test, so it is exactly
/// the four marker clips; everything else falls through to the plain template.
// PORT: FUN_801d2a10 (template + `+0x50` sub-index selection)
// NOT WIRED: the two templates are overlay-resident actor prototypes
// (`DAT_801D42FC` / `DAT_801D4314`) copied by the shared spawn API, and the
// clip index comes from `FUN_801D3EC0`'s step-layer record lookup. Neither the
// prototypes nor the step layers are decoded by the engine - the same missing
// scene floor buffer [`floor_tile_spawns`] needs, plus a dance floor renderer
// to spawn into.
pub fn marker_template(marker: u16) -> Option<MarkerTemplate> {
    match marker {
        0 => None,
        6..=9 => Some(MarkerTemplate::Marker {
            sub_index: marker - 6,
        }),
        _ => Some(MarkerTemplate::Plain),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A floor buffer big enough for one tile record, the height grid and the
    /// cell grid.
    fn buffer() -> Vec<u8> {
        vec![0u8; CELL_GRID_OFF + GRID_EXTENT as usize * CELL_GRID_PITCH]
    }

    fn set_cell(buf: &mut [u8], gx: usize, gz: usize, v: u16) {
        let off = CELL_GRID_OFF + gz * CELL_GRID_PITCH + gx * 2;
        buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }

    fn set_terrain(buf: &mut [u8], gx: usize, gz: usize, v: u8) {
        buf[HEIGHT_GRID_OFF + gz * HEIGHT_GRID_PITCH + gx] = v;
    }

    #[test]
    fn ramp_is_a_flat_32_unit_ladder() {
        let r = height_ramp();
        assert_eq!(r[0], 0);
        assert_eq!(r[1], 0x20);
        assert_eq!(r[15], 0x1E0);
        assert_eq!(r.len(), HEIGHT_RAMP_LEN);
    }

    #[test]
    fn terrain_byte_splits_into_height_and_wall_nibbles() {
        let mut buf = buffer();
        set_terrain(&mut buf, 3, 4, 0xA5);
        let g = FloorGrid::new(&buf);
        assert_eq!(g.height_index(3, 4), 5);
        assert_eq!(g.wall_nibble(3, 4), 0xA);
    }

    #[test]
    fn flat_cell_returns_the_ramp_entry_exactly() {
        let mut buf = buffer();
        // Cell (1, 1): flat height index 3 across all four corners.
        for (gx, gz) in [(1, 1), (2, 1), (1, 2), (2, 2)] {
            set_terrain(&mut buf, gx, gz, 3);
        }
        set_cell(&mut buf, 1, 1, CELL_ON_FLOOR);
        let ramp = height_ramp();
        // world x/z = 0x80..0xFF lands in half-cells 2/3 -> grid cell 1.
        let s = ground_height(FloorGrid::new(&buf), 0xC0, 0xC0, 0, &ramp, |_, _| None);
        assert_eq!(s.height, 3 * 0x20);
        // `CELL_ON_FLOOR` present -> the off-floor bit stays clear.
        assert_eq!(s.flags & ACTOR_FLAG_OFF_FLOOR, 0);
    }

    #[test]
    fn missing_on_floor_bit_raises_the_actor_flag() {
        let buf = buffer();
        let ramp = height_ramp();
        let s = ground_height(FloorGrid::new(&buf), 0, 0, 0, &ramp, |_, _| None);
        assert_eq!(s.flags & ACTOR_FLAG_OFF_FLOOR, ACTOR_FLAG_OFF_FLOOR);
    }

    #[test]
    fn a_negative_flag_word_only_ors_the_bit_in() {
        let buf = buffer();
        let ramp = height_ramp();
        // A negative flag word skips the clear-then-maintain arm entirely, so
        // every other bit survives.
        let s = ground_height(FloorGrid::new(&buf), 0, 0, 0x8000_00FF, &ramp, |_, _| None);
        assert_eq!(s.flags, 0x8080_00FF);
    }

    #[test]
    fn uneven_corners_blend_over_the_sub_cell_fraction() {
        let mut buf = buffer();
        // Cell 0: corner (0,0) index 0, (1,0) index 4, rest 0.
        set_terrain(&mut buf, 1, 0, 4);
        set_cell(&mut buf, 0, 0, CELL_ON_FLOOR);
        let ramp = height_ramp();
        // Dead on the +x corner: fx = 0x7F, fz = 0 -> nearly the full 4*0x20.
        let s = ground_height(FloorGrid::new(&buf), 0x7F, 0, 0, &ramp, |_, _| None);
        assert_eq!(s.height, (4 * 0x20 * 0x7F * 0x80) >> 14);
        // Dead on the origin corner: fx = 0 -> exactly the (0,0) entry.
        let s0 = ground_height(FloorGrid::new(&buf), 0, 0, 0, &ramp, |_, _| None);
        assert_eq!(s0.height, 0);
    }

    #[test]
    fn step_layer_path_averages_without_a_rounding_bias() {
        let mut buf = buffer();
        set_terrain(&mut buf, 0, 0, 1);
        set_terrain(&mut buf, 1, 0, 2);
        set_cell(&mut buf, 0, 0, CELL_STEP_LAYER | CELL_ON_FLOOR);
        let ramp = height_ramp();
        // No patch: plain `(0x20 + 0x40 + 0 + 0) >> 2`.
        let s = ground_height(FloorGrid::new(&buf), 0x10, 0x10, 0, &ramp, |_, _| None);
        assert_eq!(s.height, (0x20 + 0x40) >> 2);
        // With a patch: quadrant bits at the half-cell parity of (0, 0) are
        // the low two bits, and the whole step scales by 0x20.
        let s2 = ground_height(FloorGrid::new(&buf), 0x10, 0x10, 0, &ramp, |_, _| {
            Some(StepPatch {
                step: 1,
                quadrants: 0b11,
            })
        });
        assert_eq!(s2.height, ((0x20 + 0x40) >> 2) - 3 * 0x10 - 0x20);
    }

    #[test]
    fn floor_pass_skips_undrawn_and_out_of_grid_tiles() {
        let mut buf = buffer();
        // Tile id 1: drawn, neighbour delta (0, 0).
        let base = TILE_RECORD_STRIDE;
        buf[base + 0x12..base + 0x14].copy_from_slice(&TILE_FLAG_DRAWN.to_le_bytes());
        // Tile id 2: drawn, but its neighbour probe leaves the grid.
        let base2 = 2 * TILE_RECORD_STRIDE;
        buf[base2 + 0x12..base2 + 0x14].copy_from_slice(&TILE_FLAG_DRAWN.to_le_bytes());
        buf[base2 + 6] = 0x80; // -128 on x
        set_cell(&mut buf, 0, 0, 1);
        set_cell(&mut buf, 1, 0, 2);
        set_cell(&mut buf, 2, 0, 0); // tile 0 has no drawn flag
        let ramp = height_ramp();
        let spawns = floor_tile_spawns(FloorGrid::new(&buf), &ramp, 0, 0, 3, 1, true);
        assert_eq!(spawns.len(), 1);
        assert_eq!(spawns[0].tile_id, 1);
        assert_eq!(spawns[0].cell_x, 0);
    }

    #[test]
    fn floor_pass_places_z_by_subtracting_the_record_bias() {
        let mut buf = buffer();
        let base = TILE_RECORD_STRIDE;
        buf[base + 0x12..base + 0x14].copy_from_slice(&TILE_FLAG_DRAWN.to_le_bytes());
        buf[base + 4..base + 6].copy_from_slice(&0x60u16.to_le_bytes()); // off_z
        set_cell(&mut buf, 1, 2, 1);
        let ramp = height_ramp();
        let spawns = floor_tile_spawns(FloorGrid::new(&buf), &ramp, 1, 2, 1, 1, false);
        assert_eq!(spawns.len(), 1);
        assert_eq!(spawns[0].pos[0], (CELL_WORLD_UNITS + 0x40) as i16);
        assert_eq!(
            spawns[0].pos[2],
            (2 * CELL_WORLD_UNITS - (0x60 - 0x40)) as i16
        );
    }

    #[test]
    fn marker_template_window_is_exactly_the_four_clips() {
        assert_eq!(marker_template(0), None);
        assert_eq!(marker_template(5), Some(MarkerTemplate::Plain));
        assert_eq!(
            marker_template(6),
            Some(MarkerTemplate::Marker { sub_index: 0 })
        );
        assert_eq!(
            marker_template(9),
            Some(MarkerTemplate::Marker { sub_index: 3 })
        );
        assert_eq!(marker_template(10), Some(MarkerTemplate::Plain));
    }
}
