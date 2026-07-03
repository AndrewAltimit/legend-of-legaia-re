//! The plain position + index mesh (no UVs, no VRAM lookup).

use crate::{Tmd, legaia_prims};

/// Triangulated mesh ready for GPU upload.
#[derive(Debug, Clone)]
pub struct Mesh {
    /// Per-vertex `[x, y, z]` in TMD-native integer space (i16 promoted to f32
    /// without scaling). Caller-supplied transforms (perspective, view, model
    /// rotation) handle scale.
    pub positions: Vec<[f32; 3]>,
    /// Per-triangle vertex indices into `positions`. Always a multiple of 3.
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Axis-aligned bounding box `(min, max)` over [`Self::positions`].
    /// Returns `(zero, zero)` for empty meshes.
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

/// Build a triangulated mesh from a parsed TMD plus the original buffer
/// (needed to walk the per-object primitive section). Concatenates every
/// object's verts into one position list with per-object index offsets, so
/// caller can render the whole TMD as a single draw.
pub fn tmd_to_mesh(tmd: &Tmd, buf: &[u8]) -> Mesh {
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    let mut vert_base: u32 = 0;

    for o in &tmd.objects {
        let v_start = vert_base;
        for v in &o.vertices {
            positions.push([v.x as f32, v.y as f32, v.z as f32]);
        }
        let v_end = positions.len() as u32;
        let object_vert_count = v_end - v_start;

        let groups = legaia_prims::iter_groups_lenient(
            buf,
            o.primitives_byte_offset,
            o.primitives_byte_size,
        );

        for g in &groups {
            for prim in &g.prims {
                let idxs = prim.vertex_indices();
                if idxs.is_empty() {
                    continue;
                }
                if idxs.iter().any(|&i| (i as u32) >= object_vert_count) {
                    continue;
                }
                match idxs.len() {
                    3 => {
                        indices.push(v_start + idxs[0] as u32);
                        indices.push(v_start + idxs[1] as u32);
                        indices.push(v_start + idxs[2] as u32);
                    }
                    4 => {
                        // Standard PSX quad split - verts arrive as (v0, v1,
                        // v2, v3) where v3 is opposite v0; emit two triangles
                        // sharing the (v1, v2) diagonal.
                        let v0 = v_start + idxs[0] as u32;
                        let v1 = v_start + idxs[1] as u32;
                        let v2 = v_start + idxs[2] as u32;
                        let v3 = v_start + idxs[3] as u32;
                        indices.push(v0);
                        indices.push(v1);
                        indices.push(v2);
                        indices.push(v1);
                        indices.push(v3);
                        indices.push(v2);
                    }
                    _ => {}
                }
            }
        }

        vert_base = v_end;
    }

    Mesh { positions, indices }
}
