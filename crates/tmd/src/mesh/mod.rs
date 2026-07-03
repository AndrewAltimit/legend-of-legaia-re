//! TMD → triangulated mesh conversion.
//!
//! Produces engine-agnostic position + index buffers suitable for upload to a
//! GPU. Quads are split into two triangles using the standard PSX SDK winding
//! `(v0, v1, v2)` + `(v1, v3, v2)` - Sony's libgs draws GT4/FT4 as two
//! triangles that share the (v1, v2) diagonal.
//!
//! Out-of-range vertex indices and prims with no decoded indices (i.e. the
//! `legaia_prims::vertex_offset_bytes` lookup returned None) are skipped
//! silently. Validated against the full TMD corpus, where this hasn't been
//! observed in practice.

mod basic;
mod color;
mod textured;
mod vram;
mod vram_filtered;
mod vram_posed;

pub use basic::*;
pub use color::*;
pub use textured::*;
pub use vram::*;
pub use vram_filtered::*;
pub use vram_posed::*;

/// Bit 15 of the per-vertex TSB attribute carries the prim's
/// semi-transparency enable (the group mode byte's ABE bit; see
/// [`legaia_prims::GroupHeader::abe`]). A TMD TSB word only uses bits
/// 0..=8 (texture-page x/y, ABR blend mode, pixel depth), so bit 15 is
/// free for this engine-side packing. The engine-render VRAM-mesh shader
/// reads the bit per prim; consumers that decode the TSB
/// ([`legaia_prims::Prim::tpage_xy`]-style masked reads) are unaffected.
/// `legaia_engine_render::psx_blend::TSB_SEMI_TRANSPARENT_BIT` mirrors
/// this constant and is kept in lockstep.
pub const TSB_SEMI_TRANSPARENT_BIT: u16 = 0x8000;

/// Pack the prim semi-transparency enable into bit 15 of a TSB word (see
/// [`TSB_SEMI_TRANSPARENT_BIT`]). Clears the bit first so on-disc garbage
/// in the unused high bits can't leak through as a phantom enable.
pub fn pack_tsb_semi(tsb: u16, abe: bool) -> u16 {
    (tsb & !TSB_SEMI_TRANSPARENT_BIT) | if abe { TSB_SEMI_TRANSPARENT_BIT } else { 0 }
}

/// Rotate a vector by the PSX `Rz · Ry · Rx` Euler composition, given the
/// per-axis cos/sin. Mirrors the byte-for-byte order the retail engine applies
/// (`RotMatrixX` then `Y` then `Z`, i.e. the matrix product `Rz·Ry·Rx`) and the
/// visually-validated site animator (`monsters.html` `_assemble`). Shared by
/// vertex and normal transforms.
#[inline]
pub(crate) fn rot_zyx(
    v: [f32; 3],
    cx: f32,
    sx: f32,
    cy: f32,
    sy: f32,
    cz: f32,
    sz: f32,
) -> [f32; 3] {
    let (mut x, mut y, mut z) = (v[0], v[1], v[2]);
    // Rx
    let ny = y * cx - z * sx;
    let nz = y * sx + z * cx;
    y = ny;
    z = nz;
    // Ry
    let nx = x * cy + z * sy;
    let nz = -x * sy + z * cy;
    x = nx;
    z = nz;
    // Rz
    let nx = x * cz - y * sz;
    let ny = x * sz + y * cz;
    [nx, ny, z]
}

/// Build per-vertex normals from triangle geometry. Triangles whose three
/// vertices share an integer-quantized position with another triangle's
/// vertices contribute to a per-position normal bin; the per-vertex normal
/// is the normalised average of all face normals sharing that position.
///
/// Quantization uses the source TMD coordinate space (i32 of the f32
/// position) so two prims that reference the same vertex of the underlying
/// SVECTOR table land in the same bin. Returns the zero vector for
/// positions in singleton-face bins (which the renderer treats as "fall
/// back to screen-space derivatives").
pub(crate) fn compute_smooth_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    use std::collections::HashMap;
    type Key = (i32, i32, i32);
    let key_of = |p: &[f32; 3]| -> Key { (p[0] as i32, p[1] as i32, p[2] as i32) };
    let mut bins: HashMap<Key, [f32; 3]> = HashMap::new();
    for tri in indices.chunks_exact(3) {
        let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        if a >= positions.len() || b >= positions.len() || c >= positions.len() {
            continue;
        }
        let pa = positions[a];
        let pb = positions[b];
        let pc = positions[c];
        let ab = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
        let ac = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
        let n = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        // Triangles weight their face normal by area (length of unnormalised
        // cross product) - this is the standard angle-independent average
        // recommended by Max '99 ("Weights for Computing Vertex Normals from
        // Facet Normals"). Larger faces contribute more.
        for &idx in &[a, b, c] {
            let bin = bins.entry(key_of(&positions[idx])).or_insert([0.0; 3]);
            bin[0] += n[0];
            bin[1] += n[1];
            bin[2] += n[2];
        }
    }
    positions
        .iter()
        .map(|p| {
            let v = bins.get(&key_of(p)).copied().unwrap_or([0.0; 3]);
            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            if len > 1e-6 {
                [v[0] / len, v[1] / len, v[2] / len]
            } else {
                [0.0, 0.0, 0.0]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
