//! The field actor **drop shadow** - the dark blob retail draws under every
//! seated actor, `FUN_8001C394`.
//!
//! The animated-actor renderer `FUN_8001B964` ends every draw with a gate
//! (`0x8001BE20..0x8001BE48`): when the actor's flag word (staged into
//! scratchpad `0x1F8002D0` by the actor pass `FUN_8001ADA4`) carries either
//! class bit of `0x01020000` - `0x20000`, which the MAN placement seater
//! `FUN_8003A1E4` ORs into every partition-1 actor, or `0x01000000`, the party
//! bit - and not `0x200000` (raised by the jump take-off and by the scripted
//! vanish that also zeroes `+0x72`), it calls `FUN_8001C394(actor)`. The gate
//! runs even when the render scale `+0x72` is zero: that test branches
//! straight to it, so a collapsed actor keeps its shadow unless `0x200000` is
//! up.
//!
//! # Geometry
//!
//! `FUN_800460AC(actor + 0x14)` projects a three-by-three grid of points
//! around the actor's position with the camera matrix `FUN_8001B964` has just
//! restored from `0x1F8003C8`: three `RTPT`s over the rows `z + 0x20`, `z`,
//! `z - 0x20`, each row `x - 0x20`, `x`, `x + 0x20`, all at the actor's own
//! `y` (`0x800460E0..0x80046164`). Each point lands in the scratchpad as an
//! `[SXY, SZ]` word pair from `0x1F800020`, so a row is `0x18` bytes.
//!
//! `FUN_8001C394` then walks the grid's four cells (`0x8001C3D4..0x8001C5EC`,
//! cell row outer, column inner) and emits one `POLY_FT4` each:
//!
//! | word | value |
//! |---|---|
//! | tag | `0x09000000` (nine words) |
//! | command + colour | `0x2E808080` - textured, semi-transparent, texture-blended, neutral |
//! | vertices | points `(r, c)`, `(r, c + 1)`, `(r + 1, c)`, `(r + 1, c + 1)` |
//! | UVs | `u = 0xE0 + 8c .. 0xE7 + 8c`, `v = 8r .. 8r + 7` |
//! | CLUT | `0x7F86` - row 510, `x = 96` |
//! | texpage | `0x001F` - `(960, 256)`, 4-bit, ABR 0 (`B / 2 + F / 2`) |
//!
//! So the four cells tile one `16 x 16` blob at `(0xE0, 0)` of the
//! menu-glyph atlas page, each quad sampling one `8 x 8` quarter
//! (`legaia_asset::menu_glyph_atlas` - the boot system-UI bundle keeps it
//! resident, [`crate::scene_resources`]). The blob's fill is palette index
//! `5`, a dark grey with the STP bit set; its surround is index `0`, which
//! the GPU treats as transparent.
//!
//! The cell's OT slot is `((SZ_a + SZ_b + SZ_c + SZ_d + 0xA0) >> 4) >> shift`
//! with `shift` the OT-resolution byte `DAT_1F8003A4`: the mean depth over
//! four, biased `0xA0 / 4` deeper so the blob sorts behind the actor standing
//! on it. An actor carrying `0x800000` links ten slots nearer (`-0x28` off
//! the table base, `0x8001C51C`), the same offset its own mesh took.
//!
//! On the overworld (`_DAT_1F800394 & 1`) every vertex's `SY` also takes the
//! curvature entry at `(sum >> 2) >> 5` - **without** the `+1` the fog and the
//! prim leaves index with ([`crate::overworld_curvature`]) - so the blob rides
//! the same horizon bend as the ground under it (`0x8001C4A4..0x8001C504`).
//!
//! Nothing in the routine culls: every cell is linked whatever its depth.

use crate::overworld_curvature::curvature_table;

/// Grid pitch, both axes (`addi s5,t3,-0x20` / `addi t5,t5,-0x20`).
pub const SHADOW_GRID_STEP: i32 = 0x20;
/// The packet's texpage word (`li v0,0x1f`, `sh v0,0x16(a2)`).
pub const SHADOW_TPAGE: u16 = 0x001F;
/// The packet's CLUT word (`li v0,0x7f86`, `sh v0,0xe(a2)`).
pub const SHADOW_CLUT: u16 = 0x7F86;
/// The modulation colour in the command word (`0x2E80_8080`).
pub const SHADOW_RGB: [u8; 3] = [0x80, 0x80, 0x80];
/// Left texel column of the blob (`li t2,0xe0`).
pub const SHADOW_U0: u8 = 0xE0;
/// The OT bias added to the four-depth sum (`addiu v1,a0,0xa0`).
pub const SHADOW_OT_BIAS: i32 = 0xA0;
/// Slots the `0x800000` actors link nearer (`lw v0,-0x28(v0)`: ten words).
pub const SHADOW_NEAR_SLOTS: u32 = 10;

