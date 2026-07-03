use std::path::{Path, PathBuf};

use crate::common::*;
use anyhow::Result;
use legaia_asset::{battle_data_pack, effect_bundle, field_pack};
use legaia_prot::cdname;

pub(crate) fn field_pack_one(input: &PathBuf, all_slots: bool, groups: bool) -> Result<()> {
    let raw = std::fs::read(input)?;
    let Some(fp) = field_pack::detect(&raw) else {
        anyhow::bail!(
            "no field-pack signature in {} ({} bytes)",
            input.display(),
            raw.len()
        );
    };
    let (preamble_lo, preamble_hi) = fp.preamble_range();
    let (assets_lo, assets_hi) = fp.assets_range();
    println!("file:           {}", input.display());
    println!(
        "size:           {} bytes (0x{:X})",
        fp.file_size, fp.file_size
    );
    println!(
        "preamble:       0x{:X}..0x{:X} ({} bytes)",
        preamble_lo,
        preamble_hi,
        preamble_hi - preamble_lo
    );
    println!(
        "magic @         0x{:X} (= 0x{:08X})",
        fp.magic_offset,
        field_pack::MAGIC
    );
    println!(
        "schema table:   0x{:X}..0x{:X} ({} entries × 4 = {} bytes)",
        fp.table_offset,
        fp.table_offset + field_pack::SCHEMA_SIZE,
        field_pack::RECORD_COUNT,
        field_pack::SCHEMA_SIZE
    );
    println!(
        "assets region:  0x{:X}..0x{:X} ({} bytes)",
        assets_lo,
        assets_hi,
        assets_hi - assets_lo
    );
    println!();
    println!("schema slots:");
    let n = fp.slots.len();
    let show: Vec<usize> = if all_slots {
        (0..n).collect()
    } else {
        let mut v: Vec<usize> = (0..n.min(8)).collect();
        if n > 16 {
            v.push(usize::MAX); // sentinel for ellipsis
            v.extend((n - 8)..n);
        } else {
            v.extend(8..n);
        }
        v
    };
    for i in show {
        if i == usize::MAX {
            println!("  ...");
            continue;
        }
        let s = &fp.slots[i];
        match s.size {
            Some(sz) => println!(
                "  [{:>2}] off=0x{:>5X}  size={:>5} (0x{:X})",
                i, s.offset, sz, sz
            ),
            None => println!("  [{:>2}] off=0x{:>5X}  size=  ?", i, s.offset),
        }
    }
    if groups {
        println!();
        println!("slot size groups (slots sharing the same size = same record kind):");
        for (size, idxs) in fp.slot_size_groups() {
            let head: Vec<String> = idxs.iter().take(10).map(|i| i.to_string()).collect();
            let tail = if idxs.len() > 10 {
                format!(" … (+{} more)", idxs.len() - 10)
            } else {
                String::new()
            };
            println!(
                "  size={:>5} (0x{:X})  count={:>3}  slots={}{}",
                size,
                size,
                idxs.len(),
                head.join(","),
                tail
            );
        }
    }
    Ok(())
}

pub(crate) fn field_pack_scan(dir: &Path, only_hits: bool) -> Result<()> {
    let mut files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    println!(
        "{:<32}  {:>9}  {:>10}  {:>9}  {:>9}",
        "entry", "size", "table_off", "preamble", "assets"
    );
    println!("{}", "-".repeat(76));
    let mut hits = 0usize;
    let mut total = 0usize;
    for path in &files {
        total += 1;
        let raw = std::fs::read(path)?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        match field_pack::detect(&raw) {
            Some(fp) => {
                hits += 1;
                let (assets_lo, assets_hi) = fp.assets_range();
                println!(
                    "{:<32}  {:>9}  0x{:>8X}  {:>9}  {:>9}",
                    stem,
                    fp.file_size,
                    fp.table_offset,
                    fp.magic_offset,
                    assets_hi - assets_lo,
                );
            }
            None => {
                if !only_hits {
                    println!(
                        "{:<32}  {:>9}  {:>10}  {:>9}  {:>9}",
                        stem,
                        raw.len(),
                        "-",
                        "-",
                        "-"
                    );
                }
            }
        }
    }
    println!();
    println!(
        "{} of {} entries match the field-pack signature",
        hits, total
    );
    Ok(())
}

