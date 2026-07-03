//! Stage-geometry wireframe viewer: parses a stage-geometry PROT entry
//! into a colored line-list payload for the renderer.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// CPU-side payload for the wireframe path. Built by [`load_stage_for_view`].
pub(crate) struct LinesPayload {
    pub(crate) positions: Vec<[f32; 3]>,
    pub(crate) colors: Vec<[u8; 4]>,
    pub(crate) indices: Vec<u32>,
}

/// Build a renderable wireframe payload from a stage-geometry PROT entry.
/// Each record becomes a line loop (4 segments - degenerate triangle quads
/// collapse one edge naturally). Vertex coords come from the parsed pool;
/// PSX is Y-down so we flip Y at upload so up-is-up in the viewer camera.
pub(crate) fn load_stage_for_view(path: &Path) -> Result<LinesPayload> {
    let raw = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let stage = legaia_asset::stage_geom::parse(&raw)
        .ok_or_else(|| anyhow::anyhow!("no stage table in {}", path.display()))?;
    let largest = stage
        .tables
        .iter()
        .max_by_key(|t| t.records)
        .ok_or_else(|| anyhow::anyhow!("empty table list"))?;

    let vert_count = stage.vertex_count();
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(vert_count);
    let mut colors: Vec<[u8; 4]> = Vec::with_capacity(vert_count);
    // Walk vertex pool to compute AABB on Y for depth-shaded coloring.
    let mut min_y: f32 = f32::MAX;
    let mut max_y: f32 = f32::MIN;
    for i in 0..vert_count {
        let v = stage.vertex(&raw, i).expect("in range");
        let py = -(v.y as f32); // PSX Y-down -> renderer Y-up
        if py < min_y {
            min_y = py;
        }
        if py > max_y {
            max_y = py;
        }
        positions.push([v.x as f32, py, v.z as f32]);
        colors.push([0xFF, 0xFF, 0xFF, 0xFF]); // overwritten below
    }
    let y_span = (max_y - min_y).max(1.0);
    for (i, c) in colors.iter_mut().enumerate() {
        let py = positions[i][1];
        // Cool→warm gradient on Y so the eye reads height. Top = warm.
        let t = ((py - min_y) / y_span).clamp(0.0, 1.0);
        let r = (60.0 + 195.0 * t) as u8;
        let g = (140.0 + 60.0 * (1.0 - (t - 0.5).abs() * 2.0)) as u8;
        let b = (220.0 - 160.0 * t) as u8;
        *c = [r, g, b, 0xFF];
    }

    // Build line indices: for each in-range record, emit 4 segments
    // forming the quad outline. Degenerate quads (idx[3] == idx[0]) just
    // add one zero-length edge - harmless.
    let mut indices: Vec<u32> = Vec::with_capacity(largest.records * 8);
    let mut emitted = 0usize;
    let mut skipped = 0usize;
    for rec in legaia_asset::stage_geom::records(&raw, largest) {
        let Some(idx) = stage.quad_vertex_indices(&rec) else {
            skipped += 1;
            continue;
        };
        // Range check (parse already guarantees but belt-and-braces).
        if idx.iter().any(|&i| i >= vert_count) {
            skipped += 1;
            continue;
        }
        let a = idx[0] as u32;
        let b = idx[1] as u32;
        let c = idx[2] as u32;
        let d = idx[3] as u32;
        // Quad outline: a-b, b-c, c-d, d-a.
        indices.extend_from_slice(&[a, b, b, c, c, d, d, a]);
        emitted += 1;
    }
    if skipped > 0 {
        log::warn!(
            "stage {}: skipped {} of {} records (out-of-range indices)",
            path.display(),
            skipped,
            largest.records
        );
    }
    log::info!(
        "stage {}: {} verts, {} records -> {} line segments",
        path.display(),
        vert_count,
        emitted,
        indices.len() / 2,
    );
    Ok(LinesPayload {
        positions,
        colors,
        indices,
    })
}

/// Walk `root` for files that parse as stage-geometry PROT entries. Used
/// by `stage <DIR>` mode to skip non-stage entries during navigation.
pub(crate) fn collect_stage_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_file() {
            continue;
        }
        let Ok(raw) = std::fs::read(&path) else {
            continue;
        };
        if legaia_asset::stage_geom::parse(&raw).is_some() {
            out.push(path);
        }
    }
    out
}
