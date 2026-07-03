use std::path::Path;

use crate::RenderTier;
use crate::common::*;
use anyhow::Result;
use legaia_asset::{tim_catalog, tim_deep_catalog, tim_scan, tmd_scan};
use legaia_prot::cdname;

pub(crate) fn tmd_scan_cmd(
    dir: &std::path::Path,
    cdname_path: Option<&std::path::Path>,
    only_hits: bool,
    out: Option<&std::path::Path>,
) -> Result<()> {
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };

    if let Some(out) = out {
        std::fs::create_dir_all(out)?;
    }

    println!(
        "{:<32}  {:>4}  {:>4}  {:>5}  {:>6}  notes",
        "entry", "raw", "lzs", "verts", "prims"
    );
    println!("{}", "-".repeat(80));

    let mut total_hits = 0usize;
    let mut total_verts = 0u32;
    let mut total_prims = 0u32;
    let mut entries_with_hits = 0usize;
    let mut tmds_written = 0usize;

    for path in &entries {
        let raw = std::fs::read(path)?;
        let scan = tmd_scan::scan_entry(&raw);
        if scan.hits.is_empty() && only_hits {
            continue;
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let display_name = display_name_for(&stem, names.as_ref());

        let raw_hits = scan
            .hits
            .iter()
            .filter(|(s, _)| matches!(s, tmd_scan::Source::Raw))
            .count();
        let lzs_hits = scan.hits.len() - raw_hits;
        let v: u32 = scan.hits.iter().map(|(_, h)| h.total_verts).sum();
        let p: u32 = scan.hits.iter().map(|(_, h)| h.total_prims).sum();
        let notes = if scan.lzs_ok { "" } else { "(lzs:no)" };
        if !scan.hits.is_empty() {
            println!(
                "{:<32}  {:>4}  {:>4}  {:>5}  {:>6}  {}",
                display_name, raw_hits, lzs_hits, v, p, notes
            );
            entries_with_hits += 1;
            total_hits += scan.hits.len();
            total_verts += v;
            total_prims += p;
        } else if !only_hits {
            println!(
                "{:<32}  {:>4}  {:>4}  {:>5}  {:>6}  {}",
                display_name, "-", "-", "-", "-", notes
            );
        }

        if let Some(out_root) = out {
            let entry_dir = out_root.join(&display_name);
            for (src, hit) in &scan.hits {
                let (buf, label) = match src {
                    tmd_scan::Source::Raw => (raw.as_slice(), "raw".to_string()),
                    tmd_scan::Source::Lzs(idx) => {
                        let Some(section) = scan.lzs_sections.get(*idx) else {
                            continue;
                        };
                        (section.as_slice(), format!("lzs{}", idx))
                    }
                };
                let end = (hit.offset + hit.byte_len).min(buf.len());
                let slab = &buf[hit.offset..end];
                std::fs::create_dir_all(&entry_dir)?;
                let fname = format!("{}_off{:06X}.tmd", label, hit.offset);
                std::fs::write(entry_dir.join(&fname), slab)?;
                tmds_written += 1;
            }
        }
    }

    println!();
    println!(
        "{} entries with TMDs, {} hits total ({} verts, {} prims)",
        entries_with_hits, total_hits, total_verts, total_prims
    );
    if out.is_some() {
        println!("wrote {} TMD files", tmds_written);
    }
    Ok(())
}

pub(crate) fn tim_scan_cmd(
    dir: &std::path::Path,
    cdname_path: Option<&std::path::Path>,
    only_hits: bool,
    out: Option<&std::path::Path>,
) -> Result<()> {
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };

    if let Some(out) = out {
        std::fs::create_dir_all(out)?;
    }

    println!(
        "{:<32}  {:>4}  {:>4}  {:>5}  {:>5}  notes",
        "entry", "raw", "lzs", "tims", "px"
    );
    println!("{}", "-".repeat(80));

    let mut total_hits = 0usize;
    let mut entries_with_hits = 0usize;
    let mut tims_written = 0usize;

    for path in &entries {
        let raw = std::fs::read(path)?;
        let scan = tim_scan::scan_entry(&raw);
        if scan.hits.is_empty() && only_hits {
            continue;
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let display_name = display_name_for(&stem, names.as_ref());

        let raw_hits = scan
            .hits
            .iter()
            .filter(|(s, _)| matches!(s, tim_scan::Source::Raw))
            .count();
        let lzs_hits = scan.hits.len() - raw_hits;
        let total_px: u64 = scan
            .hits
            .iter()
            .map(|(_, h)| h.width as u64 * h.height as u64)
            .sum();
        let notes = if scan.lzs_ok { "" } else { "(lzs:no)" };
        if !scan.hits.is_empty() {
            println!(
                "{:<32}  {:>4}  {:>4}  {:>5}  {:>5}  {}",
                display_name,
                raw_hits,
                lzs_hits,
                scan.hits.len(),
                total_px,
                notes
            );
            entries_with_hits += 1;
            total_hits += scan.hits.len();
        } else if !only_hits {
            println!(
                "{:<32}  {:>4}  {:>4}  {:>5}  {:>5}  {}",
                display_name, "-", "-", "-", "-", notes
            );
        }

        if let Some(out_root) = out {
            let entry_dir = out_root.join(&display_name);
            for (src, hit) in &scan.hits {
                let (buf, label) = match src {
                    tim_scan::Source::Raw => (raw.as_slice(), "raw".to_string()),
                    tim_scan::Source::Lzs(idx) => {
                        let Some(section) = scan.lzs_sections.get(*idx) else {
                            continue;
                        };
                        (section.as_slice(), format!("lzs{}", idx))
                    }
                };
                let end = (hit.offset + hit.byte_len).min(buf.len());
                let slab = &buf[hit.offset..end];
                std::fs::create_dir_all(&entry_dir)?;
                let fname = format!(
                    "{}_off{:06X}_{}x{}_{}bpp.tim",
                    label, hit.offset, hit.width, hit.height, hit.bpp
                );
                std::fs::write(entry_dir.join(&fname), slab)?;
                tims_written += 1;
            }
        }
    }

    println!();
    println!(
        "{} entries with TIMs, {} hits total",
        entries_with_hits, total_hits
    );
    if out.is_some() {
        println!("wrote {} TIM files", tims_written);
    }
    Ok(())
}

