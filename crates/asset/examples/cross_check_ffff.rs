//! Cross-check: of the PROT entries with first u32 = 0xFFFFFFFF,
//! how many classify as `stage_geometry` vs other classes?
use legaia_asset::categorize;
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
    let mut hits = 0usize;
    let mut classes: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut samples_per_class: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    for p in &entries {
        let raw = std::fs::read(p).unwrap();
        if raw.len() < 4 {
            continue;
        }
        if u32::from_le_bytes(raw[0..4].try_into().unwrap()) != 0xFFFFFFFF {
            continue;
        }
        hits += 1;
        let report = categorize::classify(&raw);
        let name = report.class.name();
        *classes.entry(name).or_insert(0) += 1;
        let v = samples_per_class.entry(name).or_default();
        if v.len() < 3 {
            v.push(p.file_name().unwrap().to_string_lossy().into_owned());
        }
    }
    println!("of {} files with first u32 = 0xFFFFFFFF:", hits);
    for (name, count) in &classes {
        println!("  {:>22}  {:>3}", name, count);
        for s in samples_per_class.get(name).unwrap_or(&Vec::new()) {
            println!("                            {}", s);
        }
    }
}
