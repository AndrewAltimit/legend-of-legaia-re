//! VRAM-aware textured mesh + its builders (per-vertex CBA/TSB, in-shader
//! page + CLUT lookup).

use crate::{Tmd, legaia_prims};

use super::{compute_smooth_normals, pack_tsb_semi};

/// VRAM-aware textured mesh: per-vertex `(u, v)` and per-vertex `(cba, tsb)`
/// PSX VRAM addresses. Built by [`tmd_to_vram_mesh`]; consumed by the
/// engine-render VRAM-mesh pipeline, which does the page+CLUT lookup in
/// the fragment shader and so handles meshes that sample multiple texture
/// pages and palettes correctly (the single-binding [`TexturedMesh`] path
/// does not).
///
/// [`TexturedMesh`]: super::TexturedMesh
#[derive(Debug, Clone)]
pub struct VramMesh {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[u8; 2]>,
    /// Per-vertex `[cba, tsb]`. The TSB half additionally carries the prim's
    /// semi-transparency enable in bit 15 (see [`TSB_SEMI_TRANSPARENT_BIT`]).
    ///
    /// [`TSB_SEMI_TRANSPARENT_BIT`]: super::TSB_SEMI_TRANSPARENT_BIT
    pub cba_tsb: Vec<[u16; 2]>,
    pub indices: Vec<u32>,
    /// Per-vertex normals, one per entry in `positions`. Computed at
    /// mesh-build time by accumulating face normals into per-position bins
    /// and normalising - this gives smooth shading for connected surfaces
    /// without needing the TMD per-prim normal-index byte offset (which is
    /// still unreversed for Legaia's six prim modes; see
    /// [`legaia_prims::vertex_offset_bytes`] for the parallel case).
    ///
    /// `[0.0, 0.0, 0.0]` is a sentinel that the renderer should fall back to
    /// screen-space derivative normals for; this happens for degenerate or
    /// untextured prims that don't contribute to the position bins.
    pub normals: Vec<[f32; 3]>,
    /// Per-vertex `[R, G, B]` **texture-modulation** colour, one per entry in
    /// `positions` - the prim's baked colour word (see
    /// [`legaia_prims::Prim::colors`]).
    ///
    /// This is retail's field lighting. The PSX GPU blends a textured prim as
    /// `texel * colour / 128`: `0x80` is neutral, below darkens, above
    /// brightens (up to nearly 2x at `0xFF`). The renderer must apply it -
    /// dropping it flattens the scene to the raw texel and loses both tails of
    /// the contrast. Lit-row prims, which carry no colour word, are
    /// [`legaia_prims::MODULATION_NEUTRAL`] here.
    pub colors: Vec<[u8; 3]>,
}

/// The corner colour a mesh builder should emit for `prim`'s `corner`-th
/// vertex. Falls back to the prim's first colour (flat prims store one word)
/// and then to [`legaia_prims::MODULATION_NEUTRAL`], so a prim whose colour
/// block was truncated still draws its raw texel rather than black.
pub(crate) fn prim_color(prim: &legaia_prims::Prim, corner: usize) -> [u8; 3] {
    prim.colors
        .get(corner)
        .or_else(|| prim.colors.first())
        .copied()
        .unwrap_or([legaia_prims::MODULATION_NEUTRAL; 3])
}