/// `asset tim-catalog <PROT.DAT>` - flat-scan the whole archive image for
/// standard TIMs and emit the per-TIM catalog (jPSXdec parity).
pub(crate) fn tim_catalog_cmd(
    prot: &std::path::Path,
    out: Option<&std::path::Path>,
    rollup: bool,
) -> Result<()> {
    let catalog = tim_catalog::build_from_path(prot)?;

    if let Some(out) = out {
        let body = if out.extension().and_then(|e| e.to_str()) == Some("tsv") {
            tim_catalog::to_tsv(&catalog)
        } else {
            serde_json::to_string_pretty(&catalog)?
        };
        std::fs::write(out, body)?;
        println!("wrote {} TIMs -> {}", catalog.len(), out.display());
    } else {
        println!(
            "{:>5}  {:>10}  {:>6}  {:>14}  {:>9}  {:>4}  {:>4}  {:>9}  fnv1a",
            "id", "abs_off", "sector", "entry", "off_in", "bpp", "pal", "bytes"
        );
        println!("{}", "-".repeat(92));
        for t in &catalog {
            let entry = match t.entry_index {
                Some(i) => i.to_string(),
                None => "gap".to_string(),
            };
            println!(
                "{:>5}  0x{:08X}  {:>6}  {:>14}  0x{:07X}  {:>4}  {:>4}  {:>9}  {:016x}  {}x{}",
                t.id,
                t.abs_offset,
                t.sector,
                entry,
                t.offset_in_entry,
                t.bpp,
                t.clut_count,
                t.byte_len,
                t.fnv1a,
                t.width,
                t.height,
            );
        }
    }

    if rollup {
        let r = tim_catalog::rollup(&catalog);
        println!("rollup: count={} digest=0x{:016x}", r.count, r.digest);
    }
    Ok(())
}

/// `asset tim-deep-catalog <PROT.DAT>` - LZS-decompress every entry and
/// catalog the standard TIMs hiding inside the compressed sections.
pub(crate) fn tim_deep_catalog_cmd(
    prot: &std::path::Path,
    out: Option<&std::path::Path>,
    rollup: bool,
) -> Result<()> {
    let catalog = tim_deep_catalog::build_from_path(prot)?;

    if let Some(out) = out {
        let body = if out.extension().and_then(|e| e.to_str()) == Some("tsv") {
            tim_deep_catalog::to_tsv(&catalog)
        } else {
            serde_json::to_string_pretty(&catalog)?
        };
        std::fs::write(out, body)?;
        println!("wrote {} deep TIMs -> {}", catalog.len(), out.display());
    } else {
        println!(
            "{:>5}  {:>5}  {:>3}  {:>9}  {:>4}  {:>4}  {:>9}  fnv1a",
            "id", "entry", "sec", "off_in", "bpp", "pal", "bytes"
        );
        println!("{}", "-".repeat(78));
        for t in &catalog {
            println!(
                "{:>5}  {:>5}  {:>3}  0x{:07X}  {:>4}  {:>4}  {:>9}  {:016x}  {}x{}",
                t.id,
                t.entry_index,
                t.lzs_section,
                t.offset_in_section,
                t.bpp,
                t.clut_count,
                t.byte_len,
                t.fnv1a,
                t.width,
                t.height,
            );
        }
    }

    if rollup {
        let r = tim_deep_catalog::rollup(&catalog);
        println!("rollup: count={} digest=0x{:016x}", r.count, r.digest);
    }
    Ok(())
}

