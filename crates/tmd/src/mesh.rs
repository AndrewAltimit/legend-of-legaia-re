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

/// VRAM-aware textured mesh: per-vertex `(u, v)` and per-vertex `(cba, tsb)`
/// PSX VRAM addresses. Built by [`tmd_to_vram_mesh`]; consumed by the
/// engine-render VRAM-mesh pipeline, which does the page+CLUT lookup in
/// the fragment shader and so handles meshes that sample multiple texture
/// pages and palettes correctly (the single-binding [`TexturedMesh`] path
/// does not).
#[derive(Debug, Clone)]
pub struct VramMesh {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[u8; 2]>,
    /// Per-vertex `[cba, tsb]`. The TSB half additionally carries the prim's
    /// semi-transparency enable in bit 15 (see [`TSB_SEMI_TRANSPARENT_BIT`]).
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
    let mut indices = Vec::new();
    let mut object_ids = Vec::new();
    let mut colors = Vec::new();
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
                    object_ids.push(o_idx as u32);
                    colors.push(color_of(prim, corner));
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
        },
        object_ids,
        VertexShading {
            colors,
            textured: textured_flags,
        },
    )
}

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

/// Like [`tmd_to_vram_mesh`] but drops primitives whose textures wouldn't
/// have valid data when sampled - the caller's `keep_prim` closure decides
/// per primitive whether the (CBA, TSB, UV) tuple has plausible VRAM data.
///
/// Returning `false` from the closure skips that primitive entirely (its
/// vertices don't enter the mesh), which is the cleanest way to deal with
/// the asset-viewer case where a TMD references CLUT rows / texture pages
/// that the loaded TIM bundle didn't supply: rather than rasterising
/// solid `CLUT[0]` over the whole prim (which often shows up as a flat
/// green / cyan tint), we just leave the prim out and let the rest of the
/// model render correctly.
///
/// The closure receives the raw CBA/TSB bytes and a slice of per-vertex
/// UVs (`(u, v)` each in `0..=255`); a typical predicate is "any non-zero
/// VRAM pixel inside the CLUT row + texture-page UV bbox". See
/// `crates/asset-viewer` for a concrete VRAM-backed predicate.
pub fn tmd_to_vram_mesh_filtered<F>(tmd: &Tmd, buf: &[u8], mut keep_prim: F) -> VramMesh
where
    F: FnMut(u16, u16, &[(u8, u8)]) -> bool,
{
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut cba_tsb = Vec::new();
    let mut indices = Vec::new();

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
                if prim.uvs.is_empty() {
                    continue;
                }
                if !keep_prim(prim.cba, prim.tsb, &prim.uvs) {
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
    VramMesh {
        positions,
        uvs,
        cba_tsb,
        indices,
        normals,
    }
}

/// Per-build accounting for [`tmd_to_vram_mesh_filtered_stats`]. Tracks
/// how many primitives the filter kept vs dropped, broken down by the
/// reason for the drop. Used by engine diagnostics ("for this TMD the
/// VRAM pre-pass left N% of textured prims unrenderable").
#[derive(Debug, Clone, Default)]
pub struct FilterStats {
    /// Primitives that made it into the mesh.
    pub kept: usize,
    /// Primitives the `keep_prim` predicate rejected (typically missing
    /// CLUT row / palette-depth mismatch / un-uploaded texture page).
    pub dropped_by_filter: usize,
    /// Primitives skipped because their vertex indices walked off the
    /// object's vertex pool. Indicates a parser-or-data bug, not a
    /// VRAM-coverage issue.
    pub skipped_bad_vert_index: usize,
    /// Primitives skipped because they carry no UVs (untextured shapes).
    /// The filter never runs for these because there's nothing to look
    /// up.
    pub skipped_untextured: usize,
}

/// Structured decision a status-aware filter predicate returns for one
/// primitive. Lives in `legaia_tmd` (not `legaia_tim`) so the mesh
/// builder doesn't need to depend on TIM/VRAM internals to count drops
/// by reason. `legaia-engine-core` translates `tim::PrimTextureStatus`
/// into this enum at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimDecision {
    /// Keep the prim - VRAM has both CLUT and texture data.
    Keep,
    /// CLUT row is empty - drop.
    MissingClut,
    /// CLUT row's populated width disagrees with the prim's color depth
    /// (e.g. 4bpp prim sampling an 8bpp palette row). Drop.
    ClutDepthMismatch,
    /// The texture-page region the UVs cover has zero VRAM data. Drop.
    MissingTexturePage,
}

