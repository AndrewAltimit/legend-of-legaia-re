//! Battle backdrop kernels: which stage-dome objects get drawn, and the
//! procedural ground grid's geometry / texture / visibility rules.
//!
//! Both halves of the battle backdrop are decided by code rather than by the
//! asset, which is why they live here as pure kernels rather than inside a
//! host: the dome's object list is edited at registration time, and the ground
//! is not geometry from a file at all.
//!
//! PORT: FUN_800513f0 - the backdrop registration's object-table edit.
//! PORT: FUN_801d02c0 - the ground grid's origin, sub-tile UVs and visibility.
//!
//! Sources: `ghidra/scripts/funcs/800513f0.txt` and
//! `ghidra/scripts/funcs/overlay_battle_action_801d02c0.txt`, both read as
//! disassembly.

// ---------------------------------------------------------------------------
// Stage dome object list
// ---------------------------------------------------------------------------

/// The object index the backdrop registration drops.
///
/// `FUN_800513f0` registers the resident stage TMD once (`tmd_register` at
/// `0x80051a60`) and then allocates **two** backdrop actors from the same
/// descriptor `0x8007680c` (`0x80051a7c` and `0x80051aa8`, results stored at
/// `_DAT_8007bd24 + 0x106c` and `+ 0x1070`). Each actor owns a private
/// `0x9c`-byte part table at `+0x44` - `actor_alloc` zeroes the field
/// (`0x80020f04`) and the link pass allocates it (`jal 0x80017888` with
/// `a1 = 0x9c` at `0x80021184`, stored at `0x80021190`) - laid out as
/// `[u32 count][u32 entry[..]]`.
///
/// The edit at `0x80051ad4..0x80051bac` then does, **per actor**, one
/// `count -= 1` (`0x80051aec` / `0x80051b0c`) followed by
/// `entry[i] = entry[i + 1]` for `i` in `1..count` (`0x80051b68` /
/// `0x80051b88`). One decrement and one left-shift from index 1 removes
/// exactly index 1 and keeps the rest.
///
/// The tables are per-actor allocations, not one shared table, which is what
/// makes this a single drop rather than a double one - worth stating because
/// the two decrements sit back to back and read as a pair.
pub const BACKDROP_DROPPED_OBJECT: usize = 1;

/// Runtime byte that suppresses the object drop entirely (`_DAT_8007b64b`).
///
/// `0x80051abc` loads it and `0x80051acc` branches past the whole edit when it
/// is non-zero, so a non-zero value leaves every object registered. The port
/// models the `== 0` arm, which is the one every catalogued battle savestate
/// is in.
pub const BACKDROP_DROP_GATE: u32 = 0x8007_B64B;

/// The stage-dome object indices a backdrop actor actually draws.
///
/// Retail's part table is the full object list minus
/// [`BACKDROP_DROPPED_OBJECT`], so this is `0, 2, 3, ..` for anything with
/// more than two objects and `0` alone for the two-object shells that make up
/// the bulk of the corpus.
///
/// The two shapes on the disc:
///
/// * **Two-object stage shells** (the common case, e.g. `town01` extraction 7)
///   keep object 0 - the arena shell - and drop object 1, the ground-level
///   ribbon of near props that no retail capture shows on screen.
/// * **Four-object overworld domes** (`map01` / `map02` / `map03`, extraction
///   88/89/90, 247/248/249, 394) keep objects 0 (sky), 2 (mountains) and 3
///   (the flat ground ring at `Y = 0`), dropping only object 1.
///
/// Drawing object 0 alone is therefore right for the shells and wrong for the
/// domes: it loses the mountain ring and the far ground.
pub fn backdrop_object_indices(object_count: usize) -> Vec<usize> {
    (0..object_count)
        .filter(|i| *i != BACKDROP_DROPPED_OBJECT)
        .collect()
}

// ---------------------------------------------------------------------------
// Procedural ground grid
// ---------------------------------------------------------------------------

/// Grid cell pitch in world units (`0x200`).
pub const GRID_CELL_PITCH: i32 = 0x200;

/// Sub-cell step (`0x100`) - half a cell, the spacing of the `3 x 3` corner
/// lattice each cell is projected from.
pub const GRID_SUB_STEP: i32 = 0x100;

/// Texture page attribute for the ground tile: 4bpp page at framebuffer
/// `(832, 0)`.
///
/// Read straight out of the emitter's UV word table (`lui s1,0xd` at
/// `0x801d030c`, landing in the `tpage` half of the second POLY_GT4 UV word).
pub const GROUND_TSB: u16 = 0x000D;

/// CLUT attribute for the ground tile: palette at framebuffer `(0, 479)`
/// (`lui t5,0x77c0` at `0x801d0304`, the `clut` half of the first UV word).
pub const GROUND_CBA: u16 = 0x77C0;

/// Low UV coordinate of the tile window (`0xC0`). The window is
/// `(192..=255)^2` and holds four `32 x 32` sub-tiles.
pub const GROUND_UV_BASE: u8 = 0xC0;

/// Side of one sub-tile in texels.
pub const GROUND_SUB_TILE: u8 = 0x20;