/// Encode an RGBA8 buffer to a PNG file via the `png` crate.
pub(crate) fn write_png(path: &Path, rgba: &[u8], w: u32, h: u32) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(rgba)?;
    Ok(())
}

/// `asset tim-render-distinct` - decode each DISTINCT cataloged TIM (deduped by
/// content fingerprint) to `<out>/<fnv>.png` and write `<out>/manifest.tsv`.
///
/// The output is decoded Sony pixel data - it is meant for local inspection
/// (driving the `tim_labels` visual categorization) and must never be
/// committed. Only the resulting fingerprint -> label table is committed.
pub(crate) fn tim_render_distinct_cmd(prot: &Path, out: &Path, tier: RenderTier) -> Result<()> {
    use std::collections::HashMap;

    std::fs::create_dir_all(out)?;
    let prot_bytes = std::fs::read(prot)?;

    let want_raw = matches!(tier, RenderTier::Raw | RenderTier::Both);
    let want_deep = matches!(tier, RenderTier::Deep | RenderTier::Both);

    // Per fingerprint: (tier, width, height, bpp, clut_count, count seen).
    struct Rec {
        tier: &'static str,
        w: u32,
        h: u32,
        bpp: u32,
        clut: usize,
        count: u32,
    }
    let mut recs: HashMap<u64, Rec> = HashMap::new();

    // Decode one TIM (palette 0) from a byte slice and, if its fingerprint is
    // new, write the PNG. Always bumps the per-fingerprint count.
    let mut emit = |fnv: u64,
                    bytes: &[u8],
                    tier: &'static str,
                    w: u32,
                    h: u32,
                    bpp: u32,
                    clut: usize|
     -> Result<()> {
        if let Some(r) = recs.get_mut(&fnv) {
            r.count += 1;
            return Ok(());
        }
        recs.insert(
            fnv,
            Rec {
                tier,
                w,
                h,
                bpp,
                clut,
                count: 1,
            },
        );
        if let Ok(tim) = legaia_tim::parse(bytes)
            && let Ok(rgba) = legaia_tim::decode_rgba8(&tim, 0)
        {
            let pw = tim.pixel_width() as u32;
            let ph = tim.image.h as u32;
            if pw > 0 && ph > 0 {
                write_png(&out.join(format!("{fnv:016x}.png")), &rgba, pw, ph)?;
            }
        }
        Ok(())
    };

    if want_raw {
        let archive = legaia_prot::archive::Archive::open(prot)?;
        let catalog = tim_catalog::build(&prot_bytes, &archive.entries);
        for t in &catalog {
            let off = t.abs_offset as usize;
            emit(
                t.fnv1a,
                &prot_bytes[off..off + t.byte_len],
                "raw",
                t.width,
                t.height,
                t.bpp,
                t.clut_count,
            )?;
        }
    }

    if want_deep {
        // Decompress each entry once; decode the deep TIMs it hosts.
        let mut archive = legaia_prot::archive::Archive::open(prot)?;
        let deep = tim_deep_catalog::build(&archive, &prot_bytes);
        let entries = archive.entries.clone();
        let mut buf = Vec::new();
        // Group deep hits by entry to decompress once per entry.
        let mut by_entry: HashMap<u32, Vec<&tim_deep_catalog::DeepCatalogTim>> = HashMap::new();
        for t in &deep {
            by_entry.entry(t.entry_index).or_default().push(t);
        }
        for entry in &entries {
            let Some(hits) = by_entry.get(&entry.index) else {
                continue;
            };
            archive.read_entry(entry, &mut buf)?;
            let Ok(sections) = legaia_lzs::decompress_container(&buf) else {
                continue;
            };
            for t in hits {
                let Some(section) = sections.get(t.lzs_section as usize) else {
                    continue;
                };
                let o = t.offset_in_section as usize;
                if o + t.byte_len > section.len() {
                    continue;
                }
                emit(
                    t.fnv1a,
                    &section[o..o + t.byte_len],
                    "deep",
                    t.width,
                    t.height,
                    t.bpp,
                    t.clut_count,
                )?;
            }
        }
    }

    // Manifest, sorted by fingerprint for a stable file.
    let mut rows: Vec<(u64, &Rec)> = recs.iter().map(|(&f, r)| (f, r)).collect();
    rows.sort_by_key(|&(f, _)| f);
    let mut tsv = String::from("fnv1a\ttier\twidth\theight\tbpp\tclut_count\tcount\n");
    for (fnv, r) in &rows {
        tsv.push_str(&format!(
            "{:016x}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            fnv, r.tier, r.w, r.h, r.bpp, r.clut, r.count
        ));
    }
    std::fs::write(out.join("manifest.tsv"), tsv)?;
    println!(
        "rendered {} distinct textures -> {} (NOT for commit: decoded pixel data)",
        rows.len(),
        out.display()
    );
    Ok(())
}