pub(crate) fn effect_bundle_one(input: &PathBuf, all_slots: bool) -> Result<()> {
    let raw = std::fs::read(input)?;
    let Some(eb) = effect_bundle::detect(&raw) else {
        anyhow::bail!(
            "no effect-bundle signature in {} ({} bytes)",
            input.display(),
            raw.len()
        );
    };
    let (preamble_lo, preamble_hi) = eb.preamble_range();
    let (assets_lo, assets_hi) = eb.assets_range();
    println!("file:           {}", input.display());
    println!(
        "size:           {} bytes (0x{:X})",
        eb.file_size, eb.file_size
    );
    println!(
        "preamble:       0x{:X}..0x{:X} ({} bytes)",
        preamble_lo,
        preamble_hi,
        preamble_hi - preamble_lo
    );
    println!(
        "magic @         0x{:X} (= 0x{:08X})",
        eb.magic_offset,
        effect_bundle::MAGIC
    );
    println!(
        "header_a:       0x{:08X}{}",
        eb.header_a,
        if eb.header_a == effect_bundle::HEADER_A {
            " (= constant)"
        } else {
            " (UNEXPECTED)"
        }
    );
    println!(
        "header_b:       0x{:08X}{}",
        eb.header_b,
        if eb.header_b == effect_bundle::HEADER_B {
            " (= constant)"
        } else {
            " (UNEXPECTED)"
        }
    );
    println!(
        "schema table:   0x{:X}..0x{:X} ({} entries × 4 = {} bytes)",
        eb.table_offset,
        eb.table_offset + effect_bundle::TABLE_SIZE,
        effect_bundle::RECORD_COUNT,
        effect_bundle::TABLE_SIZE
    );
    println!(
        "assets region:  0x{:X}..0x{:X} ({} bytes)",
        assets_lo,
        assets_hi,
        assets_hi - assets_lo
    );
    println!();
    println!("asset region content:");
    let n_tmds = eb.assets.tmds.len();
    let n_tims = eb.assets.tims.len();
    println!(
        "  {} TMD(s) - {} master + {} sub (HEADER_A reserves 1 master + 28 sub = 29 slots)",
        n_tmds,
        n_tmds.min(1),
        n_tmds.saturating_sub(1),
    );
    if let Some(&master) = eb.assets.tmds.first() {
        println!("    master TMD @ 0x{:X} (= assets_start)", master);
    }
    if eb.assets.tmds.len() > 1 {
        let preview: Vec<String> = eb.assets.tmds[1..]
            .iter()
            .take(4)
            .map(|o| format!("0x{:X}", o))
            .collect();
        let suffix = if eb.assets.tmds.len() > 5 {
            ", …"
        } else {
            ""
        };
        println!("    sub-TMDs   @ {}{}", preview.join(", "), suffix);
    }
    println!("  {} TIM(s)", n_tims);
    if !eb.assets.tims.is_empty() {
        let preview: Vec<String> = eb
            .assets
            .tims
            .iter()
            .take(4)
            .map(|o| format!("0x{:X}", o))
            .collect();
        let suffix = if eb.assets.tims.len() > 4 {
            ", …"
        } else {
            ""
        };
        println!("    @ {}{}", preview.join(", "), suffix);
    }
    println!();
    println!("schema slots:");
    let n = eb.slots.len();
    let show: Vec<usize> = if all_slots {
        (0..n).collect()
    } else {
        let mut v: Vec<usize> = (0..n.min(8)).collect();
        if n > 16 {
            v.push(usize::MAX); // sentinel for ellipsis
            v.extend((n - 8)..n);
        } else {
            v.extend(8..n);
        }
        v
    };
    for i in show {
        if i == usize::MAX {
            println!("  ...");
            continue;
        }
        let s = &eb.slots[i];
        match s.size {
            Some(sz) => println!(
                "  [{:>2}] off=0x{:>5X}  size={:>5} (0x{:X})",
                i, s.offset, sz, sz
            ),
            None => println!("  [{:>2}] off=0x{:>5X}  size=  ?", i, s.offset),
        }
    }
    Ok(())
}