/// Sub-tiles per cell side - the emit loop runs `2 x 2` per visible cell.
pub const GRID_SUB_TILES_PER_SIDE: i32 = 2;

/// One sub-tile's UV window as `(u_lo, v_lo, u_hi, v_hi)`, inclusive.
///
/// The four entries are the emitter's own UV word table, built by the
/// `lui`/`ori` block at `0x801d0304..0x801d03a0` into scratchpad `0x1f800034`
/// and read back one group per quad at `0x801d0660` (the read cursor advances
/// `0x10` per quad, `0x801d06c8`). Decoding the POLY_GT4 UV words - word 0
/// `clut:u0:v0`, word 1 `tpage:u1:v1`, words 2/3 `u2:v2` / `u3:v3` - gives:
///
/// | Quad | Words | `u` | `v` |
/// |---:|---|---|---|
/// | 0 | `77c0c0c0 000dc0df 0000dfc0 0000dfdf` | `0xC0..=0xDF` | `0xC0..=0xDF` |
/// | 1 | `77c0c0e0 000dc0ff 0000dfe0 0000dfff` | `0xE0..=0xFF` | `0xC0..=0xDF` |
/// | 2 | `77c0e0c0 000de0df 0000ffc0 0000ffdf` | `0xC0..=0xDF` | `0xE0..=0xFF` |
/// | 3 | `77c0e0e0 000de0ff 0000ffe0 0000ffff` | `0xE0..=0xFF` | `0xE0..=0xFF` |
///
/// so quad `n` takes sub-tile `n`, and since the emit loop's inner axis is the
/// sub-column and its outer axis the sub-row, `n == sub_row * 2 + sub_col`.
/// **There is no RNG anywhere in the routine** and no corner mirroring: the
/// tiling is deterministic and the UVs are read verbatim from this table. The
/// random corner mirror belongs to the particle scatter `FUN_801E0080`.
pub const GROUND_SUB_TILE_UVS: [(u8, u8, u8, u8); 4] = [
    (0xC0, 0xC0, 0xDF, 0xDF),
    (0xE0, 0xC0, 0xFF, 0xDF),
    (0xC0, 0xE0, 0xDF, 0xFF),
    (0xE0, 0xE0, 0xFF, 0xFF),
];

/// The sub-tile a cell's `(sub_row, sub_col)` quad samples.
pub fn ground_sub_tile_uv(sub_row: i32, sub_col: i32) -> (u8, u8, u8, u8) {
    GROUND_SUB_TILE_UVS[((sub_row * GRID_SUB_TILES_PER_SIDE + sub_col) & 3) as usize]
}

/// World-space `(x, z)` of the grid's first cell corner for a `w x h` grid.
///
/// `0x801d03b4..0x801d03d8`: `x0 = -((w >> 1) << 9)` and
/// `z0 = -((h >> 1) << 9) - 0x200`. The `z` axis carries an extra
/// `-0x200` bias, so the grid is **not** symmetric about the origin - it sits
/// one cell further toward `-Z` than a naive centring would put it.
pub fn grid_origin(width: i32, height: i32) -> (i32, i32) {
    let x0 = -((width >> 1) << 9);
    let z0 = -((height >> 1) << 9) - GRID_CELL_PITCH;
    (x0, z0)
}

/// Pass-1 visibility verdict for one cell, matching the byte retail writes
/// into `_DAT_8007b814`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellVisibility {
    /// `-1`: the cell centre is at or behind the near limit.
    Behind,
    /// `0`: the cell centre is past the far limit.
    TooFar,
    /// `1`: emit this cell.
    Visible,
}

/// Near limit added to the view-space `z` before the sign test (`0x200`).
pub const GRID_NEAR_BIAS: i32 = 0x200;

/// Far limit the biased `z` is compared against (`0x6700`), i.e. a raw view
/// `z` of `0x6500`.
pub const GRID_FAR_LIMIT: i32 = 0x6700;

/// Classify one cell from the view-space `z` of its centre.
///
/// Pass 1 (`0x801d0420..0x801d0464`) transforms the cell centre
/// `(x + 0x100, ~0, z + 0x100)` by the view matrix with `MVMVA`
/// (`cop2 0x0480012`: rotation matrix, `V0`, `+TR`, `sf = 1`), reads `IR3`
/// back with `mfc2 t4, $11`, and then:
///
/// ```text
/// t = z + 0x200
/// if t <= 0            -> -1   (bgtz t4 falls through to addi s0, zero, -1)
/// else if t - 0x6700 > 0 -> 0  (bgtz t4 skips the store of 1)
/// else                 ->  1
/// ```
///
/// Only `1` emits: pass 2 skips on both `bltz s0` (`0x801d04b0`) and
/// `beq s0, zero` (`0x801d04b8`).
///
/// Note what is *not* here - there is no screen-space test in pass 1. The
/// screen-space rejection is a separate, per-cell test in pass 2
/// ([`cell_offscreen`]).
pub fn classify_cell(view_z: i32) -> CellVisibility {
    let t = view_z + GRID_NEAR_BIAS;
    if t <= 0 {
        CellVisibility::Behind
    } else if t - GRID_FAR_LIMIT > 0 {
        CellVisibility::TooFar
    } else {
        CellVisibility::Visible
    }
}