/// Same as [`FilterStats`] but for the [`PrimDecision`]-aware variant:
/// tracks not just "kept vs dropped" but *why* each drop happened. Lets
/// engine diagnostics distinguish "we never uploaded the texture page"
/// (load chain incomplete) from "two TIMs collided on the same CLUT
/// row" (slot-arbitration bug in the pre-pass).
#[derive(Debug, Clone, Default)]
pub struct FilterStatsByReason {
    pub kept: usize,
    pub missing_clut: usize,
    pub clut_depth_mismatch: usize,
    pub missing_texture_page: usize,
    pub skipped_bad_vert_index: usize,
    pub skipped_untextured: usize,
}

impl FilterStatsByReason {
    /// Total prims walked.
    pub fn total_seen(&self) -> usize {
        self.kept
            + self.missing_clut
            + self.clut_depth_mismatch
            + self.missing_texture_page
            + self.skipped_bad_vert_index
            + self.skipped_untextured
    }

    /// Fraction of textured prims (anything that ran the filter) that
    /// survived. Returns `1.0` when there are no textured prims to
    /// avoid punishing flat-shaded meshes.
    pub fn keep_ratio(&self) -> f32 {
        let textured =
            self.kept + self.missing_clut + self.clut_depth_mismatch + self.missing_texture_page;
        if textured == 0 {
            1.0
        } else {
            self.kept as f32 / textured as f32
        }
    }
}

/// Same as [`tmd_to_vram_mesh_filtered_stats`] but the filter predicate
/// returns a [`PrimDecision`] so the resulting [`FilterStatsByReason`]
/// can break down drops by reason.
pub fn tmd_to_vram_mesh_status_stats<F>(
    tmd: &Tmd,
    buf: &[u8],
    mut keep_prim: F,
) -> (VramMesh, FilterStatsByReason)
where
    F: FnMut(u16, u16, &[(u8, u8)]) -> PrimDecision,
{
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut cba_tsb = Vec::new();
    let mut indices = Vec::new();
    let mut stats = FilterStatsByReason::default();

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
                    stats.skipped_bad_vert_index += 1;
                    continue;
                }
                if prim.uvs.is_empty() {
                    stats.skipped_untextured += 1;
                    continue;
                }
                match keep_prim(prim.cba, prim.tsb, &prim.uvs) {
                    PrimDecision::Keep => stats.kept += 1,
                    PrimDecision::MissingClut => {
                        stats.missing_clut += 1;
                        continue;
                    }
                    PrimDecision::ClutDepthMismatch => {
                        stats.clut_depth_mismatch += 1;
                        continue;
                    }
                    PrimDecision::MissingTexturePage => {
                        stats.missing_texture_page += 1;
                        continue;
                    }
                }
                let ct = [prim.cba, pack_tsb_semi(prim.tsb, g.header.abe())];
                let mut push_vert = |vidx: u16, uv_idx: usize| -> u32 {
                    let v = &o.vertices[vidx as usize];
                    let i = positions.len() as u32;
                    positions.push([v.x as f32, v.y as f32, v.z as f32]);
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
    (
        VramMesh {
            positions,
            uvs,
            cba_tsb,
            indices,
            normals,
        },
        stats,
    )
}

impl FilterStats {
    /// Total primitives walked (kept + dropped + skipped).
    pub fn total_seen(&self) -> usize {
        self.kept + self.dropped_by_filter + self.skipped_bad_vert_index + self.skipped_untextured
    }

    /// Fraction of textured primitives that survived the filter.
    /// `1.0` means everything kept; `0.0` means everything dropped.
    /// Returns `1.0` for the "no textured prims" case (so it doesn't
    /// punish flat-shaded meshes).
    pub fn keep_ratio(&self) -> f32 {
        let textured = self.kept + self.dropped_by_filter;
        if textured == 0 {
            1.0
        } else {
            self.kept as f32 / textured as f32
        }
    }
}

