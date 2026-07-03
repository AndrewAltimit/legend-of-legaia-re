//! Untextured flat / gouraud colour meshes (vertex-colour render path).

use crate::{Tmd, legaia_prims};

use super::{pack_tsb_semi, rot_zyx};

/// A flat / gouraud, **untextured** mesh: per-vertex position + RGB colour,
/// no UVs or VRAM lookup. Built from a TMD's `F*`/`G*` primitives for the
/// engine's vertex-colour render path (the props whose prims carry colours
/// instead of texture coordinates, which the textured VRAM-mesh builder drops).
#[derive(Debug, Clone, Default)]
pub struct ColorMesh {
    /// Per-vertex object-local position (one entry per emitted corner).
    pub positions: Vec<[f32; 3]>,
    /// Per-vertex RGB colour (0..=255), index-aligned with `positions`.
    pub colors: Vec<[u8; 3]>,
    /// Triangle indices into `positions` (quads emitted as two triangles).
    pub indices: Vec<u32>,
    /// Per-vertex PSX blend word, index-aligned with `positions`: the group
    /// mode byte's ABE bit packed into bit 15 (the same
    /// [`TSB_SEMI_TRANSPARENT_BIT`] convention the textured path rides on
    /// TSB), ABR blend mode in bits 5..=6. Legaia's untextured prims carry
    /// no texpage/ABR field, so ABR is 0 (B/2+F/2, the PSX draw-env
    /// default); only the ABE enable varies. All corners of a prim share
    /// one word.
    ///
    /// [`TSB_SEMI_TRANSPARENT_BIT`]: super::TSB_SEMI_TRANSPARENT_BIT
    pub blend: Vec<u16>,
}

impl ColorMesh {
    /// `true` when there is nothing to draw.
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

/// Build a [`ColorMesh`] from a TMD's **untextured** primitives only (the
/// `F*`/`G*` flat / gouraud prims that carry a per-vertex colour block instead
/// of UVs - see [`legaia_prims::Prim::colors`]). Textured prims are skipped:
/// they belong to the VRAM-mesh path, so a caller can render the textured part
/// via [`tmd_to_vram_mesh_filtered`] and the untextured part via this without
/// double-drawing. Mirrors the walk / winding of [`tmd_to_vram_mesh_field_hybrid`]
/// (quad → `[0,1,2, 1,3,2]`), but emits a standalone colour mesh.
///
/// [`tmd_to_vram_mesh_filtered`]: super::tmd_to_vram_mesh_filtered
/// [`tmd_to_vram_mesh_field_hybrid`]: super::tmd_to_vram_mesh_field_hybrid
pub fn tmd_to_color_mesh(tmd: &Tmd, buf: &[u8]) -> ColorMesh {
    use crate::descriptor::Descriptor;

    let mut out = ColorMesh::default();
    for o in &tmd.objects {
        let object_vert_count = o.header.n_vert;
        let groups = legaia_prims::iter_groups_lenient(
            buf,
            o.primitives_byte_offset,
            o.primitives_byte_size,
        );
        for g in &groups {
            let desc = Descriptor::for_flags(g.header.flags);
            // Only untextured prims have a colour block; textured ones go
            // through the VRAM-mesh path.
            if desc.is_none_or(|d| d.packet_shape.is_textured()) {
                continue;
            }
            let blend_word = pack_tsb_semi(0, g.header.abe());
            for prim in &g.prims {
                let raw_idx = prim.vertex_indices();
                if raw_idx.is_empty() || raw_idx.iter().any(|&i| (i as u32) >= object_vert_count) {
                    continue;
                }
                let mut push_vert = |corner: usize, vidx: u16| -> u32 {
                    let v = &o.vertices[vidx as usize];
                    let i = out.positions.len() as u32;
                    out.positions.push([v.x as f32, v.y as f32, v.z as f32]);
                    out.colors
                        .push(prim.colors.get(corner).copied().unwrap_or([128, 128, 128]));
                    out.blend.push(blend_word);
                    i
                };
                match raw_idx.len() {
                    3 => {
                        let i0 = push_vert(0, raw_idx[0]);
                        let i1 = push_vert(1, raw_idx[1]);
                        let i2 = push_vert(2, raw_idx[2]);
                        out.indices.extend_from_slice(&[i0, i1, i2]);
                    }
                    4 => {
                        let i0 = push_vert(0, raw_idx[0]);
                        let i1 = push_vert(1, raw_idx[1]);
                        let i2 = push_vert(2, raw_idx[2]);
                        let i3 = push_vert(3, raw_idx[3]);
                        out.indices.extend_from_slice(&[i0, i1, i2, i1, i3, i2]);
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

/// Like [`tmd_to_color_mesh`] but applies the per-object **rigid transform**
/// (`R·v + T`, same convention as [`tmd_to_vram_mesh_posed_rot`]) before
/// emitting each vertex. The character field meshes are hybrids - textured
/// prims (head / shirt / hands) plus untextured F*/G* colour prims (pants /
/// sleeves) - so a posed character draw needs both halves: this one on the
/// colour pipeline, the textured half via [`tmd_to_vram_mesh_posed_rot`].
/// Objects past the end of `bone_offsets` render at their TMD-local rest
/// position (identity transform).
///
/// [`tmd_to_vram_mesh_posed_rot`]: super::tmd_to_vram_mesh_posed_rot
pub fn tmd_to_color_mesh_posed_rot(
    tmd: &Tmd,
    buf: &[u8],
    bone_offsets: &[([i16; 3], [i16; 3])],
) -> ColorMesh {
    use crate::descriptor::Descriptor;
    const A2R: f32 = std::f32::consts::TAU / 4096.0;

    let mut out = ColorMesh::default();
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
            let desc = Descriptor::for_flags(g.header.flags);
            if desc.is_none_or(|d| d.packet_shape.is_textured()) {
                continue;
            }
            let blend_word = pack_tsb_semi(0, g.header.abe());
            for prim in &g.prims {
                let raw_idx = prim.vertex_indices();
                if raw_idx.is_empty() || raw_idx.iter().any(|&i| (i as u32) >= object_vert_count) {
                    continue;
                }
                let mut push_vert = |corner: usize, vidx: u16| -> u32 {
                    let v = &o.vertices[vidx as usize];
                    let r = rot_zyx([v.x as f32, v.y as f32, v.z as f32], cx, sx, cy, sy, cz, sz);
                    let i = out.positions.len() as u32;
                    out.positions.push([
                        r[0] + bone_pos[0],
                        r[1] + bone_pos[1],
                        r[2] + bone_pos[2],
                    ]);
                    out.colors
                        .push(prim.colors.get(corner).copied().unwrap_or([128, 128, 128]));
                    out.blend.push(blend_word);
                    i
                };
                match raw_idx.len() {
                    3 => {
                        let i0 = push_vert(0, raw_idx[0]);
                        let i1 = push_vert(1, raw_idx[1]);
                        let i2 = push_vert(2, raw_idx[2]);
                        out.indices.extend_from_slice(&[i0, i1, i2]);
                    }
                    4 => {
                        let i0 = push_vert(0, raw_idx[0]);
                        let i1 = push_vert(1, raw_idx[1]);
                        let i2 = push_vert(2, raw_idx[2]);
                        let i3 = push_vert(3, raw_idx[3]);
                        out.indices.extend_from_slice(&[i0, i1, i2, i1, i3, i2]);
                    }
                    _ => {}
                }
            }
        }
    }
    out
}
