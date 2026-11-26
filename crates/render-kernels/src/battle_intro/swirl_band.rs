//! The swirl's band submit and its trig tables.

use super::*;
use crate::battle_intro_swirl as swirl;
use crate::battle_intro_swirl::{SwirlBandDraw, SwirlMesh};
use crate::gte::{GteMat3, GteVec3, psx_cos, psx_sin};
use crate::screen_prim::ScreenPrim;

// ---------------------------------------------------------------------------
// The swirl's band submit (FUN_801D1A20)
// ---------------------------------------------------------------------------

/// Colour word of a swirl **ring** quad (`0x2C808080`: opaque FT4, neutral).
pub const SWIRL_RING_RGB: u32 = 0x0080_8080;
/// Colour word of a swirl **wall** quad (`0x2C404040`: the darker quad that
/// joins a column's far-z copy to the near ring).
pub const SWIRL_WALL_RGB: u32 = 0x0040_4040;

/// One band half of the swirl, projected and appended. `FUN_801D1A20`.
///
/// The submit is the tile shatter's mechanism again: a synthetic Legaia-TMD
/// object in the `_DAT_8007B85C + 0x5DC00` scratch block - group header
/// `count = 0x40`, `flags = 0x22` (dispatch kind 17, the flat-textured-quad
/// handler), `ilen = 6`, `mode = 0x2C` - handed to `FUN_80043390`. The **64**
/// primitives come from a 32-iteration loop writing two quads per column
/// pair (the historical "32 triangles per half" reading is falsified by the
/// packet: every primitive is a `POLY_FT4`):
///
/// * the **ring** quad `(p, p+1, p+3, p+4)` - inner/outer rim of column `c`
///   to inner/outer rim of column `c+1` - colour `0x2C808080`, and
/// * the **wall** quad `(p+2, p, p+5, p+3)` - the far-z copies joined to the
///   near inner rim - colour `0x2C404040`,
///
/// each with the texel pairs of its own vertices and the half's texture page
/// (`0x117` primary / `0x115` mirrored - the right and left 320-column halves
/// of the capture). The flag word `0x1880_8080` sets bit 27, so the dispatch
/// is **double-sided** ([`CullMode`]) - which is what lets the x-mirrored
/// half, whose winding is reversed, draw at all - though an edge-on quad
/// still drops. Its non-zero top byte ORs `1` into the dispatcher's third
/// argument (`0x800433C0..0x800433CC`), which selects the depth-cue bank:
/// the quad links at its **largest** corner depth (the byte's `0x10`), a quad
/// wholly off the screen is dropped (`FUN_80044798`), and `DPCS` moves the
/// colour toward the grey far colour `0x808080` by `IR0 = 1`, which leaves
/// both shipped colours as they are (`desc + 0xC` is zero, so the tint block
/// is skipped).
///
/// `band_z` is the band record's `+0x08` scalar **latched before the tick's
/// integration**: the per-band pose is identity rotation with translation
/// `(0, 0, band_z)` (`FUN_801D1888` at `0x801D1904..0x801D195C`), so the
/// scalar is the band's view depth, not a rotation angle - alternating rate
/// signs make alternating bands fly toward and away from the camera.
///
/// Past the late-phase frame the submit goes through `FUN_80029888` instead,
/// the shatter renderer ([`crate::shatter`]) with draw word `(clock - 0x3C) *
/// 4` ([`SwirlBandDraw::late_arg`]) and flag word `0x8180_8080`: each quad
/// is drawn as its two triangles `(v0, v1, v2)` / `(v1, v2, v3)`
/// (`FUN_80029DD8`), each spun about its own centroid by `RotMatrix(v << 4,
/// 0, v << 4)` and pushed out from the band's origin by `c * v >> 7`
/// ([`crate::shatter::ShatterObject::triangle`], the default arm: the call
/// stages no deformation row, so no burst offset and no flags - inference
/// from the call's arguments), and drawn with no `NCLIP`. The retail kernel
/// differential (`crates/retail-exec/tests/intro_swirl.rs`) compares the
/// early phase only.
///
/// PORT: FUN_801D1A20
/// REF: FUN_80029888 (the late-phase dispatcher; far colour + roll staging)
pub fn emit_swirl_band(
    mesh: &SwirlMesh,
    band_z: i32,
    draw: &SwirlBandDraw,
    prims: &mut Vec<ScreenPrim>,
) -> bool {
    let tr = GteVec3::new(0, 0, band_z);
    if let Some(n) = draw.late_arg {
        return emit_swirl_band_shattered(mesh, tr, n, draw, prims);
    }
    let rot = GteMat3::IDENTITY;
    let mut drawn = false;
    for seg in 0..swirl::COLUMNS - 1 {
        let p = draw.first_vertex + seg * swirl::VERTS_PER_COLUMN;
        for (idx, rgb) in [
            ([p, p + 1, p + 3, p + 4], SWIRL_RING_RGB),
            ([p + 2, p, p + 5, p + 3], SWIRL_WALL_RGB),
        ] {
            let pc: [ProjCorner; 4] = std::array::from_fn(|i| {
                let v = mesh.vertices[idx[i]];
                project_intro_corner(&rot, tr, (v.x, v.y, v.z))
            });
            let uv: [(u8, u8); 4] = std::array::from_fn(|i| {
                let (u, v) = mesh.texels[idx[i]];
                (u as u8, v as u8)
            });
            drawn |= push_ft4_quad(
                &pc,
                uv,
                0,
                draw.tpage as u16,
                rgb,
                false,
                CullMode::DoubleSided,
                // `0x1880_8080`: a non-zero top byte selects the depth-cue
                // bank with `IR0 = 1` toward the grey far colour, and its
                // `0x10` takes the largest corner depth.
                Some(DepthCue {
                    ir0: 1,
                    far: 0x80,
                    max_sz: true,
                }),
                prims,
            );
        }
    }
    drawn
}

