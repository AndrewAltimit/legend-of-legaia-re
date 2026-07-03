use std::path::{Path, PathBuf};

use crate::common::*;
use anyhow::Result;
use legaia_asset::stage_geom;
use legaia_prot::cdname;

/// `asset stage <PATH>` - dump one entry's stage-geometry layout. Useful
/// to confirm pool placement, sample resolved quad indices, and (with
/// `--obj-out`) export a wireframe mesh for any external viewer.
pub(crate) fn stage_one(
    input: &PathBuf,
    head: usize,
    verts: usize,
    obj_out: Option<&Path>,
) -> Result<()> {
    let raw = std::fs::read(input)?;
    let stage = stage_geom::parse(&raw)
        .ok_or_else(|| anyhow::anyhow!("no stage-geometry tables in {}", input.display()))?;
    println!(
        "file: {}  size={}  tables={}",
        input.display(),
        raw.len(),
        stage.tables.len()
    );
    for (i, t) in stage.tables.iter().enumerate() {
        println!(
            "  table[{}]: start=0x{:X} ({})  records={}  end=0x{:X}",
            i, t.start, t.start, t.records, t.end
        );
    }
    println!(
        "vertex pool: offset=0x{:X} ({})  bytes={}  verts={}",
        stage.pool_offset,
        stage.pool_offset,
        stage.pool_bytes,
        stage.vertex_count()
    );

    let largest = stage
        .tables
        .iter()
        .max_by_key(|t| t.records)
        .expect("at least one");
    println!("\nfirst {} records (resolved):", head.min(largest.records));
    let mut resolved = 0usize;
    let mut unresolved = 0usize;
    for (i, rec) in stage_geom::records(&raw, largest).enumerate().take(head) {
        let pl = rec.payload_u16s();
        match stage.quad_vertex_indices(&rec) {
            Some(idx) => {
                let kind = if idx[3] == idx[0] { "tri" } else { "quad" };
                println!(
                    "  rec {:>4}: bytes [{:>5} {:>5} {:>5} {:>5}]  -> {} verts {:?}",
                    i, pl[0], pl[1], pl[2], pl[3], kind, idx
                );
                resolved += 1;
            }
            None => {
                println!(
                    "  rec {:>4}: bytes [{:>5} {:>5} {:>5} {:>5}]  -> OUT OF RANGE",
                    i, pl[0], pl[1], pl[2], pl[3]
                );
                unresolved += 1;
            }
        }
    }
    // Tally for the whole table so the user knows the overall hit rate.
    let mut total_resolved = 0usize;
    for rec in stage_geom::records(&raw, largest) {
        if stage.quad_vertex_indices(&rec).is_some() {
            total_resolved += 1;
        }
    }
    println!(
        "\nresolved {}/{} records overall ({} shown above: {} ok, {} oor)",
        total_resolved,
        largest.records,
        head.min(largest.records),
        resolved,
        unresolved
    );

    println!("\nfirst {} vertices:", verts.min(stage.vertex_count()));
    for i in 0..verts.min(stage.vertex_count()) {
        let v = stage.vertex(&raw, i).expect("in range");
        println!("  v{:<4}: x={:>6} y={:>6} z={:>6}", i, v.x, v.y, v.z);
    }

    if let Some(out) = obj_out {
        write_stage_obj(&raw, &stage, largest, out)?;
        println!("\nwrote wireframe OBJ: {}", out.display());
    }
    Ok(())
}

/// Write a Wavefront OBJ with all in-range quads/tris from `table` as line
/// loops (`l` directives). Standard 3D viewers render these as wireframe.
pub(crate) fn write_stage_obj(
    buf: &[u8],
    stage: &stage_geom::Stage,
    table: &stage_geom::GeomTable,
    out: &Path,
) -> Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(out)?;
    writeln!(f, "# stage-geometry wireframe")?;
    writeln!(
        f,
        "# verts={}  records={}",
        stage.vertex_count(),
        table.records
    )?;
    for i in 0..stage.vertex_count() {
        let v = stage.vertex(buf, i).unwrap();
        // OBJ is right-handed Y-up; the source is PSX Y-down, so flip Y.
        writeln!(f, "v {} {} {}", v.x, -(v.y as i32), v.z)?;
    }
    for rec in stage_geom::records(buf, table) {
        let Some(idx) = stage.quad_vertex_indices(&rec) else {
            continue;
        };
        // OBJ indices are 1-based; degenerate 4th vert (idx[3] == idx[0])
        // collapses naturally in a 4-vertex line loop.
        let a = idx[0] + 1;
        let b = idx[1] + 1;
        let c = idx[2] + 1;
        let d = idx[3] + 1;
        writeln!(f, "l {} {} {} {} {}", a, b, c, d, a)?;
    }
    Ok(())
}

