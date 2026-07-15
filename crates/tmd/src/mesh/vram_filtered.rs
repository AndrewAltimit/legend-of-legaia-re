//! VRAM-coverage-filtered mesh builders + their per-build drop accounting.

use crate::{Tmd, legaia_prims};

use super::vram::prim_color;
use super::{VramMesh, compute_smooth_normals, pack_tsb_semi};

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
///
/// [`tmd_to_vram_mesh`]: super::tmd_to_vram_mesh
pub fn tmd_to_vram_mesh_filtered<F>(tmd: &Tmd, buf: &[u8], mut keep_prim: F) -> VramMesh
where
    F: FnMut(u16, u16, &[(u8, u8)]) -> bool,
{
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut cba_tsb = Vec::new();
    let mut colors = Vec::new();
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
                    colors.push(prim_color(prim, uv_idx));
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
        colors,
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
    let mut colors = Vec::new();
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
                    colors.push(prim_color(prim, uv_idx));
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
    let mut colors = Vec::new();
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
                    colors.push(prim_color(prim, uv_idx));
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
        stats,
    )
}
