//! Hunt for hero meshes: character-shaped TMDs sorted by vertex count.
//!
//! Heroes have more detail than NPCs/monsters — typically 400-800 verts vs
//! ~100-300 for NPCs. Filter to height/horizontal > 1.5 (character-shape)
//! and show the top-N by vertex count.
//!
//! Run: `cargo run -p legaia-tmd --example find_heroes -- <dir> [top_n]`

use anyhow::{Context, Result};
use legaia_tmd as tmd;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("usage: find_heroes <dir> [top_n]"))?;
    let top_n: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(20);

    let mut hits: Vec<(u32, [f32; 3], String)> = Vec::new();
    walk(std::path::Path::new(&dir), &mut |path| {
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let Ok(parsed) = tmd::parse(&bytes) else {
            return;
        };
        let Some(aabb) = mesh_aabb(&parsed) else {
            return;
        };
        let extent = [
            aabb.1[0] - aabb.0[0],
            (aabb.1[1] - aabb.0[1]).abs(),
            aabb.1[2] - aabb.0[2],
        ];
        let horizontal = extent[0].max(extent[2]).max(1.0);
        let aspect = extent[1].max(1.0) / horizontal;
        if aspect <= 1.5 {
            return;
        }
        // Bound height to character scale: real Legaia characters are 100-600
        // units tall; anything over 1000 is a building / landscape feature.
        if extent[1] > 1000.0 {
            return;
        }
        let total_verts: u32 = parsed.objects.iter().map(|o| o.header.n_vert).sum();
        hits.push((total_verts, extent, path.display().to_string()));
    })
    .context("walk dir")?;

    hits.sort_by_key(|h| std::cmp::Reverse(h.0));
    println!(
        "{} character-shaped TMDs total — top {} by vertex count:",
        hits.len(),
        top_n
    );
    println!(
        "{:>5}  {:>8}  {:>5}x{:>5}x{:>5}  path",
        "verts", "h/horiz", "w", "h", "d"
    );
    for (verts, ext, path) in hits.iter().take(top_n) {
        let horiz = ext[0].max(ext[2]).max(1.0);
        let aspect = ext[1].max(1.0) / horiz;
        println!(
            "{:>5}  {:>8.2}  {:>5.0}x{:>5.0}x{:>5.0}  {}",
            verts, aspect, ext[0], ext[1], ext[2], path
        );
    }
    Ok(())
}

fn mesh_aabb(parsed: &tmd::Tmd) -> Option<([f32; 3], [f32; 3])> {
    let mut iter = parsed.objects.iter().flat_map(|o| o.vertices.iter());
    let first = iter.next()?;
    let mut lo = [first.x as f32, first.y as f32, first.z as f32];
    let mut hi = lo;
    for v in iter {
        let p = [v.x as f32, v.y as f32, v.z as f32];
        for i in 0..3 {
            if p[i] < lo[i] {
                lo[i] = p[i];
            }
            if p[i] > hi[i] {
                hi[i] = p[i];
            }
        }
    }
    Some((lo, hi))
}

fn walk(dir: &std::path::Path, cb: &mut impl FnMut(&std::path::Path)) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            walk(&path, cb)?;
        } else if ft.is_file()
            && path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("tmd"))
        {
            cb(&path);
        }
    }
    Ok(())
}