/// `asset clut-finder` - walk `extracted/tim_scan/<entry>/*.tim` and report
/// every TIM whose CLUT or image rect covers the requested VRAM cell.
///
/// Used to discover which PROT entry provides a specific CLUT row that a
/// character mesh references - see `project_clut_scattering.md`.
pub(crate) fn clut_finder_cmd(
    extracted_root: &Path,
    x: u16,
    y: u16,
    clut_only: bool,
) -> Result<()> {
    let tim_scan_root = extracted_root.join("tim_scan");
    if !tim_scan_root.is_dir() {
        anyhow::bail!(
            "no tim_scan/ under {} (run `asset tim-scan` first?)",
            extracted_root.display()
        );
    }
    let mut hits: Vec<(String, String, &'static str, u16, u16, u16, u16)> = Vec::new();

    let mut subdirs: Vec<PathBuf> = std::fs::read_dir(&tim_scan_root)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    subdirs.sort();

    for sub in &subdirs {
        let entry_name = sub
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let mut tims: Vec<PathBuf> = std::fs::read_dir(sub)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .map(|e| e == "tim" || e == "TIM")
                        .unwrap_or(false)
            })
            .collect();
        tims.sort();
        for tim_path in &tims {
            let bytes = match std::fs::read(tim_path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let tim = match legaia_tim::parse(&bytes) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let tim_name = tim_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            if let Some(c) = &tim.clut {
                let inside = x >= c.fb_x && x < c.fb_x + c.w && y >= c.fb_y && y < c.fb_y + c.h;
                if inside {
                    hits.push((
                        entry_name.clone(),
                        tim_name.clone(),
                        "clut",
                        c.fb_x,
                        c.fb_y,
                        c.w,
                        c.h,
                    ));
                }
            }
            if !clut_only {
                let img = &tim.image;
                let inside = x >= img.fb_x
                    && x < img.fb_x + img.fb_w
                    && y >= img.fb_y
                    && y < img.fb_y + img.h;
                if inside {
                    hits.push((
                        entry_name.clone(),
                        tim_name,
                        "image",
                        img.fb_x,
                        img.fb_y,
                        img.fb_w,
                        img.h,
                    ));
                }
            }
        }
    }
    println!(
        "VRAM cell ({x}, {y}): {} match(es) across {} entries",
        hits.len(),
        subdirs.len()
    );
    println!(
        "{:<28}  {:<24}  {:<6}  {:>4} {:>4} {:>4} {:>4}",
        "entry", "tim", "kind", "fbx", "fby", "w", "h"
    );
    println!("{}", "-".repeat(80));
    for (entry, tim, kind, fx, fy, w, h) in &hits {
        println!("{entry:<28}  {tim:<24}  {kind:<6}  {fx:>4} {fy:>4} {w:>4} {h:>4}");
    }
    Ok(())
}

/// `asset stage-scan <DIR>` - scan a directory of PROT entries for
/// stage-geometry tables and report per-entry stats.
pub(crate) fn stage_scan_cmd(
    dir: &Path,
    cdname_path: Option<&Path>,
    only_hits: bool,
) -> Result<()> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };

    println!(
        "{:<32}  {:>5}  {:>4}  {:>6}  {:>6}  {:>4}  pool",
        "entry", "size", "tabs", "recs", "verts", "ok%"
    );
    println!("{}", "-".repeat(80));

    let mut total_hits = 0usize;
    let mut total_resolved = 0usize;
    let mut total_records = 0usize;
    for path in &paths {
        let raw = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let Some(stage) = stage_geom::parse(&raw) else {
            if !only_hits {
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
                let display_name = display_name_for(stem, names.as_ref());
                println!(
                    "{:<32}  {:>5}  {:>4}  {:>6}  {:>6}  {:>4}  no table",
                    display_name,
                    raw.len(),
                    "-",
                    "-",
                    "-",
                    "-"
                );
            }
            continue;
        };
        total_hits += 1;
        let largest = stage
            .tables
            .iter()
            .max_by_key(|t| t.records)
            .expect("at least one");
        let mut resolved = 0usize;
        for rec in stage_geom::records(&raw, largest) {
            if stage.quad_vertex_indices(&rec).is_some() {
                resolved += 1;
            }
        }
        total_resolved += resolved;
        total_records += largest.records;
        let pct = (100 * resolved).checked_div(largest.records).unwrap_or(0);
        let pool_side = if stage.pool_offset == 0 {
            "before"
        } else {
            "after"
        };

        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        let display_name = display_name_for(stem, names.as_ref());
        println!(
            "{:<32}  {:>5}  {:>4}  {:>6}  {:>6}  {:>3}%  {}",
            display_name,
            raw.len(),
            stage.tables.len(),
            largest.records,
            stage.vertex_count(),
            pct,
            pool_side
        );
    }
    println!();
    println!(
        "{} entries with stage tables; {}/{} records resolved overall ({:.1}%)",
        total_hits,
        total_resolved,
        total_records,
        if total_records > 0 {
            100.0 * total_resolved as f64 / total_records as f64
        } else {
            0.0
        }
    );
    Ok(())
}
