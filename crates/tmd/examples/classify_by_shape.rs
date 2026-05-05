//! Classify TMDs in a directory by AABB shape.
//!
//! Characters tend to be taller than they are wide (height/horizontal > 1.5);
//! arenas/stages tend to be wide and flat (horizontal/height > 2). Useful for
//! triaging the bulk-scan output, where CDNAME labels turn out to be misleading
//! (battle_data is actually battle arenas, not characters; etc.).
//!
//! Run: `cargo run -p legaia-tmd --example classify_by_shape -- <dir> [min_size]`

use anyhow::{Context, Result};
use legaia_tmd as tmd;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("usage: classify_by_shape <dir> [min_size_bytes]"))?;
    let min_size: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(0);

    let mut chars = 0usize;
    let mut arenas = 0usize;
    let mut blobs = 0usize;
    let mut tiny = 0usize;
    let mut sample_chars: Vec<String> = Vec::new();
    let mut sample_arenas: Vec<String> = Vec::new();

    walk(std::path::Path::new(&dir), &mut |path| {
        let Ok(meta) = std::fs::metadata(path) else {
            return;
        };
        if meta.len() < min_size {
            return;
        }
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let Ok(parsed) = tmd::parse(&bytes) else {
            return;
        };
        let Some(aabb) = mesh_aabb(&parsed) else {
            tiny += 1;
            return;
        };
        let (w, h, d) = (
            aabb.1[0] - aabb.0[0],
            aabb.1[1] - aabb.0[1],
            aabb.1[2] - aabb.0[2],
        );
        let horizontal = w.max(d).max(1.0);
        let height = h.abs().max(1.0); // Y is vertical (PSX uses Y-down so abs)
        let aspect_h_over_horiz = height / horizontal;
        let label = path.display().to_string();
        if aspect_h_over_horiz > 1.5 {
            chars += 1;
            if sample_chars.len() < 6 {
                sample_chars.push(format!(
                    "  h/horiz={:>4.1}  bbox={:>4.0}x{:>4.0}x{:>4.0}  {}",
                    aspect_h_over_horiz, w, height, d, label
                ));
            }
        } else if aspect_h_over_horiz < 0.5 {
            arenas += 1;
            if sample_arenas.len() < 6 {
                sample_arenas.push(format!(
                    "  h/horiz={:>4.1}  bbox={:>4.0}x{:>4.0}x{:>4.0}  {}",
                    aspect_h_over_horiz, w, height, d, label
                ));
            }
        } else {
            blobs += 1;
        }
    })
    .context("walk dir")?;

    println!(
        "classification (min_size={} bytes): chars={} arenas={} blobs={} tiny={}",
        min_size, chars, arenas, blobs, tiny
    );
    println!("\nsample CHARACTER-shaped (tall, h/horiz > 1.5):");
    for s in &sample_chars {
        println!("{}", s);
    }
    println!("\nsample ARENA-shaped (wide+flat, h/horiz < 0.5):");
    for s in &sample_arenas {
        println!("{}", s);
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
