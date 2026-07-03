use std::path::{Path, PathBuf};

use anyhow::Result;
use legaia_asset::{categorize, validate};
use legaia_prot::cdname;

pub(crate) fn validate_blocks(
    dir: &PathBuf,
    cdname_path: Option<&PathBuf>,
    counts_str: &str,
    only_hits: bool,
) -> Result<()> {
    let counts: Vec<usize> = counts_str
        .split(',')
        .map(|s| s.trim().parse::<usize>())
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| anyhow::anyhow!("invalid --counts: {}", e))?;

    // Build a name lookup table from `<index>_<name>.BIN` filenames produced
    // by prot-extract. Index → full path.
    let mut index_to_path: std::collections::BTreeMap<u32, PathBuf> = Default::default();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some((idx_str, _)) = stem.split_once('_') else {
            continue;
        };
        let Ok(idx) = idx_str.parse::<u32>() else {
            continue;
        };
        index_to_path.insert(idx, p);
    }

    // Pick which entries to test: CDNAME block heads, or all entries.
    let test_indices: Vec<(u32, String)> = if let Some(p) = cdname_path {
        let map = cdname::parse(p)?;
        map.into_iter().collect()
    } else {
        index_to_path
            .keys()
            .map(|&i| (i, format!("entry_{:04}", i)))
            .collect()
    };

    let mut hits = 0usize;
    let mut tried = 0usize;
    for (start_idx, block_name) in &test_indices {
        let Some(path) = index_to_path.get(start_idx) else {
            continue;
        };
        tried += 1;
        let raw = match std::fs::read(path) {
            Ok(r) => r,
            Err(_) => continue,
        };

        // Pick the best count: highest one that yields layout_ok with at
        // least one descriptor decoding cleanly to a known magic OR all
        // descriptors decoding without error.
        let mut best: Option<(usize, legaia_asset::ContainerReport)> = None;
        for &n in &counts {
            if raw.len() < 8 + n * 8 {
                continue;
            }
            let report = match validate(&raw, n) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let any_magic_ok = report
                .descriptors
                .iter()
                .any(|d| d.magic_ok && d.decoded_as.is_some());
            let all_decoded = report
                .descriptors
                .iter()
                .all(|d| d.decoded_as.is_some() || d.error.is_some() && !report.layout_ok);
            // Prefer reports with layout_ok and a real magic hit.
            let score =
                (report.layout_ok as u8) * 4 + (any_magic_ok as u8) * 2 + (all_decoded as u8);
            let prev_score = best.as_ref().map(|(_, r)| {
                (r.layout_ok as u8) * 4
                    + (r.descriptors
                        .iter()
                        .any(|d| d.magic_ok && d.decoded_as.is_some()) as u8)
                        * 2
            });
            if prev_score.is_none_or(|ps| score > ps) {
                best = Some((n, report));
            }
        }

        let Some((count, report)) = best else {
            if !only_hits {
                println!(
                    "[skip] block={} idx={} {}: no count fits",
                    block_name,
                    start_idx,
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
            }
            continue;
        };

        let any_magic_ok = report
            .descriptors
            .iter()
            .any(|d| d.magic_ok && d.decoded_as.is_some());
        let is_hit = report.layout_ok && any_magic_ok;
        if !is_hit && only_hits {
            continue;
        }
        if is_hit {
            hits += 1;
        }
        let tag = if is_hit { "HIT " } else { "miss" };
        println!(
            "{}  block={:<16} idx={:>4}  count={}  layout_ok={}  file={}",
            tag,
            block_name,
            start_idx,
            count,
            report.layout_ok,
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        for d in &report.descriptors {
            let mode = d.decoded_as.unwrap_or("--");
            let mag = d.decoded_magic.as_deref().unwrap_or("        ");
            let magic_tag = if d.magic_ok { "OK " } else { "?? " };
            let len = d
                .decoded_len
                .map(|n| n.to_string())
                .unwrap_or_else(|| "-".into());
            let err = d.error.as_deref().unwrap_or("");
            println!(
                "    [{:>2}] type=0x{:02X} {:>8}  size={:>8}  off=0x{:08X}  mode={:<3}  magic={} {}  decoded={:>8}  {}",
                d.index,
                d.type_byte,
                d.type_name,
                d.size,
                d.data_offset,
                mode,
                mag,
                magic_tag,
                len,
                err
            );
        }
    }
    eprintln!("validate done: {} blocks tested, {} hits", tried, hits);
    Ok(())
}

pub(crate) fn categorize_dir(
    dir: &PathBuf,
    out: Option<&PathBuf>,
    top_signatures: usize,
    examples: usize,
    filter_class: Option<&str>,
    cdname_path: Option<&Path>,
) -> Result<()> {
    use std::collections::BTreeMap;

    #[derive(serde::Serialize)]
    struct PerFile<'a> {
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cdname_slot: Option<String>,
        #[serde(flatten)]
        report: &'a categorize::FileReport,
    }

    #[derive(serde::Serialize)]
    struct ClassBucket<'a> {
        class: &'static str,
        count: usize,
        total_bytes: usize,
        examples: Vec<&'a String>,
    }

    #[derive(serde::Serialize)]
    struct SignatureBucket {
        first_u32_hex: String,
        count: usize,
        examples: Vec<String>,
    }

    #[derive(serde::Serialize)]
    struct SlotHistogramRow {
        slot: u32,
        count: usize,
        scene_examples: Vec<String>,
    }

    #[derive(serde::Serialize)]
    struct Report<'a> {
        scan_root: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        filter_class: Option<&'a str>,
        n_files: usize,
        per_file: Vec<PerFile<'a>>,
        by_class: Vec<ClassBucket<'a>>,
        top_signatures: Vec<SignatureBucket>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        slot_histogram: Vec<SlotHistogramRow>,
    }

    // Load CDNAME map if requested.
    let cdname_map = cdname_path
        .map(cdname::parse)
        .transpose()
        .map_err(|e| anyhow::anyhow!("CDNAME parse error: {e}"))?;

    // Helper: PROT entry index from a filename like `0042_scene.BIN`.
    let entry_index_from_name =
        |name: &str| -> Option<u32> { name.split('_').next()?.parse::<u32>().ok() };

    // Helper: resolve a PROT entry index to `scene+slot` or `raw_N`.
    let slot_label = |idx: u32| -> String {
        if let Some(ref map) = cdname_map
            && let Some(scene) = cdname::block_for(map, idx)
        {
            // Find the scene start index to compute the slot offset.
            let start = map.range(..=idx).next_back().map(|(k, _)| *k).unwrap_or(0);
            return format!("{}+{}", scene, idx - start);
        }
        format!("raw_{}", idx)
    };

    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    paths.sort();

    let mut reports: Vec<categorize::FileReport> = Vec::with_capacity(paths.len());
    let mut names: Vec<String> = Vec::with_capacity(paths.len());
    let mut slot_labels: Vec<Option<String>> = Vec::with_capacity(paths.len());

    for p in &paths {
        let buf = match std::fs::read(p) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("read {}: {}", p.display(), e);
                continue;
            }
        };
        let name = p
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.display().to_string());
        reports.push(categorize::classify(&buf));
        let lbl = if cdname_map.is_some() {
            entry_index_from_name(&name).map(&slot_label)
        } else {
            None
        };
        slot_labels.push(lbl);
        names.push(name);
    }

    let n_files = reports.len();

    // Group by class (unfiltered, for the summary table).
    let mut by_class: BTreeMap<&'static str, (usize, usize, Vec<&String>)> = BTreeMap::new();
    for (i, r) in reports.iter().enumerate() {
        let entry = by_class.entry(r.class.name()).or_insert((0, 0, Vec::new()));
        entry.0 += 1;
        entry.1 += r.size;
        if entry.2.len() < examples {
            entry.2.push(&names[i]);
        }
    }

    // Slot histogram (only for filtered class when --cdname is given).
    let mut slot_hist: BTreeMap<u32, (usize, Vec<String>)> = BTreeMap::new();
    if cdname_map.is_some() {
        for (i, r) in reports.iter().enumerate() {
            let matches_filter = filter_class.map(|f| r.class.name() == f).unwrap_or(true);
            if !matches_filter {
                continue;
            }
            if let Some(idx) = entry_index_from_name(&names[i]) {
                // slot offset within the scene block
                let slot_offset = if let Some(ref map) = cdname_map {
                    let start = map.range(..=idx).next_back().map(|(k, _)| *k).unwrap_or(0);
                    idx - start
                } else {
                    idx
                };
                let entry = slot_hist.entry(slot_offset).or_insert((0, Vec::new()));
                entry.0 += 1;
                if entry.1.len() < 3 {
                    entry.1.push(names[i].clone());
                }
            }
        }
    }
    let mut slot_histogram: Vec<SlotHistogramRow> = slot_hist
        .into_iter()
        .map(|(slot, (count, ex))| SlotHistogramRow {
            slot,
            count,
            scene_examples: ex,
        })
        .collect();
    slot_histogram.sort_by(|a, b| b.count.cmp(&a.count).then(a.slot.cmp(&b.slot)));

    // Group by first-u32 signature (filtered).
    let mut by_sig: BTreeMap<u32, (usize, Vec<String>)> = BTreeMap::new();
    for (i, r) in reports.iter().enumerate() {
        let matches_filter = filter_class.map(|f| r.class.name() == f).unwrap_or(true);
        if !matches_filter {
            continue;
        }
        let Some(sig) = r.first_u32 else { continue };
        let entry = by_sig.entry(sig).or_insert((0, Vec::new()));
        entry.0 += 1;
        if entry.1.len() < 3 {
            entry.1.push(names[i].clone());
        }
    }
    let mut sigs: Vec<SignatureBucket> = by_sig
        .into_iter()
        .map(|(s, (c, ex))| SignatureBucket {
            first_u32_hex: format!("0x{:08X}", s),
            count: c,
            examples: ex,
        })
        .collect();
    sigs.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(a.first_u32_hex.cmp(&b.first_u32_hex))
    });
    sigs.truncate(top_signatures);

    let class_buckets: Vec<ClassBucket> = by_class
        .iter()
        .map(|(name, (c, b, ex))| ClassBucket {
            class: name,
            count: *c,
            total_bytes: *b,
            examples: ex.clone(),
        })
        .collect();

    // Console summary (unfiltered class table).
    let filter_note = filter_class
        .map(|f| format!(" (filter: {f})"))
        .unwrap_or_default();
    println!("=== categorize: {} files{} ===", n_files, filter_note);
    println!();
    println!(
        "{:>5}  {:>9}  class                      examples",
        "n", "MB"
    );
    let mut sorted_classes: Vec<_> = by_class.iter().collect();
    sorted_classes.sort_by_key(|b| std::cmp::Reverse(b.1.0));
    for (name, (count, total, ex)) in &sorted_classes {
        let marker = if filter_class == Some(name) {
            " <--"
        } else {
            ""
        };
        let mb = (*total as f64) / (1024.0 * 1024.0);
        let ex_str = ex
            .iter()
            .take(3)
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "{:>5}  {:>9.2}  {:<26} {}{}",
            count, mb, name, ex_str, marker
        );
    }
    println!();

    // Per-file listing for the filtered class (when filter_class is given).
    if let Some(fc) = filter_class {
        println!("=== entries matching class '{}' ===", fc);
        let mut printed = 0usize;
        for (i, r) in reports.iter().enumerate() {
            if r.class.name() != fc {
                continue;
            }
            let lbl = slot_labels[i].as_deref().unwrap_or("");
            let lbl_col = if lbl.is_empty() {
                String::new()
            } else {
                format!("  [{}]", lbl)
            };
            println!(
                "  {:>9}B  h={:.2}  head={}{}  {}",
                r.size, r.entropy_bits, r.head, lbl_col, names[i]
            );
            printed += 1;
        }
        println!("  ({} entries)", printed);
        println!();
    }

    if !slot_histogram.is_empty() {
        println!(
            "=== slot histogram (class '{}') ===",
            filter_class.unwrap_or("all")
        );
        println!("{:>5}  {:>5}  scene examples", "slot", "count");
        for row in &slot_histogram {
            let ex = row.scene_examples.join(", ");
            println!("{:>5}  {:>5}  {}", row.slot, row.count, ex);
        }
        println!();
    }

    println!("=== top {} first-u32 signatures ===", sigs.len());
    println!("{:>5}  {:<12}  examples", "n", "signature");
    for sb in &sigs {
        let ex = sb
            .examples
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        println!("{:>5}  {:<12}  {}", sb.count, sb.first_u32_hex, ex);
    }

    let per_file: Vec<PerFile> = reports
        .iter()
        .zip(names.iter())
        .zip(slot_labels.iter())
        .filter(|((r, _), _)| filter_class.map(|f| r.class.name() == f).unwrap_or(true))
        .map(|((r, name), lbl)| PerFile {
            path: name.clone(),
            cdname_slot: lbl.clone(),
            report: r,
        })
        .collect();

    let report = Report {
        scan_root: dir.display().to_string(),
        filter_class,
        n_files,
        per_file,
        by_class: class_buckets,
        top_signatures: sigs,
        slot_histogram,
    };

    let out_path: PathBuf = out.cloned().unwrap_or_else(|| dir.join("categorize.json"));
    let json = serde_json::to_string_pretty(&report)?;
    std::fs::write(&out_path, json)?;
    eprintln!("wrote {}", out_path.display());
    Ok(())
}
