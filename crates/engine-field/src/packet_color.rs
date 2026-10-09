//! Per-vertex **packet colour** streams for the hybrid textured +
//! vertex-colour mesh convention shared by the browser renderer and the
//! `.glb` exporters.
//!
//! Retail's field/battle lighting is the PSX GPU's texture blend, not a light
//! source: a textured prim draws as `texel * colour / 128` where `colour` is
//! the TMD prim's baked colour word (see `legaia_tmd::mesh::VramMesh::colors`
//! and the native renderer's `psx_modulate`). An untextured prim is filled
//! with that colour word directly.
//!
//! Consumers get both halves through one four-byte-per-vertex stream (the
//! WebGL `a_flat_rgba` attribute, the glb baker's `flat_rgba` side channel):
//!
//! | byte | meaning |
//! |---|---|
//! | 0..=2 | the prim's packet colour, `0x80` = neutral modulation |
//! | 3 | `255` = textured (sample VRAM and modulate), `0` = untextured |
//!
//! A mesh that uploads no such stream reads the context-global attribute
//! constant, which the renderers set to the neutral `0x80` triple, so an
//! un-coloured draw is `texel * 1.0` rather than `texel * 2.0`.

use legaia_tmd::mesh::{VertexShading, VramMesh};

/// The neutral modulation byte (`0x80`): `texel * 128 / 128 == texel`.
pub const NEUTRAL: u8 = legaia_tmd::legaia_prims::MODULATION_NEUTRAL;

/// `[r, g, b, 255]` per vertex for a purely **textured** mesh - the prim
/// colour words straight off the mesh.
///
/// Returns empty for an empty mesh so callers keep their "no stream, use the
/// constant" path.
pub fn textured(mesh: &VramMesh) -> Vec<u8> {
    let mut out = Vec::with_capacity(mesh.colors.len() * 4);
    for c in &mesh.colors {
        out.extend_from_slice(&[c[0], c[1], c[2], 255]);
    }
    out
}

/// `[r, g, b, flag]` per vertex for a **hybrid** mesh built by
/// [`legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid`].
///
/// The two halves take their colour from different places, which is the whole
/// reason this helper exists: an untextured vert's colour is its *fill*, and
/// `VertexShading::colors` is the only place it is surfaced; a textured vert's
/// colour is its *modulation*, and `VertexShading` deliberately reports white
/// there, so it has to come from `VramMesh::colors`. Reading one array for
/// both is what left the browser's textured path with nothing to modulate by.
pub fn hybrid(mesh: &VramMesh, shading: &VertexShading) -> Vec<u8> {
    let mut out = Vec::with_capacity(shading.colors.len() * 4);
    for (i, (c, &t)) in shading
        .colors
        .iter()
        .zip(shading.textured.iter())
        .enumerate()
    {
        if t != 0 {
            let m = mesh.colors.get(i).copied().unwrap_or([NEUTRAL; 3]);
            out.extend_from_slice(&[m[0], m[1], m[2], 255]);
        } else {
            out.extend_from_slice(&[c[0], c[1], c[2], 0]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh_with_colors(colors: Vec<[u8; 3]>) -> VramMesh {
        let n = colors.len();
        VramMesh {
            positions: vec![[0.0; 3]; n],
            uvs: vec![[0, 0]; n],
            cba_tsb: vec![[0, 0]; n],
            indices: Vec::new(),
            normals: vec![[0.0; 3]; n],
            colors,
        }
    }

    #[test]
    fn textured_stream_carries_the_packet_colour() {
        let m = mesh_with_colors(vec![[0x40, 0x50, 0x60], [0xFF, 0x00, 0x80]]);
        assert_eq!(
            textured(&m),
            vec![0x40, 0x50, 0x60, 255, 0xFF, 0x00, 0x80, 255]
        );
    }

    /// The regression this module exists for: a textured vert must NOT come
    /// back white. `VertexShading::colors` reports white there by design, so a
    /// helper that read it for both halves would emit `[255,255,255,255]` and
    /// the shader would modulate by ~2.0.
    #[test]
    fn hybrid_textured_verts_take_the_mesh_colour_not_white() {
        let m = mesh_with_colors(vec![[0x30, 0x60, 0x90], [0x11, 0x22, 0x33]]);
        let s = VertexShading {
            colors: vec![[255, 255, 255], [0x77, 0x88, 0x99]],
            textured: vec![1, 0],
        };
        let out = hybrid(&m, &s);
        // Textured vert: the mesh's modulation word, flag 255.
        assert_eq!(&out[0..4], &[0x30, 0x60, 0x90, 255]);
        assert_ne!(&out[0..3], &[255, 255, 255]);
        // Untextured vert: the shading fill colour, flag 0.
        assert_eq!(&out[4..8], &[0x77, 0x88, 0x99, 0]);
    }

    #[test]
    fn hybrid_falls_back_to_neutral_when_the_mesh_colour_is_missing() {
        let m = mesh_with_colors(Vec::new());
        let s = VertexShading {
            colors: vec![[255, 255, 255]],
            textured: vec![1],
        };
        assert_eq!(hybrid(&m, &s), vec![NEUTRAL, NEUTRAL, NEUTRAL, 255]);
    }
}
