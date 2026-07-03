//! Single-binding textured mesh (per-corner UVs, one texture page).

use crate::{Tmd, legaia_prims};

/// Triangulated mesh with per-vertex UVs.
///
/// Verts are duplicated per prim-corner (no shared verts between prims), so
/// `positions[i]`, `uvs[i]` always belong together. UVs are floats in `[0, 1)`
/// addressing a single texture page (caller's responsibility to bind the
/// right TIM). The PSX UV bytes are normalized by 256 - the texture page is
/// 256 pixels wide regardless of the actual TIM dimensions.
#[derive(Debug, Clone)]
pub struct TexturedMesh {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Per-triangle vertex indices into `positions`/`uvs` (always paired).
    pub indices: Vec<u32>,
    /// Texture-page byte for the first textured prim found, for caller use
    /// (which texture to bind). Decode with [`tpage_xy`](legaia_prims::Prim::tpage_xy).
    /// `0` if no textured prim was found.
    pub tpage_tsb: u16,
    /// CLUT base for the first textured prim found.
    pub clut_cba: u16,
}

impl TexturedMesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn aabb(&self) -> ([f32; 3], [f32; 3]) {
        if self.positions.is_empty() {
            return ([0.0; 3], [0.0; 3]);
        }
        let mut lo = self.positions[0];
        let mut hi = self.positions[0];
        for p in &self.positions[1..] {
            for i in 0..3 {
                if p[i] < lo[i] {
                    lo[i] = p[i];
                }
                if p[i] > hi[i] {
                    hi[i] = p[i];
                }
            }
        }
        (lo, hi)
    }
}

/// Build a textured mesh from a parsed TMD. Each prim contributes its own
/// fresh (pos, uv) verts so per-corner UVs are preserved. Quads are split
/// the same way as [`tmd_to_mesh`].
///
/// [`tmd_to_mesh`]: super::tmd_to_mesh
pub fn tmd_to_textured_mesh(tmd: &Tmd, buf: &[u8]) -> TexturedMesh {
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    let mut first_tsb = 0u16;
    let mut first_cba = 0u16;

    for o in &tmd.objects {
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
                if first_tsb == 0 && prim.tsb != 0 {
                    first_tsb = prim.tsb;
                    first_cba = prim.cba;
                }
                // UVs may be empty (untextured prim) - fall back to zeros.
                let uv_at = |i: usize| -> [f32; 2] {
                    prim.uvs
                        .get(i)
                        .map(|(u, v)| [*u as f32 / 256.0, *v as f32 / 256.0])
                        .unwrap_or([0.0, 0.0])
                };
                let push_vert = |positions: &mut Vec<[f32; 3]>,
                                 uvs: &mut Vec<[f32; 2]>,
                                 vidx: u16,
                                 uv_idx: usize|
                 -> u32 {
                    let v = &o.vertices[vidx as usize];
                    let i = positions.len() as u32;
                    positions.push([v.x as f32, v.y as f32, v.z as f32]);
                    uvs.push(uv_at(uv_idx));
                    i
                };
                match raw_idx.len() {
                    3 => {
                        let i0 = push_vert(&mut positions, &mut uvs, raw_idx[0], 0);
                        let i1 = push_vert(&mut positions, &mut uvs, raw_idx[1], 1);
                        let i2 = push_vert(&mut positions, &mut uvs, raw_idx[2], 2);
                        indices.extend_from_slice(&[i0, i1, i2]);
                    }
                    4 => {
                        // Quad → two triangles sharing (v1, v2) diagonal,
                        // matching tmd_to_mesh.
                        let i0 = push_vert(&mut positions, &mut uvs, raw_idx[0], 0);
                        let i1 = push_vert(&mut positions, &mut uvs, raw_idx[1], 1);
                        let i2 = push_vert(&mut positions, &mut uvs, raw_idx[2], 2);
                        let i3 = push_vert(&mut positions, &mut uvs, raw_idx[3], 3);
                        indices.extend_from_slice(&[i0, i1, i2, i1, i3, i2]);
                    }
                    _ => {}
                }
            }
        }
    }

    TexturedMesh {
        positions,
        uvs,
        indices,
        tpage_tsb: first_tsb,
        clut_cba: first_cba,
    }
}
