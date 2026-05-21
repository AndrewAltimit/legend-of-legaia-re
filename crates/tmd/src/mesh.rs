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
                let ct = [prim.cba, prim.tsb];
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
                let ct = [prim.cba, prim.tsb];
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
                let ct = [prim.cba, prim.tsb];
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
                let ct = [prim.cba, prim.tsb];
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
                let ct = [prim.cba, prim.tsb];
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
        for ct in &vmesh.cba_tsb {
            assert_eq!(*ct, [0, 0]);
        }
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