pub(crate) fn effect_bundle_scan(dir: &Path, only_hits: bool) -> Result<()> {
    let mut files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    println!(
        "{:<32}  {:>9}  {:>10}  {:>9}  {:>9}",
        "entry", "size", "table_off", "preamble", "assets"
    );
    println!("{}", "-".repeat(76));
    let mut hits = 0usize;
    let mut total = 0usize;
    for path in &files {
        total += 1;
        let raw = std::fs::read(path)?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        match effect_bundle::detect(&raw) {
            Some(eb) => {
                hits += 1;
                let (assets_lo, assets_hi) = eb.assets_range();
                println!(
                    "{:<32}  {:>9}  0x{:>8X}  {:>9}  {:>9}",
                    stem,
                    eb.file_size,
                    eb.table_offset,
                    eb.magic_offset,
                    assets_hi - assets_lo,
                );
            }
            None => {
                if !only_hits {
                    println!(
                        "{:<32}  {:>9}  {:>10}  {:>9}  {:>9}",
                        stem,
                        raw.len(),
                        "-",
                        "-",
                        "-"
                    );
                }
            }
        }
    }
    println!();
    println!(
        "{} of {} entries match the effect-bundle signature",
        hits, total
    );
    Ok(())
}

pub(crate) fn battle_data_pack_one(input: &Path, out: Option<&Path>) -> Result<()> {
    let raw = std::fs::read(input)?;
    let pack = battle_data_pack::parse(&raw)?;
    println!("file       : {}", input.display());
    println!("file size  : {} bytes (0x{:x})", raw.len(), raw.len());
    println!(
        "table_offset: 0x{:x}, records: {}, data_base: 0x{:x}",
        pack.table_offset,
        pack.records.len(),
        pack.data_base
    );
    println!(
        "{:>3} {:>4} {:>10} {:>10} {:>10} {:>6}",
        "rec", "id", "slot_size", "data_off", "dec_size", "tmd"
    );
    let mut tmds = 0usize;
    let mut total_decoded = 0usize;
    if let Some(out) = out {
        std::fs::create_dir_all(out)?;
    }
    for r in &pack.records {
        let entry = battle_data_pack::decode_record(&raw, &pack, r.index);
        match entry {
            Ok(e) => {
                let dec_size = e.bytes.len();
                let tmd_tag = match &e.tmd_range {
                    Some(rng) => {
                        tmds += 1;
                        format!("{}..{}", rng.start, rng.end)
                    }
                    None => "-".into(),
                };
                println!(
                    "{:>3} 0x{:02x} 0x{:08x} 0x{:08x} {:>10} {:>6}",
                    r.index, r.id, r.size, r.data_offset, dec_size, tmd_tag
                );
                total_decoded += dec_size;
                if let Some(out_dir) = out {
                    let fname = format!("rec{:03}_id{:02x}.bin", r.index, r.id);
                    std::fs::write(out_dir.join(fname), &e.bytes)?;
                }
            }
            Err(err) => {
                println!(
                    "{:>3} 0x{:02x} 0x{:08x} 0x{:08x} FAIL: {}",
                    r.index, r.id, r.size, r.data_offset, err
                );
            }
        }
    }
    println!();
    println!(
        "{} records / {} bytes decompressed / {} TMDs found",
        pack.records.len(),
        total_decoded,
        tmds
    );
    Ok(())
}

pub(crate) fn battle_data_pack_scan(
    dir: &Path,
    cdname_path: Option<&Path>,
    only_hits: bool,
) -> Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };

    println!(
        "{:<32}  {:>7}  {:>10}  {:>5}  notes",
        "entry", "records", "dec_bytes", "tmds"
    );
    println!("{}", "-".repeat(80));
    let mut total_hits = 0usize;
    let mut total_recs = 0usize;
    let mut total_tmds = 0usize;
    for path in &entries {
        let raw = std::fs::read(path)?;
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let display_name = display_name_for(&stem, names.as_ref());
        match battle_data_pack::detect(&raw) {
            Some(pack) => {
                let mut dec_bytes = 0usize;
                let mut tmds = 0usize;
                for r in &pack.records {
                    if let Ok(e) = battle_data_pack::decode_record(&raw, &pack, r.index) {
                        dec_bytes += e.bytes.len();
                        if e.tmd_range.is_some() {
                            tmds += 1;
                        }
                    }
                }
                println!(
                    "{:<32}  {:>7}  {:>10}  {:>5}",
                    display_name,
                    pack.records.len(),
                    dec_bytes,
                    tmds
                );
                total_hits += 1;
                total_recs += pack.records.len();
                total_tmds += tmds;
            }
            None => {
                if !only_hits {
                    println!("{:<32}  {:>7}  {:>10}  {:>5}", display_name, "-", "-", "-");
                }
            }
        }
    }
    println!();
    println!(
        "{} entries match, {} records, {} TMDs found",
        total_hits, total_recs, total_tmds
    );
    Ok(())
}

