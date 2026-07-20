use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};

pub type IndexMap = BTreeMap<u32, String>;

pub fn parse(path: &Path) -> Result<IndexMap> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading CDNAME map {}", path.display()))?;
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

/// Byte stride of a record in the retail in-RAM CDNAME name table
/// (`0x80088758`), from `sll v0,v0,0x4` in the copy loop.
pub const RETAIL_RECORD_STRIDE: usize = 0x10;

/// Offset of the little-endian `u16` index within a retail in-RAM CDNAME
/// record - `sb a0,0xc(v0)` / `sb v1,0xd(v0)`.
pub const RETAIL_INDEX_OFFSET: usize = 0xC;

/// Longest name that survives into the retail table as a clean C string:
/// [`RETAIL_INDEX_OFFSET`] − 1 = **11** bytes.
///
/// This is *not* a cap the loader enforces - the copy loop has no bound at all
/// (see [`retail_name_table`]). It is the longest name whose terminating byte
/// is still a table zero after the index store lands on `+0xC`/`+0xD`. A
/// 12-byte name is not "just fitting": it loses its terminator to the index's
/// low byte and reads back with the index bytes appended.
pub const RETAIL_NAME_CLEAN_CAPACITY: usize = RETAIL_INDEX_OFFSET - 1;

/// A retail-parsed CDNAME map. Names are **bytes**, not `String`: the loader
/// can leave raw index bytes inside a name (see [`retail_name_table`]), and
/// those are routinely not valid UTF-8.
pub type RetailNameMap = BTreeMap<u32, Vec<u8>>;

/// Emulate the retail loader's writes into the in-RAM name table at
/// `0x80088758`, returning `(table, indices_in_declaration_order)`.
///
/// The table is a flat array of [`RETAIL_RECORD_STRIDE`]-byte records that
/// starts zeroed (it is BSS). Per `#define` line the loader does exactly two
/// things to it:
///
/// 1. **Copy the name, unbounded and unterminated.** The loop at `0x8001d980`
///    is `sb v1,0x0(v0)` with `v0 = base + count*0x10 + i`, running while the
///    source byte is not `' '`. There is no length check and **no NUL is ever
///    written** - a name is only terminated by table bytes the copy did not
///    reach.
/// 2. **Store the index over `+0xC`/`+0xD`.** `sb a0,0xc(v0)` /
///    `sb v1,0xd(v0)` run *after* the copy, so they overwrite name bytes 12
///    and 13 if the name got that far.
///
/// The consequences, which the previous "names are capped at 12 bytes" reading
/// got wrong in both directions:
///
/// - **Bytes `+0xE` and `+0xF` survive.** They are past the index store, so a
///   name of 15 bytes keeps its byte 14. `move_program_no` does **not** become
///   `move_program`; it reads back as `move_program` + the two index bytes +
///   `o`.
/// - **A 12- or 13-byte name is not cleanly truncated either.** With no NUL of
///   its own it runs straight into the index bytes, so `monster_data` reads
///   back as `monster_data` + the index's two bytes. It is clean only in the
///   accidental case where the index's low byte is zero.
/// - **Clean C-string capacity is 11**, not 12 - see
///   [`RETAIL_NAME_CLEAN_CAPACITY`].
/// - **A name of 16+ bytes spills into the following record.** Indexing is flat
///   from the table base, so the overflow lands on the next record's bytes and
///   is then partly overwritten when that record is parsed. Whatever sits
///   between the next name's end and its index store survives. No shipped name
///   is long enough to trigger this (the longest is 15), but a modded
///   `CDNAME.TXT` can, and retail has no bounds check to stop it.
///
/// Growing the table as records are appended reproduces the spill faithfully:
/// a record is only zero-filled where the emulation has not already written.
///
// PORT: FUN_8001d8fc
// NOT WIRED: this port is deliberately the *lossy* reader and wiring it into a
// host would be a regression. Retail's loader writes names into a 16-byte
// stride with no bound and no terminator, then stores the entry index over
// bytes +0xC/+0xD, so every name of 12+ bytes comes back with the index
// overlaid inside it. Tooling reads CDNAME through the tolerant `parse_str`
// instead, and must keep doing so. Its job is to be the *other* answer: the
// disc-gated `cdname_retail_parse_disc` oracle asserts the two readers declare
// the same index set and that the mangling is exactly what the byte-level
// record model predicts. It becomes host-callable only if something needs to
// reproduce a retail-side name buffer verbatim - e.g. a randomizer feature
// that edits CDNAME.TXT and must predict what the retail loader will read
// back, including the 16+-byte spill into the following record.
pub fn retail_name_table(text: &str) -> (Vec<u8>, Vec<u32>) {
    let bytes = text.as_bytes();
    let mut table: Vec<u8> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut p = 0usize;

    // The loop condition is the '#' test: the first line that fails it ends the
    // parse, exactly as retail's `while (*p == '#')` does. There is no
    // "skip junk and keep going".
    while bytes.get(p) == Some(&b'#') {
        let rec = indices.len() * RETAIL_RECORD_STRIDE;
        // Zero-fill only what a previous record's spill has not already
        // written - shrinking here would destroy the spill retail keeps.
        if table.len() < rec + RETAIL_RECORD_STRIDE {
            table.resize(rec + RETAIL_RECORD_STRIDE, 0);
        }

        // Retail skips a fixed 8 bytes ("#define ") and copies until a space,
        // with no bound and no terminator.
        let name_start = p + 8;
        let mut n = 0usize;
        while let Some(&c) = bytes.get(name_start + n) {
            if c == b' ' {
                break;
            }
            let dst = rec + n;
            if table.len() <= dst {
                table.resize(dst + 1, 0);
            }
            table[dst] = c;
            n += 1;
        }

        // `p` now sits on the space that terminated the name; retail reads the
        // four bytes after it, folding in each digit and skipping non-digits
        // rather than stopping at the first one.
        p = name_start + n;
        let mut idx: u32 = 0;
        for k in 1..=4 {
            if let Some(&c) = bytes.get(p + k)
                && c.is_ascii_digit()
            {
                idx = idx * 10 + u32::from(c - b'0');
            }
        }

        // The index store runs last and lands on top of the name.
        table[rec + RETAIL_INDEX_OFFSET] = idx as u8;
        table[rec + RETAIL_INDEX_OFFSET + 1] = (idx >> 8) as u8;
        indices.push(idx);

        // Advance past the newline that ends this line. Retail scans forward
        // unbounded; stopping at end-of-input is the one deliberate deviation,
        // since running off the buffer is not a behaviour worth reproducing.
        match bytes[p..].iter().position(|&c| c == b'\n') {
            Some(nl) => p += nl + 1,
            None => break,
        }
    }
    (table, indices)
}

