use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;

pub type IndexMap = BTreeMap<u32, String>;

pub fn parse(path: &Path) -> Result<IndexMap> {
    let text = std::fs::read_to_string(path)?;
    parse_str(&text)
}

/// Parse a CDNAME map from a string slice (useful for in-memory / WASM usage).
pub fn parse_str(text: &str) -> Result<IndexMap> {
    let mut out = IndexMap::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("#define") else {
            continue;
        };
        let mut it = rest.split_whitespace();
        let Some(name) = it.next() else { continue };
        let Some(idx) = it.next() else { continue };
        let Ok(idx) = idx.parse::<u32>() else {
            continue;
        };
        out.insert(idx, name.to_string());
    }
    Ok(out)
}

/// Find the named block whose start index ≤ entry_index. CDNAME.TXT lists the
/// first index of each block, so consecutive PROT entries inherit the name of
/// the most recent declared block.
pub fn block_for(map: &IndexMap, entry_index: u32) -> Option<&str> {
    map.range(..=entry_index)
        .next_back()
        .map(|(_, v)| v.as_str())
}

/// Resolve a scene/block name to its `[start, end_exclusive)` PROT-entry
/// index range. Returns `None` if `name` isn't declared in the map. The
/// upper bound is the next-declared block's start (or `u32::MAX` if it's
/// the last block - caller should clamp to actual archive size).
///
/// Used by the asset viewer's `--scene <NAME>` flag to assemble the bundle
/// of PROT entries that comprise one field/town scene (matches what the
/// runtime's `FUN_8001f7c0` + `FUN_800255b8` loaders pull together).
pub fn block_range_for_name(map: &IndexMap, name: &str) -> Option<(u32, u32)> {
    let start = map.iter().find(|(_, v)| *v == name).map(|(k, _)| *k)?;
    let end = map
        .range((start + 1)..)
        .next()
        .map(|(k, _)| *k)
        .unwrap_or(u32::MAX);
    Some((start, end))
}