impl VramMesh {
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

/// Build a VRAM mesh from a parsed TMD. Each prim contributes its own
/// fresh per-corner verts (so per-corner UVs and per-prim CBA/TSB are
/// preserved exactly). Quads split the same way as [`tmd_to_mesh`].
///
/// Untextured prims (no UVs decoded) are skipped - they wouldn't sample
/// anything meaningful from VRAM, and emitting them would draw black /
/// transparent triangles that obscure other geometry.
///
/// [`tmd_to_mesh`]: super::tmd_to_mesh
pub fn tmd_to_vram_mesh(tmd: &Tmd, buf: &[u8]) -> VramMesh {
    tmd_to_vram_mesh_with_object_ids(tmd, buf).0
}

/// Same as [`tmd_to_vram_mesh`] but also returns a per-vertex **object id**
/// (the TMD object / body-part index each emitted vertex came from), parallel
/// to `mesh.positions`. Animated monster meshes use this to apply a per-object
/// transform per frame (see [`legaia_asset::monster_archive::MonsterAnimation`]).
/// `tmd_to_vram_mesh` is this function's mesh half, so the two never drift.
pub fn tmd_to_vram_mesh_with_object_ids(tmd: &Tmd, buf: &[u8]) -> (VramMesh, Vec<u32>) {
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut cba_tsb = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    let mut object_ids = Vec::new();

    for (o_idx, o) in tmd.objects.iter().enumerate() {
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
                    positions.push([v.x as f32, v.y as f32, v.z as f32]);
                    let (u8v, v8v) = prim.uvs.get(uv_idx).copied().unwrap_or((0, 0));
                    uvs.push([u8v, v8v]);
                    cba_tsb.push(ct);
                    colors.push(prim_color(prim, uv_idx));
                    object_ids.push(o_idx as u32);
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
    (
        VramMesh {
            positions,
            uvs,
            cba_tsb,
            indices,
            normals,
            colors,
        },
        object_ids,
    )
}

/// Like [`tmd_to_vram_mesh_with_object_ids`] but also includes untextured
/// primitives, emitting them with sentinel `(0, 0)` UVs and per-prim
/// `(cba, tsb)` (typically `(0, 0)` for flat-shaded prims). Use for character
/// meshes where the bulk of body parts are flat-shaded - the standard
/// extractor drops those, leaving only a few textured fragments. Consumers
/// can decide how to render the sentinel-UV verts (e.g., solid-shaded
/// fallback in the fragment shader).
pub fn tmd_to_vram_mesh_with_object_ids_lenient(tmd: &Tmd, buf: &[u8]) -> (VramMesh, Vec<u32>) {
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut cba_tsb = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    let mut object_ids = Vec::new();

    for (o_idx, o) in tmd.objects.iter().enumerate() {
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
                let ct = [prim.cba, pack_tsb_semi(prim.tsb, g.header.abe())];
                let mut push_vert = |vidx: u16, uv_idx: usize| -> u32 {
                    let v = &o.vertices[vidx as usize];
                    let i = positions.len() as u32;
                    positions.push([v.x as f32, v.y as f32, v.z as f32]);
                    let (u8v, v8v) = prim.uvs.get(uv_idx).copied().unwrap_or((0, 0));
                    uvs.push([u8v, v8v]);
                    cba_tsb.push(ct);
                    colors.push(prim_color(prim, uv_idx));
                    object_ids.push(o_idx as u32);
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
    (
        VramMesh {
            positions,
            uvs,
            cba_tsb,
            indices,
            normals,
            colors,
        },
        object_ids,
    )
}

/// Per-vertex shading attributes for the **field-character hybrid render**.
///
/// Parallel (index-aligned) to the vertex arrays of
/// [`tmd_to_vram_mesh_with_object_ids_lenient`]: one entry per emitted vertex.
/// Field-form player meshes mix textured prims (the face / skin / clothing
/// that sample the PROT 0874 §2 atlas) with **untextured** flat / gouraud prims
/// (the bulk of the body) that carry per-vertex RGB in the TMD, not UVs. The
/// textured renderer alone discards the untextured prims (their `(cba, tsb)` is
/// `(0, 0)`, so they sample empty VRAM → transparent), leaving holes. This
/// surfaces the untextured prims' colours so a hybrid shader can fill them.
#[derive(Debug, Clone, Default)]
pub struct VertexShading {
    /// Per-vertex RGB (0..=255). Meaningful only where `textured[i] == 0`;
    /// `[255, 255, 255]` for textured verts (unused by the shader there).
    pub colors: Vec<[u8; 3]>,
    /// Per-vertex flag: `1` if the prim is textured (sample VRAM), `0` if it
    /// is untextured (use `colors[i]`).
    pub textured: Vec<u8>,
}

/// Like [`tmd_to_vram_mesh_with_object_ids_lenient`] but also returns the
/// per-vertex [`VertexShading`] (flat/gouraud RGB + a textured flag) the
/// field-character hybrid renderer needs. The mesh + object-id arrays are
/// produced by the exact same walk, so all four outputs are index-aligned.
///
/// Untextured-prim colour layout (standard PSX packet, mirrored in the TMD):
/// the colour block precedes the vertex indices at the prim's start. A **flat**
/// prim (`F3`/`F4`) carries one RGB shared by every corner; a **gouraud** prim
/// (`G3`/`G4`) carries one RGB per corner at a 4-byte stride. The colour block
/// ends exactly where the texture block would begin for a textured prim (the
/// descriptor's `vertex_offset`).
pub fn tmd_to_vram_mesh_field_hybrid(tmd: &Tmd, buf: &[u8]) -> (VramMesh, Vec<u32>, VertexShading) {
    use crate::descriptor::Descriptor;

    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut cba_tsb = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    let mut object_ids = Vec::new();
    let mut shading_colors = Vec::new();
    let mut textured_flags = Vec::new();

    for (o_idx, o) in tmd.objects.iter().enumerate() {
        let object_vert_count = o.header.n_vert;
        let groups = legaia_prims::iter_groups_lenient(
            buf,
            o.primitives_byte_offset,
            o.primitives_byte_size,
        );

        for g in &groups {
            let desc = Descriptor::for_flags(g.header.flags);
            let is_textured = desc.is_some_and(|d| d.packet_shape.is_textured());
            // Per-vertex colour comes from the walker's decoded `Prim::colors`
            // (untextured F*/G* prims only; textured prims render white here and
            // sample their atlas via `cba_tsb`).
            let color_of = |prim: &legaia_prims::Prim, corner: usize| -> [u8; 3] {
                if is_textured {
                    return [255, 255, 255];
                }
                prim.colors.get(corner).copied().unwrap_or([128, 128, 128])
            };

            for prim in &g.prims {
                let raw_idx = prim.vertex_indices();
                if raw_idx.is_empty() || raw_idx.iter().any(|&i| (i as u32) >= object_vert_count) {
                    continue;
                }
                let ct = [prim.cba, pack_tsb_semi(prim.tsb, g.header.abe())];
                let tex_flag = if is_textured { 1u8 } else { 0u8 };
                let mut push_vert = |vidx: u16, uv_idx: usize, corner: usize| -> u32 {
                    let v = &o.vertices[vidx as usize];
                    let i = positions.len() as u32;
                    positions.push([v.x as f32, v.y as f32, v.z as f32]);
                    let (u8v, v8v) = prim.uvs.get(uv_idx).copied().unwrap_or((0, 0));
                    uvs.push([u8v, v8v]);
                    cba_tsb.push(ct);
                    colors.push(prim_color(prim, uv_idx));
                    object_ids.push(o_idx as u32);
                    shading_colors.push(color_of(prim, corner));
                    textured_flags.push(tex_flag);
                    i
                };
                match raw_idx.len() {
                    3 => {
                        let i0 = push_vert(raw_idx[0], 0, 0);
                        let i1 = push_vert(raw_idx[1], 1, 1);
                        let i2 = push_vert(raw_idx[2], 2, 2);
                        indices.extend_from_slice(&[i0, i1, i2]);
                    }
                    4 => {
                        let i0 = push_vert(raw_idx[0], 0, 0);
                        let i1 = push_vert(raw_idx[1], 1, 1);
                        let i2 = push_vert(raw_idx[2], 2, 2);
                        let i3 = push_vert(raw_idx[3], 3, 3);
                        indices.extend_from_slice(&[i0, i1, i2, i1, i3, i2]);
                    }
                    _ => {}
                }
            }
        }
    }

    let normals = compute_smooth_normals(&positions, &indices);
    (
        VramMesh {
            positions,
            uvs,
            cba_tsb,
            indices,
            normals,
            colors,
        },
        object_ids,
        VertexShading {
            colors: shading_colors,
            textured: textured_flags,
        },
    )
}
