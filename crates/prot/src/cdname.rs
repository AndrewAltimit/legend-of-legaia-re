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
    // `start` comes from a parsed CDNAME index, which `parse_str` accepts as
    // any `u32` - a hostile map could declare `#define foo 4294967295`, and
    // `start + 1` would then overflow (panic in debug). Saturate so the
    // exclusive lower bound stays in range; a `u32::MAX` start simply has no
    // following block.
    let next_start = start.saturating_add(1);
    let end = map
        .range(next_start..)
        .next()
        .map(|(k, _)| *k)
        .unwrap_or(u32::MAX);
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_str_reads_defines_and_ignores_junk() {
        let text = "\
// a comment line
#define town01 5
#define battle01 10
not a define
#define malformed
#define bad_index notanumber
#define dungeon 20
";
        let map = parse_str(text).unwrap();
        assert_eq!(map.get(&5).map(String::as_str), Some("town01"));
        assert_eq!(map.get(&10).map(String::as_str), Some("battle01"));
        assert_eq!(map.get(&20).map(String::as_str), Some("dungeon"));
        // malformed / non-numeric lines are skipped, not panicked on.
        assert_eq!(map.len(), 3);
    }

    #[test]
    fn block_for_inherits_most_recent_block() {
        let map = parse_str("#define a 0\n#define b 5\n#define c 10\n").unwrap();
        assert_eq!(block_for(&map, 0), Some("a"));
        assert_eq!(block_for(&map, 4), Some("a"));
        assert_eq!(block_for(&map, 5), Some("b"));
        assert_eq!(block_for(&map, 100), Some("c"));
    }

    #[test]
    fn block_range_for_name_finds_bounds() {
        let map = parse_str("#define a 0\n#define b 5\n#define c 10\n").unwrap();
        assert_eq!(block_range_for_name(&map, "a"), Some((0, 5)));
        assert_eq!(block_range_for_name(&map, "b"), Some((5, 10)));
        assert_eq!(block_range_for_name(&map, "c"), Some((10, u32::MAX)));
        assert_eq!(block_range_for_name(&map, "missing"), None);
    }

    #[test]
    fn block_range_for_name_max_u32_index_does_not_overflow() {
        // A hostile CDNAME can declare a block at u32::MAX; `start + 1` used to
        // panic in debug. Must return a sane range with no following block.
        let map = parse_str("#define edge 4294967295\n").unwrap();
        assert_eq!(
            block_range_for_name(&map, "edge"),
            Some((u32::MAX, u32::MAX))
        );
    }

    #[test]
    fn parse_str_empty_is_empty_map() {
        assert!(parse_str("").unwrap().is_empty());
    }
}