/// Flag-word bits of `FUN_8001B964`'s shadow gate.
pub const SHADOW_CLASS_BITS: u32 = 0x0102_0000;
/// The opt-out bit the gate tests clear (`lui v1,0x20`).
pub const SHADOW_SUPPRESS_BIT: u32 = 0x0020_0000;
/// The actor-flag bit that pulls the blob ten OT slots nearer.
pub const SHADOW_NEAR_BIT: u32 = 0x0080_0000;

/// Whether `FUN_8001B964`'s gate emits a shadow for flag word `flags`.
pub fn casts_shadow(flags: u32) -> bool {
    flags & SHADOW_CLASS_BITS != 0 && flags & SHADOW_SUPPRESS_BIT == 0
}

/// The nine grid points `FUN_800460AC` transforms, in scratchpad order
/// (row-major, the `z + 0x20` row first, `x - 0x20` first in a row).
///
/// PORT: FUN_800460AC
pub fn shadow_grid(pos: [i32; 3]) -> [[i32; 3]; 9] {
    let [x, y, z] = pos;
    let mut out = [[0; 3]; 9];
    for (r, row) in out.chunks_mut(3).enumerate() {
        let pz = z + SHADOW_GRID_STEP - r as i32 * SHADOW_GRID_STEP;
        for (c, p) in row.iter_mut().enumerate() {
            *p = [x - SHADOW_GRID_STEP + c as i32 * SHADOW_GRID_STEP, y, pz];
        }
    }
    out
}

/// One transformed grid point: the GTE's `SXY` pair and `SZ`, plus the
/// host's scene depth for a depth-tested draw (see [`DropShadowQuad::depth`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowVertex {
    pub sx: i16,
    pub sy: i16,
    pub sz: u16,
    pub depth: Option<f32>,
}

/// One cell of the blob as the `POLY_FT4` retail links.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropShadowQuad {
    /// Corners in `POLY_FT4` order.
    pub xy: [(i16, i16); 4],
    pub uv: [(u8, u8); 4],
    pub clut: u16,
    pub tpage: u16,
    pub rgb: [u8; 3],
    /// The retail OT slot (larger = farther).
    pub ot_index: u32,
    /// Per-corner scene depth in the frame matrix's normalised depth
    /// (`legaia_engine_ui::screen_prim::CornerDepth`'s convention), when the
    /// host draws the blob depth-tested against its 3D scene. Retail has no
    /// depth buffer: its ground sorts into the far bucket and the actor in
    /// front of the blob's `+0xA0` bias, which a host with a depth buffer
    /// reproduces by testing the blob against the scene it already drew.
    pub depth: Option<[f32; 4]>,
}

/// The four cells of one actor's blob from its transformed grid - the
/// packet half of `FUN_8001C394`. `overworld` is `_DAT_1F800394 & 1`,
/// `ot_shift` the OT-resolution byte `DAT_1F8003A4`, `near` the actor's
/// `0x800000` bit.
///
/// PORT: FUN_8001C394
pub fn shadow_quads(
    grid: &[ShadowVertex; 9],
    overworld: bool,
    ot_shift: u8,
    near: bool,
) -> [DropShadowQuad; 4] {
    let mut out = [DropShadowQuad {
        xy: [(0, 0); 4],
        uv: [(0, 0); 4],
        clut: SHADOW_CLUT,
        tpage: SHADOW_TPAGE,
        rgb: SHADOW_RGB,
        ot_index: 0,
        depth: None,
    }; 4];
    for r in 0..2 {
        for c in 0..2 {
            let corners = [
                grid[r * 3 + c],
                grid[r * 3 + c + 1],
                grid[(r + 1) * 3 + c],
                grid[(r + 1) * 3 + c + 1],
            ];
            let sum: i32 = corners.iter().map(|v| i32::from(v.sz)).sum();
            // 0x8001C4A4..0x8001C504: one curvature entry for all four `SY`s.
            let bend = if overworld {
                let i = (((sum >> 2) >> 5).max(0) as usize).min(curvature_table().len() - 1);
                curvature_table()[i]
            } else {
                0
            };
            let (u0, v0) = (SHADOW_U0 + 8 * c as u8, 8 * r as u8);
            let mut slot = (((sum + SHADOW_OT_BIAS) >> 4) >> (ot_shift & 0x1F)).max(0) as u32;
            if near {
                slot = slot.saturating_sub(SHADOW_NEAR_SLOTS);
            }
            let depth = corners
                .iter()
                .map(|v| v.depth)
                .collect::<Option<Vec<f32>>>()
                .map(|d| [d[0], d[1], d[2], d[3]]);
            out[r * 2 + c] = DropShadowQuad {
                xy: corners.map(|v| (v.sx, v.sy.wrapping_add(bend))),
                uv: [(u0, v0), (u0 + 7, v0), (u0, v0 + 7), (u0 + 7, v0 + 7)],
                ot_index: slot,
                depth,
                ..out[r * 2 + c]
            };
        }
    }
    out
}

