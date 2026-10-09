//! The kingdom overworld's draw order between the continent ground and the
//! fog sheets, as a depth policy both hosts can run.
//!
//! The PSX has no depth buffer: retail links every overworld primitive into
//! one ordering table and the GPU draws it back to front, so a primitive in
//! a nearer bucket covers everything linked behind it wherever the two
//! overlap. The comparison is once per *primitive*, by bucket, never per
//! pixel.
//!
//! # The two keys
//!
//! **Ground.** The continent is drawn by `FUN_801F89B8` (PROT 0901, the
//! world-map slot-B image; `jal`'d from `0x801F733C` at the end of that
//! image's decoration sweep `FUN_801F69D8`). One `POLY_FT4` per map cell.
//! Each column step `RTPT`s two new corners and keeps the previous step's
//! two `SZ` values at scratchpad `-0x2DC` / `-0x2D8` of its `0x1F800314`
//! base, so the four corners' depths are all at hand when it links:
//!
//! ```text
//! 801f8dc8  mfc2  t8, SZ1          ; this step's two corners
//! 801f8dcc  mfc2  s0, SZ2
//! 801f8dd0  lw    t9, -0x2dc(t6)   ; the previous step's two
//! 801f8dd4  lw    s1, -0x2d8(t6)
//!   ...                            ; t9 = max of the four (three sub/bgez)
//! 801f8e08  srl   t9, t9, 5
//! 801f8e0c  addi  t9, t9, 2
//! 801f8e10  sll   t9, t9, 2
//! 801f8e14  addu  t7, t9, t4       ; t4 = *0x1F8003F4, the OT base
//! 801f8e20  addi  t7, t7, 0x30     ; twelve more buckets
//! ```
//!
//! So a cell links at bucket `(max(SZ) >> 5) + 14` of the table the base
//! pointer names - no `>> shift` (the `0x1F8003A4` shift only feeds the
//! routine's unused far-bucket pointer at `0x801F89E8..0x801F89F8`).
//!
//! **Fog.** The half-sheet emitter `FUN_8003F86C` links at `(SZ - 0x10) >> 5`
//! of the same base on the overworld (`0x8003F978..0x8003F9D8`, see
//! `legaia_engine_core::fog_particles`).
//!
//! Both keys index the one table `*0x1F8003F4` points into, so they are on
//! one scale: a cell covers a sheet exactly when its bucket is the lower.
//! On `keikoku_chest_preload`'s walked table every fog half the port
//! reproduces sits at the bucket the fog formula gives from the port's own
//! projection, and all but a few percent of the continent cells at the
//! ground formula's, the rest one bucket off where the port's rounded `SZ`
//! crosses a bucket edge
//! (`crates/engine-core/tests/overworld_draw_order_retail_capture_disc.rs`).
//!
//! **Ties.** A later `AddPrim` into a bucket is drawn first. In every
//! bucket of that table holding both a sheet and a cell, the sheet comes
//! first in the chain - the fog pass links after the ground - so a cell
//! covers a sheet in its own bucket.
//!
//! # As a depth buffer
//!
//! The port keeps its depth buffer and hands each primitive a *flat* depth
//! instead of a per-pixel one: every ground cell and every fog sheet draws at
//! the depth of its bucket ([`bucket_sz`]), with the sheet a quarter bucket
//! behind its bucket's cells ([`FOG_TIE_SZ`]) so ties resolve the way the
//! chain does. With depth writes on for the ground, the depth buffer then
//! holds, per pixel, the lowest bucket of any cell covering it - which is
//! exactly the cell the ordering table draws last there - and a sheet passes
//! only where no nearer-bucket cell covers it. Per-pixel depth let a sheet
//! through wherever the sloping ridge's own pixel lay behind the sheet's
//! particle; the bucket key puts the whole cell at its nearest corner plus
//! fourteen buckets.
//!
//! The mesh shaders compute a cell's key from its four corners
//! ([`ground_flat_refs`] packs them per vertex): `OVERWORLD_FLAT_DEPTH_WGSL`
//! in `engine-render` and `overworldFlatDepth` in the play page's GLSL
//! (`site/js/webgl-shaders.js`). `legaia_engine_core::fog_particles::FogQuad::depth`
//! carries the sheet's side.

/// Bits an `SZ` is shifted right by to form a bucket index (`srl t9,t9,5` at
/// `0x801F8E08`; `srl t7,t7,5` in the fog emitter).
pub const OT_BUCKET_SHIFT: u32 = 5;

/// Width of one bucket in `SZ` units.
pub const OT_BUCKET_SZ: f32 = (1u32 << OT_BUCKET_SHIFT) as f32;

