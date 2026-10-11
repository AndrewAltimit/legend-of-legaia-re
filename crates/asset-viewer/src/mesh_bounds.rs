//! The per-actor uploaded mesh with its local bounds, and the AABB math that
//! sizes the camera frustum.

pub(crate) struct WorldActorMesh {
    pub(crate) mesh: legaia_engine_render::UploadedVramMesh,
    /// AABB of the local TMD geometry (pre-transform). Used to size the
    /// camera frustum.
    pub(crate) aabb_lo: [f32; 3],
    pub(crate) aabb_hi: [f32; 3],
}

pub(crate) fn mesh_aabb(positions: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    if positions.is_empty() {
        return ([0.0; 3], [0.0; 3]);
    }
    let mut lo = positions[0];
    let mut hi = positions[0];
    for p in &positions[1..] {
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