/// Read a record back out of a [`retail_name_table`] table as retail does:
/// a C string from the record base, stopping at the first zero byte. A name
/// with no zero inside its own record keeps running into the next one.
pub fn retail_name_at(table: &[u8], record: usize) -> Vec<u8> {
    let start = record * RETAIL_RECORD_STRIDE;
    let tail = &table[start.min(table.len())..];
    let end = tail.iter().position(|&c| c == 0).unwrap_or(tail.len());
    tail[..end].to_vec()
}

/// Parse a CDNAME map the way retail's loader does, rather than the way a
/// tolerant tool would, and read each record back as the C string retail would
/// see. See [`retail_name_table`] for the write semantics and
/// [`retail_name_at`] for the read.
///
/// Two behaviours [`parse_str`] deliberately does not reproduce, beyond the
/// name mangling: retail **stops at the first line that does not begin with
/// `#`**, and it emits a record even for an empty name (the copy loop is
/// simply skipped; the index store still runs).
///
/// On the shipped `CDNAME.TXT` the two readers agree on every **index**, so no
/// block is gained, lost or renumbered - but they disagree on every name of 12
/// bytes or more, of which the file has five. Tooling that matches CDNAME names
/// against the full `#define` spelling is comparing against something this
/// loader never produces. The disc-gated `cdname_retail_parse_disc` test pins
/// the index agreement and the exact per-name divergence.
///
/// Whether retail ever populates this table at all is a separate **open
/// question** - the loader's source branch is selected by a flag whose shipped
/// value is not settled. See `docs/formats/cdname.md` and
/// `docs/subsystems/boot.md`.
///
// REF: FUN_8001d8fc
// NOT WIRED: the crate's own consumers (`block_for`, `prot-extract` labels,
// scene windows) all use the tolerant `parse_str`. This retail model is
// exercised only by `tests/cdname_retail_parse_disc.rs`.
pub fn parse_retail_str(text: &str) -> RetailNameMap {
    let (table, indices) = retail_name_table(text);
    indices
        .iter()
        .enumerate()
        .map(|(n, &idx)| (idx, retail_name_at(&table, n)))
        .collect()
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
    fn retail_parse_stops_at_the_first_non_hash_line() {
        // The tolerant parser skips the junk line and picks up `after`; retail's
        // loop condition ends the map there.
        let text = "#define first 5\nnot a define\n#define after 9\n";
        let retail = parse_retail_str(text);
        assert_eq!(retail.get(&5).map(Vec::as_slice), Some(&b"first"[..]));
        assert_eq!(
            retail.get(&9),
            None,
            "retail must not resume past the break"
        );
        assert_eq!(retail.len(), 1);
        assert_eq!(parse_str(text).unwrap().len(), 2);
    }

    #[test]
    fn retail_name_bytes_past_the_index_store_survive() {
        // 15 bytes with index 972 (lo 0xCC, hi 0x03). The index store lands on
        // name bytes 12 and 13 only - byte 14 ('o') is past it and survives, so
        // the name does NOT truncate to "move_program".
        let map = parse_retail_str("#define move_program_no 972\n");
        assert_eq!(
            map.get(&972).map(Vec::as_slice),
            Some(&b"move_program\xcc\x03o"[..])
        );
    }

    #[test]
    fn retail_name_of_exactly_twelve_bytes_loses_its_terminator() {
        // 12 bytes fills 0..0xB, so the record has no zero of its own and the
        // read runs straight into the index bytes. "Fits in 12" is not clean.
        let map = parse_retail_str("#define monster_data 869\n");
        assert_eq!(
            map.get(&869).map(Vec::as_slice),
            Some(&b"monster_data\x65\x03"[..])
        );
        // ...unless the index's low byte happens to be zero, which terminates
        // it by accident.
        let lucky = parse_retail_str("#define monster_data 768\n");
        assert_eq!(
            lucky.get(&768).map(Vec::as_slice),
            Some(&b"monster_data"[..]),
            "index low byte 0 terminates the name by coincidence"
        );
    }

    #[test]
    fn retail_names_up_to_eleven_bytes_are_clean() {
        // 11 bytes is the longest that keeps a table zero as its terminator.
        let text = "#define abcdefghijk 999\n#define town01 5\n";
        let map = parse_retail_str(text);
        assert_eq!(map.get(&999).map(Vec::as_slice), Some(&b"abcdefghijk"[..]));
        assert_eq!(map.get(&5).map(Vec::as_slice), Some(&b"town01"[..]));
        assert_eq!(RETAIL_NAME_CLEAN_CAPACITY, 11);
    }

    #[test]
    fn retail_name_of_sixteen_plus_bytes_spills_into_the_next_record() {
        // Indexing is flat from the table base and there is no bound check, so
        // an 18-byte name writes over the next record's bytes 0 and 1. That
        // record's own (shorter) name then overwrites byte 0 only, leaving the
        // spilled byte at index 1 in place.
        let text = "#define abcdefghijklmnopqr 1\n#define x 2\n";
        let (table, indices) = retail_name_table(text);
        assert_eq!(indices, vec![1, 2]);
        // Record 1 byte 1 is the 18th byte of the first name ('r'), not a zero.
        assert_eq!(table[RETAIL_RECORD_STRIDE + 1], b'r');
        // So the second record reads back as its name plus the surviving spill.
        assert_eq!(
            parse_retail_str(text).get(&2).map(Vec::as_slice),
            Some(&b"xr"[..])
        );
    }

    #[test]
    fn retail_parse_folds_digits_leniently() {
        // Up to four bytes after the name are inspected; non-digits are skipped
        // rather than terminating the scan.
        assert_eq!(
            parse_retail_str("#define a 123\n")
                .get(&123)
                .map(Vec::as_slice),
            Some(&b"a"[..])
        );
        // Only the first four bytes past the name are read at all.
        assert_eq!(
            parse_retail_str("#define a 12345\n")
                .get(&1234)
                .map(Vec::as_slice),
            Some(&b"a"[..])
        );
        // A non-digit in the middle is skipped, not a terminator.
        assert_eq!(
            parse_retail_str("#define a 1x2\n")
                .get(&12)
                .map(Vec::as_slice),
            Some(&b"a"[..])
        );
    }

    #[test]
    fn retail_parse_emits_a_record_for_an_empty_name() {
        // The copy loop is skipped when the first name byte is already a space,
        // but the index store and the record counter still run.
        let map = parse_retail_str("#define  7\n");
        assert_eq!(map.get(&7).map(Vec::as_slice), Some(&b""[..]));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn retail_parse_handles_empty_and_unterminated_input() {
        assert!(parse_retail_str("").is_empty());
        // No trailing newline: the walk stops instead of running off the buffer.
        assert_eq!(parse_retail_str("#define a 1").len(), 1);
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