/// Buckets the ground cell's key sits behind its nearest corner: `+2`
/// (`addi t9,t9,2` at `0x801F8E0C`) and `+0x30` bytes = twelve more
/// (`addi t7,t7,0x30` at `0x801F8E20`).
pub const GROUND_OT_BIAS: u32 = 14;

/// The overworld fog's key bias (`(SZ - 0x10) >> 5`, `0x8003F978..84`).
pub const FOG_OT_BIAS_SZ: f32 = 0x10 as f32;

/// How far behind its bucket's representative depth a fog sheet is drawn,
/// in `SZ` units: a quarter bucket, so a sheet and a cell in the same bucket
/// never compare equal and the cell wins, as the chain order has it.
pub const FOG_TIE_SZ: f32 = OT_BUCKET_SZ / 4.0;

/// Stride of one heightfield cell in `WalkHeightfield` vertex order.
const CELL_VERTS: usize = 4;

/// Bucket a continent cell links at from its four corners' `SZ` - the key
/// `FUN_801F89B8` forms at `0x801F8DC8..0x801F8E20`.
///
/// REF: FUN_801F89B8
pub fn ground_ot_index(corner_sz: [u32; 4]) -> u32 {
    let max = corner_sz.into_iter().max().unwrap_or(0);
    (max >> OT_BUCKET_SHIFT) + GROUND_OT_BIAS
}

/// Bucket an overworld fog half links at from its particle's `SZ` - the key
/// `FUN_8003F86C` forms on the overworld arm.
pub fn fog_ot_index(sz: f32) -> u32 {
    ((sz - FOG_OT_BIAS_SZ).max(0.0) as u32) >> OT_BUCKET_SHIFT
}

/// The representative `SZ` of bucket `ot_index`: the centre of the depth
/// range whose fog halves link there (`(SZ - 0x10) >> 5 == ot_index` for
/// `SZ` in `32 k + 16 .. 32 k + 48`). Ground cells draw at it; fog sheets at
/// it plus [`FOG_TIE_SZ`].
pub fn bucket_sz(ot_index: u32) -> f32 {
    ot_index as f32 * OT_BUCKET_SZ + FOG_OT_BIAS_SZ + OT_BUCKET_SZ / 2.0
}

/// The flat `SZ` a fog sheet in bucket `ot_index` is drawn at.
pub fn fog_flat_sz(ot_index: u32) -> f32 {
    bucket_sz(ot_index) + FOG_TIE_SZ
}

/// The flat `SZ` a ground cell with these corner depths is drawn at.
pub fn ground_flat_sz(corner_sz: [u32; 4]) -> f32 {
    bucket_sz(ground_ot_index(corner_sz))
}

/// `z = a * w + b` for every point through a perspective matrix whose depth
/// row is `(0, 0, a, b)` over its `w` row `(0, 0, 1, 0)` (the retail
/// projection, `legaia_engine_vm::psx_camera::psx_projection`), composed with
/// any affine view and model. Read off a column-major `clip = m * p`: the
/// column with the largest `w` weight gives `a`, the translation column `b`.
/// The shaders derive the pair from their `mvp` the same way, so a flat
/// depth computed on either side lands on the same normalised depth.
pub fn depth_affine(m: &[f32; 16]) -> (f32, f32) {
    let j = (0..3)
        .max_by(|&x, &y| m[4 * x + 3].abs().total_cmp(&m[4 * y + 3].abs()))
        .unwrap_or(2);
    let wj = m[4 * j + 3];
    let a = if wj.abs() > f32::EPSILON {
        m[4 * j + 2] / wj
    } else {
        0.0
    };
    (a, m[14] - a * m[15])
}

/// Normalised depth (`z / w`) of a point at clip-space `w` through `m`.
pub fn ndc_at_w(m: &[f32; 16], w: f32) -> f32 {
    let (a, b) = depth_affine(m);
    a + b / w
}

