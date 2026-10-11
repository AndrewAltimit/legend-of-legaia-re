//! Geometry reduction for the field budget: decimating an object and flattening
//! a primitive.

use super::super::*;
use legaia_tmd::encode::ModelPrim;

/// Drop prims whose baked extent falls under `threshold` (invisible at
/// the chibi field scale) and prune the vertices nothing references.
pub(in super::super) fn decimate_object(o: &mut ModelObject, threshold: f32) {
    if threshold <= 0.0 {
        return;
    }
    for g in o.groups.iter_mut() {
        g.prims.retain(|p| {
            let mut min = [i16::MAX; 3];
            let mut max = [i16::MIN; 3];
            for &vi in &p.vertices {
                let Some(v) = o.vertices.get(vi as usize) else {
                    return true;
                };
                for k in 0..3 {
                    min[k] = min[k].min(v[k]);
                    max[k] = max[k].max(v[k]);
                }
            }
            let d = (0..3)
                .map(|k| (max[k] - min[k]) as f32)
                .fold(0f32, |a, b| a.max(b));
            d >= threshold
        });
    }
    o.groups.retain(|g| !g.prims.is_empty());
    // Prune unreferenced vertices.
    let mut used = vec![false; o.vertices.len()];
    for g in &o.groups {
        for p in &g.prims {
            for &vi in &p.vertices {
                if let Some(u) = used.get_mut(vi as usize) {
                    *u = true;
                }
            }
        }
    }
    let mut remap = vec![0u16; o.vertices.len()];
    let mut kept = Vec::new();
    for (i, v) in o.vertices.iter().enumerate() {
        if used[i] {
            remap[i] = kept.len() as u16;
            kept.push(*v);
        }
    }
    o.vertices = kept;
    for g in o.groups.iter_mut() {
        for p in g.prims.iter_mut() {
            for vi in p.vertices.iter_mut() {
                *vi = remap[*vi as usize];
            }
        }
    }
}

/// Convert a textured prim into an untextured gouraud prim whose corner
/// colours sample the source page (colour = texel through its palette,
/// modulated by the prim's own colour, `0x80` = neutral).
pub(in super::super) fn flatten_prim(
    p: &ModelPrim,
    cluts: &[[u16; 16]],
    indices: &[u8],
    width: usize,
) -> ModelPrim {
    let sample = |uv: (u8, u8), k: usize| -> [u8; 3] {
        let (u, v) = (uv.0 as usize, uv.1 as usize);
        let idx = if u < width && v < PAGE_HEIGHT {
            indices[v * width + u] as usize
        } else {
            0
        };
        let pal = (p.cba & 0x3F) as usize;
        let c = cluts.get(pal).map_or(0, |cl| cl[idx]);
        let scale5 = |x: u16| ((x << 3) | (x >> 2)) as u32;
        let (r, g, b) = (
            scale5(c & 0x1F),
            scale5((c >> 5) & 0x1F),
            scale5((c >> 10) & 0x1F),
        );
        let mc = if p.colors.len() == p.vertices.len() {
            p.colors[k]
        } else {
            p.colors.first().copied().unwrap_or([0x80; 3])
        };
        [
            ((r * mc[0] as u32) / 0x80).min(255) as u8,
            ((g * mc[1] as u32) / 0x80).min(255) as u8,
            ((b * mc[2] as u32) / 0x80).min(255) as u8,
        ]
    };
    // One flat colour (the corner average): F3/F4 store a single colour
    // word, half the size of a gouraud prim - the field budget is tight
    // and the sprites are tiny on screen.
    let mut acc = [0u32; 3];
    for (k, &uv) in p.uvs.iter().enumerate() {
        let c = sample(uv, k);
        for (a, ch) in acc.iter_mut().zip(c) {
            *a += ch as u32;
        }
    }
    let n = p.uvs.len().max(1) as u32;
    let avg = [(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8];
    ModelPrim {
        vertices: p.vertices.clone(),
        uvs: Vec::new(),
        cba: 0,
        tsb: 0,
        colors: vec![avg],
    }
}