/// Same as [`tmd_to_vram_mesh_filtered`] but also returns a
/// [`FilterStats`] summary so diagnostics can report how many prims the
/// VRAM-coverage filter dropped. The mesh is identical to what
/// [`tmd_to_vram_mesh_filtered`] would emit; only the bookkeeping is new.
pub fn tmd_to_vram_mesh_filtered_stats<F>(
    tmd: &Tmd,
    buf: &[u8],
    mut keep_prim: F,
) -> (VramMesh, FilterStats)
where
    F: FnMut(u16, u16, &[(u8, u8)]) -> bool,
{
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut cba_tsb = Vec::new();
    let mut indices = Vec::new();
    let mut stats = FilterStats::default();

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
                    stats.skipped_bad_vert_index += 1;
                    continue;
                }
                if prim.uvs.is_empty() {
                    stats.skipped_untextured += 1;
                    continue;
                }
                if !keep_prim(prim.cba, prim.tsb, &prim.uvs) {
                    stats.dropped_by_filter += 1;
                    continue;
                }
                stats.kept += 1;
                let ct = [prim.cba, pack_tsb_semi(prim.tsb, g.header.abe())];
                let mut push_vert = |vidx: u16, uv_idx: usize| -> u32 {
                    let v = &o.vertices[vidx as usize];
                    let i = positions.len() as u32;
                    positions.push([v.x as f32, v.y as f32, v.z as f32]);
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
    (
        VramMesh {
            positions,
            uvs,
            cba_tsb,
            indices,
            normals,
        },
        stats,
    )
}

/// Like [`tmd_to_vram_mesh`] but applies per-object (per-bone) pose offsets
/// before emitting vertices. Each element of `bone_offsets` is a `(pos, rot)`
/// pair sourced from [`legaia_anm::PoseFrame::bone_outputs`] for the
/// corresponding TMD object index. Only the translation (`pos`) is applied;
/// rotation requires full GTE-matrix math and is deferred.
///
/// If `bone_offsets` is shorter than the TMD's object count, the remaining
/// objects are rendered at their default positions (no pose applied).
pub fn tmd_to_vram_mesh_posed(
    tmd: &Tmd,
    buf: &[u8],
    bone_offsets: &[([i16; 3], [i16; 3])],
) -> VramMesh {
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
    VramMesh {
        positions,
        uvs,
        cba_tsb,
        indices,
        normals,
    }
}