/// One actor's blob through a host projector: the grid, each point through
/// `project` (`None` = at or behind the eye, where the GTE's divide would
/// overflow - the port drops the whole blob), then [`shadow_quads`].
pub fn drop_shadow(
    pos: [i32; 3],
    overworld: bool,
    ot_shift: u8,
    near: bool,
    project: impl Fn([i32; 3]) -> Option<ShadowVertex>,
) -> Option<[DropShadowQuad; 4]> {
    let pts = shadow_grid(pos);
    let mut grid = [ShadowVertex {
        sx: 0,
        sy: 0,
        sz: 0,
        depth: None,
    }; 9];
    for (g, p) in grid.iter_mut().zip(pts) {
        *g = project(p)?;
    }
    Some(shadow_quads(&grid, overworld, ot_shift, near))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(sx: i16, sy: i16, sz: u16) -> ShadowVertex {
        ShadowVertex {
            sx,
            sy,
            sz,
            depth: None,
        }
    }

    #[test]
    fn the_grid_is_three_rows_far_first() {
        let g = shadow_grid([100, -8, 200]);
        assert_eq!(g[0], [68, -8, 232]);
        assert_eq!(g[4], [100, -8, 200]);
        assert_eq!(g[8], [132, -8, 168]);
    }

    #[test]
    fn cells_tile_the_blob_and_sort_behind_the_mean() {
        let mut grid = [flat(0, 0, 0); 9];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = flat((i % 3) as i16 * 10, (i / 3) as i16 * 10, 1000);
        }
        let q = shadow_quads(&grid, false, 0, false);
        assert_eq!(q[0].xy, [(0, 0), (10, 0), (0, 10), (10, 10)]);
        assert_eq!(q[3].xy, [(10, 10), (20, 10), (10, 20), (20, 20)]);
        assert_eq!(q[0].uv, [(0xE0, 0), (0xE7, 0), (0xE0, 7), (0xE7, 7)]);
        assert_eq!(q[1].uv[3], (0xEF, 7));
        assert_eq!(q[2].uv[3], (0xE7, 15));
        // (4 * 1000 + 0xA0) >> 4.
        assert!(q.iter().all(|c| c.ot_index == 260));
        let near = shadow_quads(&grid, false, 3, true);
        assert_eq!(near[0].ot_index, (260 >> 3) - 10);
        assert!(
            q.iter()
                .all(|c| c.clut == SHADOW_CLUT && c.tpage == SHADOW_TPAGE)
        );
    }

    #[test]
    fn the_overworld_bends_every_corner_by_one_unshifted_entry() {
        let grid = [flat(0, 50, 0x2000); 9];
        let q = shadow_quads(&grid, true, 0, false);
        let bend = curvature_table()[0x2000 >> 5];
        assert!(bend > 0);
        assert!(q[0].xy.iter().all(|&(_, y)| y == 50 + bend));
    }

    #[test]
    fn the_gate_wants_a_class_bit_and_no_suppress_bit() {
        assert!(casts_shadow(0x0902_0880));
        assert!(casts_shadow(0x0802_0886));
        assert!(!casts_shadow(0x0800_8882));
        assert!(!casts_shadow(0x2822_0882));
    }
}