/// The late-phase band half through the shatter renderer: every ring and
/// wall quad as its two triangles, each run through
/// [`crate::shatter::ShatterObject::triangle`] in the band's local frame and
/// projected at the band's depth, texels and colours as the early submit
/// carries them.
// REF: FUN_80029888, FUN_80029DD8
fn emit_swirl_band_shattered(
    mesh: &SwirlMesh,
    tr: GteVec3,
    late: i32,
    draw: &SwirlBandDraw,
    prims: &mut Vec<ScreenPrim>,
) -> bool {
    use crate::shatter::{ShatterObject, ShatterRow, SweepFrame};
    let row = ShatterRow::default();
    let mut obj = ShatterObject::new(late as u32 & 0xFFFF, &row, SweepFrame::MODEL);
    let mut drawn = false;
    for seg in 0..swirl::COLUMNS - 1 {
        let p = draw.first_vertex + seg * swirl::VERTS_PER_COLUMN;
        for (idx, rgb) in [
            ([p, p + 1, p + 3, p + 4], SWIRL_RING_RGB),
            ([p + 2, p, p + 5, p + 3], SWIRL_WALL_RGB),
        ] {
            for tri in [[0usize, 1, 2], [1, 2, 3]] {
                let local: [[i16; 3]; 3] = std::array::from_fn(|i| {
                    let v = mesh.vertices[idx[tri[i]]];
                    [v.x, v.y, v.z]
                });
                let moved = obj.triangle(local);
                let pc: [ProjCorner; 3] = std::array::from_fn(|i| {
                    let m = moved[i];
                    let q = |c: i32| c.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
                    project_intro_corner(&GteMat3::IDENTITY, tr, (q(m[0]), q(m[1]), q(m[2])))
                });
                let uv: [(u8, u8); 3] = std::array::from_fn(|i| {
                    let (u, v) = mesh.texels[idx[tri[i]]];
                    (u as u8, v as u8)
                });
                drawn |= push_ft4_quad(
                    &[pc[0], pc[1], pc[2], pc[2]],
                    [uv[0], uv[1], uv[2], uv[2]],
                    0,
                    draw.tpage as u16,
                    rgb,
                    false,
                    CullMode::DoubleSided,
                    Some(DepthCue {
                        ir0: 1,
                        far: 0x80,
                        max_sz: true,
                    }),
                    prims,
                );
            }
        }
    }
    drawn
}

/// The swirl mesh builder's trig tables, in the phase the packet's own texel
/// math pins.
///
/// The build samples `_DAT_8007B81C` for x and `_DAT_8007B7F8` for y over
/// entries `0..=2048` (half the 4096-entry space). The primary half's u is
/// `(x >> 4) + 0x20` into the **right**-half page `0x117` and the mirrored
/// half's `-0x61 - (x >> 4)` into the left-half page `0x115`; both stay
/// inside their 320-column capture halves only when x is non-negative over
/// the sampled range - so the x table is **sine-phased** (0 at entry 0) and
/// the y table cosine-phased, spanning the full ±0x760 the `v` bias needs.
/// [`BattleIntro::new`] therefore builds the mesh with this pair - the exact
/// PSX table via [`psx_sin`] - rather than with the host-supplied
/// [`swirl::SwirlTrig`], whose phase convention (shared with the particle
/// seeders' heading tables) is the transpose.
pub(super) struct SwirlTables;

impl swirl::SwirlTrig for SwirlTables {
    fn table_x(&mut self, entry: i32) -> i16 {
        psx_sin((entry & 0xFFF) as u16) as i16
    }
    fn table_y(&mut self, entry: i32) -> i16 {
        psx_cos((entry & 0xFFF) as u16) as i16
    }
}