/// PSX display width the pass-2 reject compares against.
pub const SCREEN_W: i32 = 0x140;
/// PSX display height the pass-2 reject compares against.
pub const SCREEN_H: i32 = 0xF0;

/// Pass-2 screen-space rejection for a cell, from the projected screen `(x, y)`
/// of its four **outer** corners.
///
/// `0x801d052c..0x801d05e8` reads the corner lattice's `(0,0)`, `(2,0)`,
/// `(0,2)` and `(2,2)` points and rejects the cell when all four fall off the
/// same edge: all `y <= 0`, or all `y >= 0xF0`, or all `x <= 0`, or all
/// `x >= 0x140`. It is a bounding-box reject against the display rect, so a
/// cell straddling an edge is kept.
pub fn cell_offscreen(corners: [(i32, i32); 4]) -> bool {
    corners.iter().all(|c| c.1 <= 0)
        || corners.iter().all(|c| c.1 >= SCREEN_H)
        || corners.iter().all(|c| c.0 <= 0)
        || corners.iter().all(|c| c.0 >= SCREEN_W)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_object_shell_keeps_object_zero_only() {
        // The common stage shape: object 1 is the near-prop ribbon retail
        // drops, leaving the arena shell alone on screen.
        assert_eq!(backdrop_object_indices(2), vec![0]);
    }

    #[test]
    fn four_object_dome_keeps_sky_mountains_and_ground() {
        // map01's dome: obj0 sky, obj1 near detail (dropped), obj2 mountains,
        // obj3 flat ground ring.
        assert_eq!(backdrop_object_indices(4), vec![0, 2, 3]);
    }

    #[test]
    fn single_object_stage_is_left_alone() {
        assert_eq!(backdrop_object_indices(1), vec![0]);
        assert_eq!(backdrop_object_indices(0), Vec::<usize>::new());
    }

    #[test]
    fn sub_tiles_walk_the_two_by_two_block_in_row_major_order() {
        assert_eq!(ground_sub_tile_uv(0, 0), (0xC0, 0xC0, 0xDF, 0xDF));
        assert_eq!(ground_sub_tile_uv(0, 1), (0xE0, 0xC0, 0xFF, 0xDF));
        assert_eq!(ground_sub_tile_uv(1, 0), (0xC0, 0xE0, 0xDF, 0xFF));
        assert_eq!(ground_sub_tile_uv(1, 1), (0xE0, 0xE0, 0xFF, 0xFF));
    }

    #[test]
    fn every_sub_tile_is_thirty_two_texels_square() {
        for (u0, v0, u1, v1) in GROUND_SUB_TILE_UVS {
            assert_eq!(u1 - u0, GROUND_SUB_TILE - 1);
            assert_eq!(v1 - v0, GROUND_SUB_TILE - 1);
        }
        // ...and together they tile the (192..=255)^2 window exactly.
        let lo = GROUND_SUB_TILE_UVS.iter().map(|t| t.0).min().unwrap();
        let hi = GROUND_SUB_TILE_UVS.iter().map(|t| t.2).max().unwrap();
        assert_eq!((lo, hi), (GROUND_UV_BASE, 0xFF));
    }

    #[test]
    fn grid_origin_is_biased_one_cell_toward_negative_z() {
        // 28 x 28 is the live grid: x spans [-7168, +7168] but z is pulled a
        // whole cell back, to [-7680, +6656].
        assert_eq!(grid_origin(28, 28), (-7168, -7680));
        let (x0, z0) = grid_origin(28, 28);
        assert_eq!(x0 - z0, GRID_CELL_PITCH);
    }

    #[test]
    fn visibility_brackets_the_view_z_range() {
        assert_eq!(classify_cell(-0x200), CellVisibility::Behind);
        assert_eq!(classify_cell(-0x201), CellVisibility::Behind);
        assert_eq!(classify_cell(-0x1FF), CellVisibility::Visible);
        assert_eq!(classify_cell(0), CellVisibility::Visible);
        assert_eq!(classify_cell(0x6500), CellVisibility::Visible);
        assert_eq!(classify_cell(0x6501), CellVisibility::TooFar);
    }

    #[test]
    fn offscreen_reject_needs_all_four_corners_past_one_edge() {
        // Fully above the display.
        assert!(cell_offscreen([(10, -5), (40, -5), (10, -30), (40, -30)]));
        // Fully right of it.
        assert!(cell_offscreen([(400, 10), (420, 10), (400, 40), (420, 40)]));
        // Straddling the top edge is kept - three corners off is not enough.
        assert!(!cell_offscreen([(10, -5), (40, -5), (10, 20), (40, -5)]));
        // Wholly on screen.
        assert!(!cell_offscreen([(10, 10), (40, 10), (10, 40), (40, 40)]));
        // Spanning the whole display is kept: no single edge has all four.
        assert!(!cell_offscreen([
            (-10, -10),
            (400, -10),
            (-10, 300),
            (400, 300)
        ]));
    }
}
