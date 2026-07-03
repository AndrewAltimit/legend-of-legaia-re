//! Per-object (per-bone) posed VRAM-mesh builders.

use crate::{Tmd, legaia_prims};

use super::{compute_smooth_normals, pack_tsb_semi, rot_zyx};

/// Like [`tmd_to_vram_mesh`] but applies per-object (per-bone) pose offsets
/// before emitting vertices. Each element of `bone_offsets` is a `(pos, rot)`
/// pair sourced from [`legaia_anm::PoseFrame::bone_outputs`] for the
/// corresponding TMD object index. Only the translation (`pos`) is applied;
/// rotation requires full GTE-matrix math and is deferred.
///
/// If `bone_offsets` is shorter than the TMD's object count, the remaining
/// objects are rendered at their default positions (no pose applied).
///
/// [`tmd_to_vram_mesh`]: super::tmd_to_vram_mesh
pub fn tmd_to_vram_mesh_posed(
    tmd: &Tmd,
    buf: &[u8],
    bone_offsets: &[([i16; 3], [i16; 3])],
) -> super::VramMesh {
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut cba_tsb = Vec::new();
    let mut indices = Vec::new();

    for (o_idx, o) in tmd.objects.iter().enumerate() {
        let bone_pos: [f32; 3] = bone_offsets
            .get(o_idx)
            .map(|(p, _r)| [p[0] as f32, p[1] as f32, p[2] as f32])
            .unwrap_or([0.0; 3]);

        let object_vert_count = o.header.n_vert;
        let groups = legaia_prims::iter_groups_lenient(
            buf,
            o.primitives_byte_offset,
            o.primitives_byte_size,
        );

        for g in &groups {
            for prim in &g.prims {
                let raw_idx = prim.vertex_indices();
                if raw_idx.is_empty() || raw_idx.iter().any(|&i| (i as u32) >= object_vert_count) {
                    continue;
                }
                if prim.uvs.is_empty() {
                    continue;
                }
                let ct = [prim.cba, pack_tsb_semi(prim.tsb, g.header.abe())];
                let mut push_vert = |vidx: u16, uv_idx: usize| -> u32 {
                    let v = &o.vertices[vidx as usize];
                    let i = positions.len() as u32;
                    positions.push([
                        v.x as f32 + bone_pos[0],
                        v.y as f32 + bone_pos[1],
                        v.z as f32 + bone_pos[2],
                    ]);
                    let (u8v, v8v) = prim.uvs.get(uv_idx).copied().unwrap_or((0, 0));
                    uvs.push([u8v, v8v]);
                    cba_tsb.push(ct);
                    i
                };
                match raw_idx.len() {
                    3 => {
                        let i0 = push_vert(raw_idx[0], 0);
                        let i1 = push_vert(raw_idx[1], 1);
                        let i2 = push_vert(raw_idx[2], 2);
                        indices.extend_from_slice(&[i0, i1, i2]);
                    }
                    4 => {
                        let i0 = push_vert(raw_idx[0], 0);
                        let i1 = push_vert(raw_idx[1], 1);
                        let i2 = push_vert(raw_idx[2], 2);
                        let i3 = push_vert(raw_idx[3], 3);
                        indices.extend_from_slice(&[i0, i1, i2, i1, i3, i2]);
                    }
                    _ => {}
                }
            }
        }
    }

    let normals = compute_smooth_normals(&positions, &indices);
    super::VramMesh {
        positions,
        uvs,
        cba_tsb,
        indices,
        normals,
    }
}

/// Like [`tmd_to_vram_mesh_posed`] but applies the full per-object **rigid
/// transform** (rotate-then-translate, `R·v + T`) instead of translation only.
///
/// Each element of `bone_offsets` is a `(pos, rot)` pair for the corresponding
/// TMD object index; `rot` holds three PSX 12-bit Euler angles (`4096` = a full
/// turn) on the X/Y/Z axes, composed `Rz·Ry·Rx` about the object's local
/// origin, then offset by `pos`. This matches the retail per-object pose
/// assembly (`FUN_8004998C` → `RotMatrixX/Y/Z`) and the site's monster /
/// character animators. Objects past the end of `bone_offsets` render at their
/// TMD-local rest position (identity transform).
///
/// Normals are recomputed from the posed positions (same as
/// [`tmd_to_vram_mesh_posed`]), so the rotation propagates to lighting without a
/// separate normal transform.
pub fn tmd_to_vram_mesh_posed_rot(
    tmd: &Tmd,
    buf: &[u8],
    bone_offsets: &[([i16; 3], [i16; 3])],
) -> super::VramMesh {
    const A2R: f32 = std::f32::consts::TAU / 4096.0;
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut cba_tsb = Vec::new();
    let mut indices = Vec::new();

    for (o_idx, o) in tmd.objects.iter().enumerate() {
        let (bone_pos, trig) = match bone_offsets.get(o_idx) {
            Some((p, r)) => {
                let (sx, cx) = (r[0] as f32 * A2R).sin_cos();
                let (sy, cy) = (r[1] as f32 * A2R).sin_cos();
                let (sz, cz) = (r[2] as f32 * A2R).sin_cos();
                (
                    [p[0] as f32, p[1] as f32, p[2] as f32],
                    [cx, sx, cy, sy, cz, sz],
                )
            }
            None => ([0.0; 3], [1.0, 0.0, 1.0, 0.0, 1.0, 0.0]),
        };
        let [cx, sx, cy, sy, cz, sz] = trig;

        let object_vert_count = o.header.n_vert;
        let groups = legaia_prims::iter_groups_lenient(
            buf,
            o.primitives_byte_offset,
            o.primitives_byte_size,
        );

        for g in &groups {
            for prim in &g.prims {
                let raw_idx = prim.vertex_indices();
                if raw_idx.is_empty() || raw_idx.iter().any(|&i| (i as u32) >= object_vert_count) {
                    continue;
                }
                if prim.uvs.is_empty() {
                    continue;
                }
                let ct = [prim.cba, pack_tsb_semi(prim.tsb, g.header.abe())];
                let mut push_vert = |vidx: u16, uv_idx: usize| -> u32 {
                    let v = &o.vertices[vidx as usize];
                    let r = rot_zyx([v.x as f32, v.y as f32, v.z as f32], cx, sx, cy, sy, cz, sz);
                    let i = positions.len() as u32;
                    positions.push([r[0] + bone_pos[0], r[1] + bone_pos[1], r[2] + bone_pos[2]]);
                    let (u8v, v8v) = prim.uvs.get(uv_idx).copied().unwrap_or((0, 0));
                    uvs.push([u8v, v8v]);
                    cba_tsb.push(ct);
                    i
                };
                match raw_idx.len() {
                    3 => {
                        let i0 = push_vert(raw_idx[0], 0);
                        let i1 = push_vert(raw_idx[1], 1);
                        let i2 = push_vert(raw_idx[2], 2);
                        indices.extend_from_slice(&[i0, i1, i2]);
                    }
                    4 => {
                        let i0 = push_vert(raw_idx[0], 0);
                        let i1 = push_vert(raw_idx[1], 1);
                        let i2 = push_vert(raw_idx[2], 2);
                        let i3 = push_vert(raw_idx[3], 3);
                        indices.extend_from_slice(&[i0, i1, i2, i1, i3, i2]);
                    }
                    _ => {}
                }
            }
        }
    }

    let normals = compute_smooth_normals(&positions, &indices);
    super::VramMesh {
        positions,
        uvs,
        cba_tsb,
        indices,
        normals,
    }
}
