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

/// CDNAME `#define` numbers are **raw in-RAM PROT-TOC indices** - the index
/// space `FUN_8003E8A8` consumes - not extraction-entry indices. The boot TOC
/// loader copies `PROT.DAT` verbatim (8-byte header included) to `0x801C70F0`,
/// so `raw index = extraction index + 2`. Pinned by loader-constant
/// identities: `PLAYER1..4 = 0x361..0x364` (`battle_data 865..868`),
/// `monster.snd = 0x37D` (`monster_se 893`), `summon.dat`/`readef.DAT` =
/// `0x37F`/`0x380` (`bat_back_dat 895..896`), overlay slots `0x381+`
/// (`xxx_dat 897`). See `docs/formats/cdname.md` § numbering space and
/// `scripts/asset-investigation/cdname_shift_analysis.py`.
pub const RAW_TOC_INDEX_OFFSET: u32 = 2;

/// Resolve the CDNAME block that retail-semantically covers an **extraction**
/// entry index (the `NNNN` in `extracted/PROT/NNNN_*.BIN`): looks up
/// `extraction_index + RAW_TOC_INDEX_OFFSET` in the define map, since the
/// `#define` numbers live in the raw-TOC space (see [`RAW_TOC_INDEX_OFFSET`]).
///
/// `prot-extract`'s filename labels apply the define numbers as extraction
/// indices directly and are therefore shifted +2 relative to this; that
/// default naming is kept stable, so use this helper when the *retail*
/// meaning of an entry matters.
pub fn block_for_extraction_index(map: &IndexMap, extraction_index: u32) -> Option<&str> {
    block_for(map, extraction_index.saturating_add(RAW_TOC_INDEX_OFFSET))
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

/// Resolve a scene/block name to its retail **extraction-frame** entry window
/// `[start, end_exclusive)` - the index space of `extracted/PROT/NNNN_*.BIN`
/// files and of [`crate::archive::Archive::entries`].
///
/// CDNAME `#define` numbers are raw-TOC indices ([`RAW_TOC_INDEX_OFFSET`]), so
/// the window [`block_range_for_name`] returns is `+2` from the extraction
/// frame: applying it unshifted drops the block's first two retail entries
/// (the `.MAP` + sidecars) and bleeds in the *next* block's first two - the
/// mis-framing behind the historical rikuroa/geremi MAN mixup.
///
/// Head defines whose raw start sits inside the TOC's header rows
/// (`raw_start < RAW_TOC_INDEX_OFFSET`: `init_data 0`, `gameover_data 1`)
/// keep their legacy unshifted windows - the `-2` conversion has no content
/// to land on there, and the entries they name (`0000_init_data`, ...) are
/// what consumers load. Mirrors `Scene::load` in `legaia-engine-core`.
pub fn block_range_for_name_extraction(map: &IndexMap, name: &str) -> Option<(u32, u32)> {
    let (raw_start, raw_end) = block_range_for_name(map, name)?;
    if raw_start < RAW_TOC_INDEX_OFFSET {
        Some((raw_start, raw_end))
    } else {
        Some((
            raw_start - RAW_TOC_INDEX_OFFSET,
            raw_end.saturating_sub(RAW_TOC_INDEX_OFFSET),
        ))
    }
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

    #[test]
    fn block_range_for_name_extraction_shifts_to_retail_frame() {
        // Real CDNAME tail around the effect cluster: `befect_data 872`,
        // `player_data 876`. The retail befect block is EXTRACTION 870..874
        // (etim/etmd/vdf/efect); the unshifted window (872..876) misses
        // etim/etmd and bleeds into player_data.
        let map = parse_str("#define befect_data 872\n#define player_data 876\n").unwrap();
        assert_eq!(
            block_range_for_name_extraction(&map, "befect_data"),
            Some((870, 874))
        );
        // The last block's open end (u32::MAX) also shifts; callers clamp to
        // the actual archive size either way.
        assert_eq!(
            block_range_for_name_extraction(&map, "player_data"),
            Some((874, u32::MAX - 2))
        );
        assert_eq!(block_range_for_name_extraction(&map, "missing"), None);
    }

    #[test]
    fn block_range_for_name_extraction_keeps_head_define_legacy_windows() {
        // `init_data 0` / `gameover_data 1` sit inside the raw TOC's header
        // rows; the -2 conversion has no content to land on, so their legacy
        // unshifted windows are kept (mirrors `Scene::load`).
        let map =
            parse_str("#define init_data 0\n#define gameover_data 1\n#define town01 3\n").unwrap();
        assert_eq!(
            block_range_for_name_extraction(&map, "init_data"),
            Some((0, 1))
        );
        assert_eq!(
            block_range_for_name_extraction(&map, "gameover_data"),
            Some((1, 3))
        );
        assert_eq!(
            block_range_for_name_extraction(&map, "town01"),
            Some((1, u32::MAX - 2))
        );
    }

    #[test]
    fn block_for_extraction_index_applies_raw_toc_offset() {
        // Real CDNAME tail: `battle_data 865`, `monster_data 869`,
        // `sound_data 870`. The monster stat archive is byte-pinned at
        // EXTRACTION entry 867 (raw 869) - the raw-space lookup must name it
        // `monster_data`, while the naive define-as-extraction-index reading
        // (what the extractor's filenames use) calls 867 `battle_data`.
        let map = parse_str(
            "#define battle_data 865\n#define monster_data 869\n#define sound_data 870\n",
        )
        .unwrap();
        assert_eq!(block_for_extraction_index(&map, 867), Some("monster_data"));
        assert_eq!(block_for(&map, 867), Some("battle_data"));
        // PLAYER1..4 = raw 0x361..0x364 = extraction 863..866.
        assert_eq!(block_for_extraction_index(&map, 863), Some("battle_data"));
        assert_eq!(block_for_extraction_index(&map, 866), Some("battle_data"));
        assert_eq!(block_for_extraction_index(&map, 868), Some("sound_data"));
        // Below the first block start there is no name in either space.
        assert_eq!(block_for_extraction_index(&map, 862), None);
        // Saturating add: u32::MAX must not panic.
        let edge = parse_str("#define edge 0\n").unwrap();
        assert_eq!(block_for_extraction_index(&edge, u32::MAX), Some("edge"));
    }
}
