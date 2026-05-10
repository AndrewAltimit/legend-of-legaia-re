//! Inspect every PROT entry whose first u32 is 0xFFFFFFFF.
//! Goal: figure out if these share a structural pattern.
use std::collections::BTreeMap;

fn main() {
    let dir = std::path::Path::new("extracted/PROT");
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();
    let mut hits: Vec<(String, usize, [u8; 32])> = Vec::new();
    for p in &entries {
        let raw = std::fs::read(p).unwrap();
        if raw.len() < 32 {
            continue;
        }
        if u32::from_le_bytes(raw[0..4].try_into().unwrap()) != 0xFFFFFFFF {
            continue;
        }
        let mut head = [0u8; 32];
        head.copy_from_slice(&raw[..32]);
        hits.push((
            p.file_name().unwrap().to_string_lossy().into_owned(),
            raw.len(),
            head,
        ));
    }
    println!(
        "found {} entries with first u32 == 0xFFFFFFFF\n",
        hits.len()
    );

    // Group by file size bucket
    let mut size_hist: BTreeMap<usize, usize> = BTreeMap::new();
    for (_, sz, _) in &hits {
        *size_hist.entry(*sz).or_insert(0) += 1;
    }
    println!("file size distribution:");
    for (sz, c) in &size_hist {
        println!("  {:>10} bytes  ({:>3} files)", sz, c);
    }

    // For each hit, look at the offset of the first non-FF byte.
    // (head is only 32 bytes; we scan the full file below for accurate run length.)
    println!("\nleading-FF run lengths (full-file scan):");
    let mut full_leading: BTreeMap<usize, usize> = BTreeMap::new();
    for p in &entries {
        let raw = std::fs::read(p).unwrap();
        if raw.len() < 32 {
            continue;
        }
        if u32::from_le_bytes(raw[0..4].try_into().unwrap()) != 0xFFFFFFFF {
            continue;
        }
        let n = raw.iter().take_while(|b| **b == 0xFF).count();
        *full_leading.entry(n).or_insert(0) += 1;
    }
    for (n, c) in &full_leading {
        println!("  {:>6} leading FF bytes  ({:>3} files)", n, c);
    }

    // Sample a few entries - show first-non-FF byte offset and the next 32 bytes after it.
    println!("\nfirst 5 samples (showing first non-FF region):");
    for (name, sz, _) in hits.iter().take(5) {
        let raw = std::fs::read(dir.join(name)).unwrap();
        let nff = raw.iter().take_while(|b| **b == 0xFF).count();
        let dump_end = (nff + 32).min(raw.len());
        let hex: String = raw[nff..dump_end]
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(" ");
        println!(
            "  {} (size={}) leading_FF={}  next32: {}",
            name, sz, nff, hex
        );
    }
}