pub(crate) fn scene_v12_one(input: &Path, dump_scripts: bool, max_scripts: usize) -> Result<()> {
    let buf = std::fs::read(input)?;
    let t = legaia_asset::scene_v12_table::detect(&buf)
        .ok_or_else(|| anyhow::anyhow!("not a scene_v12_table (header magic / algebra failed)"))?;

    println!("scene_v12_table @ {}", input.display());
    println!("  size:       {} bytes", buf.len());
    println!("  N:          {} ({:#x})", t.n, t.n);
    println!("  param:      {}", t.param);
    println!(
        "  fixup slots @ +{:#x}, +{:#x}, +{:#x} (zero on disc)",
        t.table_b_base(),
        t.n,
        t.table_a_base()
    );
    println!("  end_records: {:#x}", t.end_records());
    println!();

    // Inline records at +0x14: print compact, with a small group histogram.
    println!("inline records @ +0x14 ({} entries):", t.records.len());
    let mut by_b2: std::collections::BTreeMap<u8, usize> = std::collections::BTreeMap::new();
    let head_n = t.records.len().min(24);
    for (i, r) in t.records.iter().take(head_n).enumerate() {
        println!(
            "  [{:3}] b0={:02x} b1={:02x} b2={:02x} flag={:02x}",
            i, r.b0, r.b1, r.b2, r.flag
        );
    }
    if t.records.len() > head_n {
        println!("  ... {} more not shown", t.records.len() - head_n);
    }
    for r in &t.records {
        *by_b2.entry(r.b2).or_insert(0) += 1;
    }
    print!("  b2 histogram:");
    for (b2, n) in &by_b2 {
        print!(" {:02x}×{}", b2, n);
    }
    println!();
    println!();

    // Event-script prescript at +0x800.
    println!(
        "event scripts @ +0x800: {} records, frame-opener rate {:.0}%",
        t.scripts.len(),
        100.0 * t.frame_opener_rate()
    );
    if dump_scripts {
        let show = t.scripts.len().min(max_scripts);
        for (i, r) in t.scripts.iter().take(show).enumerate() {
            let head = &buf[r.start..r.end.min(r.start + 16)];
            print!(
                "  [{:3}] @{:#06x} len={:5} {}",
                i,
                r.start,
                r.len(),
                if r.frame_opener { "OPENER" } else { "      " }
            );
            print!("  ");
            for b in head {
                print!("{:02x} ", b);
            }
            println!();
        }
        if t.scripts.len() > show {
            println!("  ... {} more not shown", t.scripts.len() - show);
        }
    }
    Ok(())
}

pub(crate) fn scene_v12_scan(
    dir: &Path,
    cdname_path: Option<&Path>,
    only_hits: bool,
) -> Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };

    println!(
        "{:<32}  {:>5}  {:>5}  {:>5}  {:>7}  {:>4}  notes",
        "entry", "N", "param", "b2#", "scripts", "fo%"
    );
    println!("{}", "-".repeat(80));
    let mut hits = 0usize;
    let mut total_scripts = 0usize;
    let mut high_fo = 0usize;
    for path in &entries {
        let Ok(buf) = std::fs::read(path) else {
            continue;
        };
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let display = display_name_for(&stem, names.as_ref());
        match legaia_asset::scene_v12_table::detect(&buf) {
            Some(t) => {
                let unique_b2 = t
                    .records
                    .iter()
                    .map(|r| r.b2)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len();
                let rate = t.frame_opener_rate();
                println!(
                    "{:<32}  {:>5}  {:>5}  {:>5}  {:>7}  {:>3}%",
                    display,
                    t.n,
                    t.param,
                    unique_b2,
                    t.scripts.len(),
                    (rate * 100.0).round() as i32
                );
                hits += 1;
                total_scripts += t.scripts.len();
                if rate >= 0.5 {
                    high_fo += 1;
                }
            }
            None => {
                if !only_hits {
                    println!(
                        "{:<32}  {:>5}  {:>5}  {:>5}  {:>7}  {:>4}",
                        display, "-", "-", "-", "-", "-"
                    );
                }
            }
        }
    }
    println!();
    println!(
        "{hits} matches, {total_scripts} total event-script records, {high_fo} with frame-opener rate ≥ 50%"
    );
    Ok(())
}