/// Rotate a vector by the PSX `Rz · Ry · Rx` Euler composition, given the
/// per-axis cos/sin. Mirrors the byte-for-byte order the retail engine applies
/// (`RotMatrixX` then `Y` then `Z`, i.e. the matrix product `Rz·Ry·Rx`) and the
/// visually-validated site animator (`monsters.html` `_assemble`). Shared by
/// vertex and normal transforms.
#[inline]
fn rot_zyx(v: [f32; 3], cx: f32, sx: f32, cy: f32, sy: f32, cz: f32, sz: f32) -> [f32; 3] {
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
) -> VramMesh {
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
    VramMesh {
        positions,
        uvs,
        cba_tsb,
        indices,
        normals,
    }
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
fn compute_smooth_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    /// Same synthetic pyramid as the parser test: 4-vert base + apex,
    /// 4 triangles + 1 quad. Build the bytes inline so the test doesn't
    /// reach into the parser test module.
    fn synth_pyramid_tmd() -> Vec<u8> {
        let mut buf = Vec::new();
        // Header
        buf.extend_from_slice(&0x8000_0002u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        // Object table: prims start at offset 28 (right after table), then
        // verts. Synthetic prim section has 1 group of 4 FT3 prims (count=4
        // flags=0x20 olen=7 ilen=5) for 8 + 4*20 = 88 bytes, plus a 20-byte
        // footer slot, plus a 4-byte terminator = 112 bytes total.
        let prim_top: u32 = 28;
        let prim_size: u32 = 8 + (4 + 1) * 20 + 4; // 112
        let vert_top: u32 = prim_top + prim_size; // 140
        buf.extend_from_slice(&vert_top.to_le_bytes()); // vert_top
        buf.extend_from_slice(&5u32.to_le_bytes()); // n_vert
        buf.extend_from_slice(&0u32.to_le_bytes()); // normal_top
        buf.extend_from_slice(&0u32.to_le_bytes()); // n_normal
        buf.extend_from_slice(&prim_top.to_le_bytes()); // prim_top
        buf.extend_from_slice(&4u32.to_le_bytes()); // n_primitive
        buf.extend_from_slice(&0i32.to_le_bytes()); // scale
        // Group header: count=4 flags=0x20 olen=7 ilen=5 flag=1 mode=0x27
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&0x0020u16.to_le_bytes());
        buf.extend_from_slice(&[7, 5, 1, 0x27]);
        // 4 prims of 20 bytes each, vertex indices at byte offset 14 (raw
        // byte-offset = array_idx * 8). Apex-fan: (4,0,1) (4,1,2) (4,2,3) (4,3,0)
        let fan: [(u16, u16, u16); 4] = [(4, 0, 1), (4, 1, 2), (4, 2, 3), (4, 3, 0)];
        for (a, b, c) in fan {
            let mut prim = vec![0u8; 20];
            prim[14..16].copy_from_slice(&(a * 8).to_le_bytes());
            prim[16..18].copy_from_slice(&(b * 8).to_le_bytes());
            prim[18..20].copy_from_slice(&(c * 8).to_le_bytes());
            buf.extend_from_slice(&prim);
        }
        // Footer slot (one extra prim-stride of zeros).
        buf.extend_from_slice(&[0u8; 20]);
        // Terminator u32.
        buf.extend_from_slice(&0u32.to_le_bytes());
        // Vertices: 4 base @ y=85, apex at (0, -170, 0).
        let verts = [
            (64i16, 85i16, 0i16),
            (0, 85, -64),
            (-64, 85, 0),
            (0, 85, 64),
            (0, -170, 0),
        ];
        for (x, y, z) in verts {
            buf.extend_from_slice(&x.to_le_bytes());
            buf.extend_from_slice(&y.to_le_bytes());
            buf.extend_from_slice(&z.to_le_bytes());
            buf.extend_from_slice(&0i16.to_le_bytes());
        }
        buf
    }

    /// One untextured gouraud triangle (`G3`, flags 0x1D): 3 colour words at
    /// the prim start, then 3 vertex indices. Exercises the colour-mesh path.
    fn synth_untextured_g3_tmd() -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x8000_0002u32.to_le_bytes()); // magic
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes()); // 1 object
        let prim_top: u32 = 28;
        let prim_size: u32 = 8 + (1 + 1) * 20 + 4; // 1 prim + footer + terminator
        let vert_top: u32 = prim_top + prim_size;
        buf.extend_from_slice(&vert_top.to_le_bytes()); // vert_top
        buf.extend_from_slice(&3u32.to_le_bytes()); // n_vert
        buf.extend_from_slice(&0u32.to_le_bytes()); // normal_top
        buf.extend_from_slice(&0u32.to_le_bytes()); // n_normal
        buf.extend_from_slice(&prim_top.to_le_bytes()); // prim_top
        buf.extend_from_slice(&1u32.to_le_bytes()); // n_primitive
        buf.extend_from_slice(&0i32.to_le_bytes()); // scale
        // Group: count=1 flags=0x1D (G3) olen=5 ilen=5 flag=0 mode=0x31.
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&0x001Du16.to_le_bytes());
        buf.extend_from_slice(&[5, 5, 0, 0x31]);
        // Prim: [0..4) red, [4..8) green, [8..12) blue, [12..18) verts 0,1,2.
        let mut prim = vec![0u8; 20];
        prim[0..4].copy_from_slice(&[0xFF, 0x00, 0x00, 0x34]);
        prim[4..8].copy_from_slice(&[0x00, 0xFF, 0x00, 0x34]);
        prim[8..12].copy_from_slice(&[0x00, 0x00, 0xFF, 0x34]);
        for (i, &raw) in [0u16, 8, 16].iter().enumerate() {
            prim[12 + i * 2..14 + i * 2].copy_from_slice(&raw.to_le_bytes());
        }
        buf.extend_from_slice(&prim);
        buf.extend_from_slice(&[0u8; 20]); // footer slot
        buf.extend_from_slice(&0u32.to_le_bytes()); // terminator
        for (x, y, z) in [(0i16, 0i16, 0i16), (64, 0, 0), (0, 64, 0)] {
            buf.extend_from_slice(&x.to_le_bytes());
            buf.extend_from_slice(&y.to_le_bytes());
            buf.extend_from_slice(&z.to_le_bytes());
            buf.extend_from_slice(&0i16.to_le_bytes());
        }
        buf
    }

    #[test]
    fn color_mesh_from_untextured_prim() {
        let buf = synth_untextured_g3_tmd();
        let tmd = parse(&buf).unwrap();
        let cm = tmd_to_color_mesh(&tmd, &buf);
        assert!(!cm.is_empty());
        assert_eq!(cm.positions.len(), 3);
        assert_eq!(cm.indices, vec![0, 1, 2]);
        // Per-vertex gouraud colours (RGB, code byte dropped).
        assert_eq!(cm.colors, vec![[0xFF, 0, 0], [0, 0xFF, 0], [0, 0, 0xFF]]);
        // The textured VRAM builder drops this prim (no UVs) -> empty mesh,
        // which is exactly why the colour path exists.
        let vm = tmd_to_vram_mesh(&tmd, &buf);
        assert!(vm.indices.is_empty());
    }

    #[test]
    fn color_mesh_skips_textured_prims() {
        // The FT3 pyramid is all textured -> the colour mesh is empty.
        let buf = synth_pyramid_tmd();
        let tmd = parse(&buf).unwrap();
        assert!(tmd_to_color_mesh(&tmd, &buf).is_empty());
    }

    #[test]
    fn pyramid_to_mesh() {
        let buf = synth_pyramid_tmd();
        let tmd = parse(&buf).unwrap();
        let mesh = tmd_to_mesh(&tmd, &buf);
        assert_eq!(mesh.vertex_count(), 5);
        assert_eq!(mesh.triangle_count(), 4); // 4 FT3 fan tris
        assert_eq!(mesh.indices.len(), 12);
        // Apex (vertex 4) is in every triangle.
        for tri in mesh.indices.chunks_exact(3) {
            assert!(tri.contains(&4u32), "expected apex (4) in tri {:?}", tri);
        }
    }

    #[test]
    fn aabb_pyramid() {
        let buf = synth_pyramid_tmd();
        let tmd = parse(&buf).unwrap();
        let mesh = tmd_to_mesh(&tmd, &buf);
        let (lo, hi) = mesh.aabb();
        assert_eq!(lo, [-64.0, -170.0, -64.0]);
        assert_eq!(hi, [64.0, 85.0, 64.0]);
    }

    #[test]
    fn vram_mesh_pyramid_has_per_corner_verts() {
        // Synth pyramid prims are FT3 (flags=0x20) - the parser decodes a
        // texture block with all-zero UVs/CBA/TSB. tmd_to_vram_mesh emits
        // 4 prims × 3 corners = 12 verts, one per (prim, corner) pair.
        let buf = synth_pyramid_tmd();
        let tmd = parse(&buf).unwrap();
        let vmesh = tmd_to_vram_mesh(&tmd, &buf);
        assert_eq!(vmesh.positions.len(), 12);
        assert_eq!(vmesh.uvs.len(), 12);
        assert_eq!(vmesh.cba_tsb.len(), 12);
        assert_eq!(vmesh.indices.len(), 12);
        assert_eq!(vmesh.triangle_count(), 4);
        // The fixture group's mode byte is 0x27 (ABE set), so the builder
        // packs the semi-transparency enable into TSB bit 15 on top of the
        // all-zero on-disc CBA/TSB.
        for ct in &vmesh.cba_tsb {
            assert_eq!(*ct, [0, TSB_SEMI_TRANSPARENT_BIT]);
        }
    }

    #[test]
    fn pack_tsb_semi_packs_bit15_only() {
        // Sets the bit when ABE is on, leaves decode fields alone.
        assert_eq!(pack_tsb_semi(0x001A, true), 0x801A);
        assert_eq!(pack_tsb_semi(0x001A, false), 0x001A);
        // Clears on-disc garbage in bit 15 when ABE is off.
        assert_eq!(pack_tsb_semi(0x801A, false), 0x001A);
        // All used TSB fields (bits 0..=8) survive the round trip.
        assert_eq!(pack_tsb_semi(0x01FF, true) & 0x01FF, 0x01FF);
    }

    #[test]
    fn empty_object_produces_empty_mesh() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x8000_0002u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        let tmd = parse(&buf).unwrap();
        let mesh = tmd_to_mesh(&tmd, &buf);
        assert_eq!(mesh.vertex_count(), 0);
        assert_eq!(mesh.triangle_count(), 0);
    }

    #[test]
    fn posed_mesh_applies_translation_per_bone() {
        let buf = synth_pyramid_tmd();
        let tmd = parse(&buf).unwrap();

        // Shift object 0 by (+100, +200, +300).
        let offset: [i16; 3] = [100, 200, 300];
        let no_rot: [i16; 3] = [0, 0, 0];
        let bone_offsets = [(offset, no_rot)];

        let unposed = tmd_to_vram_mesh(&tmd, &buf);
        let posed = tmd_to_vram_mesh_posed(&tmd, &buf, &bone_offsets);

        assert_eq!(
            unposed.positions.len(),
            posed.positions.len(),
            "vertex count should match between posed and unposed"
        );

        for (u, p) in unposed.positions.iter().zip(posed.positions.iter()) {
            assert!(
                (p[0] - u[0] - 100.0).abs() < 0.5,
                "x should shift by 100: unposed={}, posed={}",
                u[0],
                p[0]
            );
            assert!(
                (p[1] - u[1] - 200.0).abs() < 0.5,
                "y should shift by 200: unposed={}, posed={}",
                u[1],
                p[1]
            );
            assert!(
                (p[2] - u[2] - 300.0).abs() < 0.5,
                "z should shift by 300: unposed={}, posed={}",
                u[2],
                p[2]
            );
        }
    }

    #[test]
    fn posed_mesh_zero_offset_matches_unposed() {
        let buf = synth_pyramid_tmd();
        let tmd = parse(&buf).unwrap();
        let bone_offsets = [([0i16; 3], [0i16; 3])];

        let unposed = tmd_to_vram_mesh(&tmd, &buf);
        let posed = tmd_to_vram_mesh_posed(&tmd, &buf, &bone_offsets);

        for (u, p) in unposed.positions.iter().zip(posed.positions.iter()) {
            assert_eq!(u, p, "zero offset should not move vertices");
        }
    }

    #[test]
    fn rot_zyx_identity_and_axis_quarter_turns() {
        let id = rot_zyx([3.0, 5.0, 7.0], 1.0, 0.0, 1.0, 0.0, 1.0, 0.0);
        assert_eq!(id, [3.0, 5.0, 7.0], "all-zero angles = identity");

        // 90 deg about Z (cz=0, sz=1): (x,y,z) -> (-y, x, z).
        let rz = rot_zyx([1.0, 0.0, 0.0], 1.0, 0.0, 1.0, 0.0, 0.0, 1.0);
        assert!((rz[0] - 0.0).abs() < 1e-5 && (rz[1] - 1.0).abs() < 1e-5 && rz[2].abs() < 1e-5);

        // 90 deg about X (cx=0, sx=1): (x,y,z) -> (x, -z, y).
        let rx = rot_zyx([0.0, 1.0, 0.0], 0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
        assert!(rx[0].abs() < 1e-5 && rx[1].abs() < 1e-5 && (rx[2] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn posed_rot_zero_rotation_matches_translation_posed() {
        // With zero rotation the rigid-transform builder must reduce exactly to
        // the translation-only builder (so battle posing and field posing agree
        // at the rest orientation).
        let buf = synth_pyramid_tmd();
        let tmd = parse(&buf).unwrap();
        let bone = [([100i16, 200, 300], [0i16; 3])];

        let t_only = tmd_to_vram_mesh_posed(&tmd, &buf, &bone);
        let rigid = tmd_to_vram_mesh_posed_rot(&tmd, &buf, &bone);

        assert_eq!(t_only.positions.len(), rigid.positions.len());
        for (a, b) in t_only.positions.iter().zip(rigid.positions.iter()) {
            for k in 0..3 {
                assert!((a[k] - b[k]).abs() < 1e-3, "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn posed_rot_quarter_turn_z_rotates_the_aabb() {
        // A 90-deg Z turn (rz = 4096/4 = 1024) maps (x,y,z) -> (-y, x, z), so
        // the pyramid's tall -Y apex swings onto +X. Check the posed AABB.
        let buf = synth_pyramid_tmd();
        let tmd = parse(&buf).unwrap();
        let bone = [([0i16; 3], [0i16, 0, 1024])];
        let posed = tmd_to_vram_mesh_posed_rot(&tmd, &buf, &bone);
        let (lo, hi) = posed.aabb();
        // verts rotate to x in [-85,170], y in [-64,64], z in [-64,64].
        assert!((lo[0] - -85.0).abs() < 0.5, "lo.x {}", lo[0]);
        assert!((hi[0] - 170.0).abs() < 0.5, "hi.x {}", hi[0]);
        assert!(
            (lo[1] - -64.0).abs() < 0.5 && (hi[1] - 64.0).abs() < 0.5,
            "y {lo:?} {hi:?}"
        );
    }

    #[test]
    fn posed_mesh_empty_offsets_matches_unposed() {
        let buf = synth_pyramid_tmd();
        let tmd = parse(&buf).unwrap();

        let unposed = tmd_to_vram_mesh(&tmd, &buf);
        let posed = tmd_to_vram_mesh_posed(&tmd, &buf, &[]);

        for (u, p) in unposed.positions.iter().zip(posed.positions.iter()) {
            assert_eq!(u, p, "empty offsets should not move vertices");
        }
    }
}
