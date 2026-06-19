//! Disc-gated structural facts about the field-pack block, established by
//! scanning the real extracted PROT corpus.
//!
//! These pin the corrected understanding of the format (see
//! `docs/formats/field-pack.md`):
//!
//! 1. The field-pack **magic** (`0x01059B84`) prefixes the 97-entry schema in
//!    only a handful of entries, NOT 124.
//! 2. The 97-entry schema's signature also appears in further entries **without**
//!    the magic prefix - the magic is not load-bearing.
//! 3. The ~91 KB schema-indexed region that follows the schema is a
//!    **byte-identical global constant block** across the scene clusters that
//!    carry it (town01 and town0c agree byte-for-byte). It is therefore a
//!    shared template, not the per-scene field data - the per-scene payload is
//!    the preamble that precedes the block.
//!
//! Skips silently when `extracted/PROT/` is missing.

use std::path::PathBuf;

use legaia_asset::field_pack::{self, SCHEMA_SIZE};

/// Length of the schema-indexed region (abstract `0x60..0x16651`), i.e. the
/// span the schema covers, laid out immediately after the 388-byte table.
const REGION_LEN: usize = (field_pack::SCHEMA_LAST - field_pack::SCHEMA_FIRST) as usize; // 0x165F1

fn prot_dir() -> Option<PathBuf> {
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let d = PathBuf::from(p);
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

/// Byte signature of the first five schema entries (`0x60, 0x61, 0x20E9,
/// 0x4171, 0x61F9`) - enough to locate the schema with or without the magic.
fn schema_sig() -> Vec<u8> {
    let mut v = Vec::new();
    for x in [0x60u32, 0x61, 0x20E9, 0x4171, 0x61F9] {
        v.extend_from_slice(&x.to_le_bytes());
    }
    v
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[test]
fn field_pack_magic_is_rare_and_region_is_a_global_constant() {
    let Some(dir) = prot_dir() else {
        eprintln!("[skip] extracted/PROT/ missing");
        return;
    };

    let sig = schema_sig();
    let mut magic_entries: Vec<String> = Vec::new();
    let mut schema_entries: Vec<String> = Vec::new();
    // (entry_name, sha-ish fingerprint of the full REGION_LEN region)
    let mut full_region_fp: std::collections::BTreeMap<String, u64> = Default::default();

    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "BIN"))
        .collect();
    paths.sort();

    for path in &paths {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(path).unwrap();
        // Magic-prefixed field-pack (the strict detector).
        if field_pack::detect(&bytes).is_some() {
            magic_entries.push(name.clone());
        }
        // Schema signature, with or without the magic in front of it.
        if let Some(off) = find(&bytes, &sig) {
            schema_entries.push(name.clone());
            let region_start = off + SCHEMA_SIZE;
            if let Some(region) = bytes.get(region_start..region_start + REGION_LEN) {
                // Cheap content fingerprint (FNV-1a) to compare regions
                // without pulling in a hash crate.
                let mut h = 0xcbf29ce484222325u64;
                for &b in region {
                    h = (h ^ b as u64).wrapping_mul(0x100000001b3);
                }
                full_region_fp.insert(name.clone(), h);
            }
        }
    }

    // The magic is rare - a single-digit count, nowhere near "124". (Pinning
    // the exact corpus members keeps the doc honest if the disc changes.)
    assert!(
        magic_entries.len() <= 8,
        "field-pack magic should be rare, got {} entries: {magic_entries:?}",
        magic_entries.len()
    );
    assert!(
        magic_entries.iter().any(|n| n.contains("town01")),
        "expected the town01 cluster among the magic-bearing entries: {magic_entries:?}"
    );
    // More entries carry the schema signature than carry the magic - the magic
    // is not a required prefix of the block.
    assert!(
        schema_entries.len() > magic_entries.len(),
        "schema appears without the magic somewhere: schema={schema_entries:?} magic={magic_entries:?}"
    );

    // The full ~91 KB region is a global constant: every entry that carries a
    // FULL-length region must agree on the same fingerprint. (Truncated
    // regions - entries whose asset region is shorter than the schema span -
    // are excluded by the get() bound above.)
    let fps: Vec<u64> = full_region_fp.values().copied().collect();
    assert!(
        fps.len() >= 2,
        "expected at least two full-length regions to compare, got {}",
        fps.len()
    );
    let first = fps[0];
    assert!(
        fps.iter().all(|&f| f == first),
        "the schema-indexed region is NOT byte-identical across entries: {full_region_fp:?}"
    );
    // It must span more than one scene cluster (town01 + at least one other) -
    // proving it is a shared template, not per-scene data.
    let clusters: std::collections::BTreeSet<String> = full_region_fp
        .keys()
        .map(|n| {
            n.trim_start_matches(|c: char| c.is_ascii_digit() || c == '_')
                .to_string()
        })
        .collect();
    assert!(
        clusters.len() >= 2,
        "the constant region should appear in >=2 scene clusters, saw: {clusters:?}"
    );
}