/// Per-vertex flat-depth references for a heightfield laid out as
/// `WalkHeightfield` builds it - four vertices per cell, in the order
/// `(x0, z0) (x1, z0) (x0, z1) (x1, z1)` - given its drawn positions
/// (`legaia_engine_core::field_ground::render_positions`). Each vertex carries its
/// cell's `[x0, z0, x1, z1, y00, y10, y01, y11]`, from which the mesh
/// shaders re-project all four corners and take the cell's key.
///
/// A trailing partial cell (which a well-formed grid never has) gets zeros,
/// which the shaders read as "no flat depth" (`x1 <= x0`).
pub fn ground_flat_refs(positions: &[[f32; 3]]) -> Vec<[f32; 8]> {
    let mut out = vec![[0.0; 8]; positions.len()];
    for (cell, refs) in positions
        .as_chunks::<CELL_VERTS>()
        .0
        .iter()
        .zip(out.as_chunks_mut::<CELL_VERTS>().0)
    {
        let r = [
            cell[0][0], cell[0][2], cell[3][0], cell[3][2], cell[0][1], cell[1][1], cell[2][1],
            cell[3][1],
        ];
        refs.fill(r);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ground_key_is_nearest_corner_plus_fourteen_buckets() {
        assert_eq!(ground_ot_index([0, 0, 0, 0]), 14);
        // The *farthest* corner picks the key - max, not min or mean.
        assert_eq!(
            ground_ot_index([0x400, 0x1000, 0x800, 0x7FF]),
            (0x1000 >> 5) + 14
        );
        assert_eq!(ground_ot_index([31, 31, 31, 31]), 14);
        assert_eq!(ground_ot_index([32, 0, 0, 0]), 15);
    }

    #[test]
    fn fog_key_is_sz_less_sixteen() {
        assert_eq!(fog_ot_index(16.0), 0);
        assert_eq!(fog_ot_index(47.9), 0);
        assert_eq!(fog_ot_index(48.0), 1);
        assert_eq!(fog_ot_index(0x1000 as f32), (0x1000 - 0x10) >> 5);
        assert_eq!(fog_ot_index(0.0), 0);
    }

    #[test]
    fn a_bucket_depth_sits_inside_its_own_fog_range() {
        for k in [0u32, 1, 100, 900] {
            let c = bucket_sz(k);
            assert_eq!(fog_ot_index(c), k);
            assert_eq!(fog_ot_index(fog_flat_sz(k)), k);
            assert!(fog_flat_sz(k) > c);
            assert!(fog_flat_sz(k) < bucket_sz(k + 1));
        }
    }

    #[test]
    fn a_cell_covers_a_sheet_only_from_a_lower_bucket_or_its_own() {
        // Sheet in bucket 100; a cell whose farthest corner puts it in 99,
        // 100 and 101.
        let sheet = fog_flat_sz(100);
        for (cell_bucket, covers) in [(99u32, true), (100, true), (101, false)] {
            let max_sz = (cell_bucket - GROUND_OT_BIAS) << OT_BUCKET_SHIFT;
            let g = ground_flat_sz([max_sz; 4]);
            assert_eq!(g < sheet, covers, "cell bucket {cell_bucket}");
        }
    }

    #[test]
    fn depth_affine_reads_the_projection_through_any_view() {
        use legaia_engine_vm::psx_camera::{FieldCameraView, mat4_mul, mat4_scale};
        let view = FieldCameraView {
            focus: [100.0, 0.0, -50.0],
            pitch: 0.4,
            yaw: 1.1,
            roll: 0.0,
            h: 368.0,
            tr_eye: [0.0, -20.0, 900.0],
        };
        for scale in [1.0f32, 6.0] {
            let m = mat4_mul(&view.vp(4.0 / 3.0), &mat4_scale(scale));
            for p in [
                [0.0f32, 0.0, 0.0],
                [300.0, -40.0, 800.0],
                [-120.0, 10.0, -600.0],
            ] {
                let v = [p[0], p[1], p[2], 1.0];
                let row = |r: usize| (0..4).map(|c| m[4 * c + r] * v[c]).sum::<f32>();
                let (z, w) = (row(2), row(3));
                if w <= 1.0 {
                    continue;
                }
                let got = ndc_at_w(&m, w);
                assert!(
                    (got - z / w).abs() < 1e-4,
                    "scale {scale} p {p:?}: {got} vs {}",
                    z / w
                );
            }
        }
    }

    #[test]
    fn flat_refs_carry_each_cells_four_corners() {
        let pos = [
            [0.0, -1.0, 0.0],
            [128.0, -2.0, 0.0],
            [0.0, -3.0, 128.0],
            [128.0, -4.0, 128.0],
            [128.0, 5.0, 0.0],
            [256.0, 6.0, 0.0],
            [128.0, 7.0, 128.0],
            [256.0, 8.0, 128.0],
        ];
        let r = ground_flat_refs(&pos);
        assert_eq!(r.len(), 8);
        for v in &r[..4] {
            assert_eq!(*v, [0.0, 0.0, 128.0, 128.0, -1.0, -2.0, -3.0, -4.0]);
        }
        for v in &r[4..] {
            assert_eq!(*v, [128.0, 0.0, 256.0, 128.0, 5.0, 6.0, 7.0, 8.0]);
        }
        assert_eq!(ground_flat_refs(&pos[..6])[4], [0.0; 8]);
    }
}
