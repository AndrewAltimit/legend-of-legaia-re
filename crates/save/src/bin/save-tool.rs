//! `save-tool` - read PSX memory-card images, surface Legaia save
//! blocks, and parse the per-character record region.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use legaia_save::card::{
    RETAIL_CHAR_RECORD_HEADER_SIZE, RETAIL_CHAR_RECORD_STRIDE, RETAIL_GAME_DATA_OFFSET,
    RETAIL_INVENTORY_OFFSET, RETAIL_INVENTORY_SIZE, RETAIL_STORY_FLAGS_OFFSET,
    RETAIL_STORY_FLAGS_SIZE, SAVE_BLOCK_MAGIC,
};
use legaia_save::{
    BLOCK_SIZE, CHARACTER_RECORD_SIZE, CharacterRecord, Party, parse_card, read_block,
    walk_directory, write_block,
};

#[derive(Parser)]
#[command(name = "save-tool", about = "Legaia memory-card / save inspector")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List every directory entry in a PSX memory-card image (.mcr).
    Dir { path: PathBuf },
    /// Find every active save block and report its product code + block chain.
    Saves { path: PathBuf },
    /// Parse a 0x414-byte character record from a file or memory-card slice
    /// (`--block N --offset 0xNN` to slice a save block).
    Character {
        path: PathBuf,
        /// If set, treat `path` as a memory-card image and read block `N`.
        #[arg(long)]
        block: Option<u8>,
        /// Byte offset within the file or block where the character record
        /// begins (default 0).
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// Round-trip parse → write → parse a character record region and
    /// confirm the bytes are identical.
    Roundtrip {
        path: PathBuf,
        /// If set, treat `path` as a memory-card image and read block `N`.
        #[arg(long)]
        block: Option<u8>,
        /// Byte offset within the file or block where the character record
        /// begins (default 0).
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// Parse N consecutive character records starting at `--offset` and
    /// emit them as JSON.
    Party {
        path: PathBuf,
        #[arg(long)]
        block: Option<u8>,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Number of characters to read (default 5 - Legaia's max party).
        #[arg(long, default_value_t = 5)]
        count: usize,
    },
    /// Write a raw payload into a free block chain on a PSX memory-card
    /// image (.mcr). Modifies the card in place and prints the block index
    /// of the first block written.
    Write {
        /// Path to the PSX memory-card image (.mcr) to write into.
        #[arg(long)]
        card: PathBuf,
        /// Raw payload file (e.g. a party `.bin` from `save-tool party`).
        #[arg(long)]
        payload: PathBuf,
        /// Product code to stamp in the directory frame (max 20 chars).
        #[arg(long, default_value = "BASCUS-94254LEGAIA")]
        product: String,
    },
    /// Diff the SC save block of two memory-card images and surface
    /// every byte that differs. Designed to pin still-unknown SC-block
    /// fields (story flags, inventory) by capturing two saves on either
    /// side of a known state change (e.g. picking up an item, flipping
    /// a story flag) and reading off the resulting diff cluster.
    ///
    /// Diff regions are annotated against the documented retail SC-block
    /// layout (see docs/subsystems/save-screen.md):
    ///   * `0x0000..0x0200`  - icon header (palette + pixels)
    ///   * `0x0200..0x086F`  - display / global header (location, scenes,
    ///     plus the not-yet-pinned story flags + inventory)
    ///   * `0x086F..`        - 0x414-byte character records
    ///
    /// The "differing" cluster inside `0x0200..0x086F` is the field
    /// you're hunting for; its width gives you the type (4 bytes = u32
    /// story flags; 2-byte stride = inventory `(item_id, count)` array).
    ScDiff {
        /// First memory-card image (or raw SC-block file - 8192 bytes).
        a: PathBuf,
        /// Second memory-card image (or raw SC-block file).
        b: PathBuf,
        /// Active-save index inside the card (1-based - i.e. the Nth
        /// active save). Defaults to 1. Ignored when the input is a raw
        /// SC-block file (8192 bytes).
        #[arg(long, default_value_t = 1)]
        save_index: usize,
        /// Restrict the diff to a byte range inside the SC block.
        /// Default = `0x0000..0x086F` (skip character-record region;
        /// per-character changes are visible via `save-tool character`).
        #[arg(long)]
        range: Option<String>,
        /// Group consecutive differing bytes into one cluster. Default 0
        /// (every byte separately); pass e.g. 16 to coalesce into
        /// runs of contiguous differences.
        #[arg(long, default_value_t = 8)]
        coalesce: usize,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Dir { path } => dir(&path),
        Cmd::Saves { path } => saves(&path),
        Cmd::Character {
            path,
            block,
            offset,
        } => character(&path, block, offset),
        Cmd::Roundtrip {
            path,
            block,
            offset,
        } => roundtrip(&path, block, offset),
        Cmd::Party {
            path,
            block,
            offset,
            count,
        } => party(&path, block, offset, count),
        Cmd::Write {
            card,
            payload,
            product,
        } => write_cmd(&card, &payload, &product),
        Cmd::ScDiff {
            a,
            b,
            save_index,
            range,
            coalesce,
        } => sc_diff(&a, &b, save_index, range.as_deref(), coalesce),
    }
}

fn read_input(path: &PathBuf, block: Option<u8>, offset: usize) -> Result<Vec<u8>> {
    let raw = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let slice = match block {
        Some(b) => read_block(&raw, b)
            .ok_or_else(|| anyhow::anyhow!("block {b} out of range or card too small"))?
            .to_vec(),
        None => raw,
    };
    if offset >= slice.len() {
        anyhow::bail!(
            "offset 0x{:X} past end of input ({} bytes)",
            offset,
            slice.len()
        );
    }
    Ok(slice[offset..].to_vec())
}

fn dir(path: &PathBuf) -> Result<()> {
    let raw = std::fs::read(path)?;
    let entries = walk_directory(&raw)?;
    println!("block  state    size  next  product");
    println!("-----  -----    ----  ----  -------");
    for e in &entries {
        let state_label = match e.state {
            0x51 => "FIRST",
            0x52 => "MID  ",
            0x53 => "LAST ",
            0xA0 => "FREE ",
            _ => "?    ",
        };
        println!(
            " {:>4}  {} {:>6}  0x{:04X}  {}",
            e.block, state_label, e.file_size, e.next_block, e.product_code
        );
    }
    Ok(())
}

fn saves(path: &PathBuf) -> Result<()> {
    let raw = std::fs::read(path)?;
    let saves = parse_card(&raw)?;
    if saves.is_empty() {
        println!("(no active saves found in {})", path.display());
        return Ok(());
    }
    println!("active saves in {}:", path.display());
    for s in &saves {
        println!(
            "  block={} chain={:?} size={} bytes  product={}",
            s.block, s.block_chain, s.file_size, s.product_code
        );
    }
    Ok(())
}

fn character(path: &PathBuf, block: Option<u8>, offset: usize) -> Result<()> {
    let bytes = read_input(path, block, offset)?;
    if bytes.len() < CHARACTER_RECORD_SIZE {
        anyhow::bail!(
            "input too short for character record: {} < {}",
            bytes.len(),
            CHARACTER_RECORD_SIZE
        );
    }
    let rec = CharacterRecord::parse(&bytes[..CHARACTER_RECORD_SIZE])?;
    println!("{}", serde_json::to_string_pretty(&rec.snapshot())?);
    Ok(())
}

fn roundtrip(path: &PathBuf, block: Option<u8>, offset: usize) -> Result<()> {
    let bytes = read_input(path, block, offset)?;
    if bytes.len() < CHARACTER_RECORD_SIZE {
        anyhow::bail!(
            "input too short for character record: {} < {}",
            bytes.len(),
            CHARACTER_RECORD_SIZE
        );
    }
    let original = &bytes[..CHARACTER_RECORD_SIZE];
    let rec = CharacterRecord::parse(original)?;
    let written = rec.write();
    if written == original {
        println!(
            "OK: {}-byte character record round-trips exactly (block={:?} offset=0x{:X})",
            CHARACTER_RECORD_SIZE, block, offset
        );
        return Ok(());
    }
    let mismatches: Vec<usize> = (0..CHARACTER_RECORD_SIZE)
        .filter(|&i| original[i] != written[i])
        .take(20)
        .collect();
    anyhow::bail!(
        "round-trip mismatch at {} bytes (showing first 20: {:?})",
        mismatches.len(),
        mismatches
    );
}

fn party(path: &PathBuf, block: Option<u8>, offset: usize, count: usize) -> Result<()> {
    let bytes = read_input(path, block, offset)?;
    let need = count * CHARACTER_RECORD_SIZE;
    if bytes.len() < need {
        anyhow::bail!(
            "input too short for {} character records: {} < {}",
            count,
            bytes.len(),
            need
        );
    }
    let party = Party::parse(&bytes[..need])?;
    let snapshots: Vec<_> = party.members.iter().map(|r| r.snapshot()).collect();
    println!("{}", serde_json::to_string_pretty(&snapshots)?);
    Ok(())
}

fn write_cmd(card_path: &PathBuf, payload_path: &PathBuf, product: &str) -> Result<()> {
    let payload = std::fs::read(payload_path)
        .with_context(|| format!("read payload {}", payload_path.display()))?;
    let mut card =
        std::fs::read(card_path).with_context(|| format!("read card {}", card_path.display()))?;
    let block = write_block(&mut card, &payload, product)?;
    std::fs::write(card_path, &card)
        .with_context(|| format!("write card {}", card_path.display()))?;
    println!(
        "wrote {} bytes to block {} of {}",
        payload.len(),
        block,
        card_path.display()
    );
    Ok(())
}

#[allow(dead_code)]
const _BS: usize = BLOCK_SIZE;

/// Locate the Nth active SC save block in a memory-card image.
///
/// `save_index` is 1-based (1 = first active save). Returns the
/// 8192-byte block starting with `SC`.
fn locate_sc_block(card: &[u8], save_index: usize) -> Result<Vec<u8>> {
    if save_index == 0 {
        anyhow::bail!("--save-index must be >= 1");
    }
    let saves = parse_card(card)?;
    if save_index > saves.len() {
        anyhow::bail!(
            "save-index {save_index} > {} active saves in card",
            saves.len()
        );
    }
    let block = saves[save_index - 1].block;
    let raw = read_block(card, block)
        .ok_or_else(|| anyhow::anyhow!("block {block} out of range"))?
        .to_vec();
    if raw.len() < 2 || raw[..2] != SAVE_BLOCK_MAGIC {
        anyhow::bail!("block {block} does not start with SC magic");
    }
    Ok(raw)
}

/// Read either a memory-card image (and slice to the Nth active save
/// block) or a raw 8192-byte SC-block file.
fn read_sc_block(path: &Path, save_index: usize) -> Result<Vec<u8>> {
    let raw = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if raw.len() == BLOCK_SIZE && raw[..2] == SAVE_BLOCK_MAGIC {
        return Ok(raw);
    }
    locate_sc_block(&raw, save_index)
        .with_context(|| format!("locate SC block in {}", path.display()))
}

fn parse_range(s: &str, default_end: usize) -> Result<std::ops::Range<usize>> {
    let s = s.trim();
    let parse_num = |x: &str| -> Result<usize> {
        let x = x.trim();
        if let Some(rest) = x.strip_prefix("0x").or_else(|| x.strip_prefix("0X")) {
            Ok(usize::from_str_radix(rest, 16)?)
        } else {
            Ok(x.parse()?)
        }
    };
    if let Some((lo, hi)) = s.split_once("..") {
        let lo = parse_num(lo)?;
        let hi = if hi.is_empty() {
            default_end
        } else {
            parse_num(hi)?
        };
        Ok(lo..hi)
    } else {
        anyhow::bail!("range must be `LO..HI` (got {s:?})");
    }
}

/// Annotate a byte offset inside an SC block with its documented
/// region label (so the diff output is self-explanatory).
fn annotate_sc_offset(off: usize) -> &'static str {
    if off < 2 {
        "SC magic"
    } else if off < 4 {
        "icon flags"
    } else if off < 0x60 {
        "save title"
    } else if off < 0x80 {
        "icon palette"
    } else if off < 0x100 {
        "icon pixels"
    } else if off < RETAIL_GAME_DATA_OFFSET {
        "padding/icon-frame"
    } else if off < RETAIL_GAME_DATA_OFFSET + 0x008 {
        "location name"
    } else if off < RETAIL_GAME_DATA_OFFSET + 0x054 {
        "header (early)"
    } else if off < RETAIL_GAME_DATA_OFFSET + 0x060 {
        "primary char display name"
    } else if off < RETAIL_GAME_DATA_OFFSET + 0x208 {
        "header (mid)"
    } else if off < RETAIL_GAME_DATA_OFFSET + 0x210 {
        "scene CDNAME (current)"
    } else if off < RETAIL_GAME_DATA_OFFSET + 0x218 {
        "header (late-mid)"
    } else if off < RETAIL_GAME_DATA_OFFSET + 0x220 {
        "scene CDNAME (previous)"
    } else if off < RETAIL_GAME_DATA_OFFSET + RETAIL_CHAR_RECORD_HEADER_SIZE {
        // game+0x220..0x3C8: remaining global header fields (party gold is at
        // game+0x25C). The character-record array begins at game+0x3C8.
        "header (late)"
    } else if (RETAIL_STORY_FLAGS_OFFSET..RETAIL_STORY_FLAGS_OFFSET + RETAIL_STORY_FLAGS_SIZE)
        .contains(&off)
    {
        // The 512-byte story-flag bitmap physically overlaps record [3]'s tail.
        "story flags (record [3] tail)"
    } else if (RETAIL_INVENTORY_OFFSET..RETAIL_INVENTORY_OFFSET + RETAIL_INVENTORY_SIZE)
        .contains(&off)
    {
        // The 72-slot inventory also overlaps record [3]'s tail.
        "inventory (record [3] tail)"
    } else {
        let into_records = off - RETAIL_GAME_DATA_OFFSET - RETAIL_CHAR_RECORD_HEADER_SIZE;
        let rec_idx = into_records / RETAIL_CHAR_RECORD_STRIDE;
        let _rec_off = into_records % RETAIL_CHAR_RECORD_STRIDE;
        match rec_idx {
            0 => "char record [0]",
            1 => "char record [1]",
            2 => "char record [2]",
            3 => "char record [3]",
            4 => "char record [4]",
            _ => "trailing data",
        }
    }
}

#[derive(Debug)]
struct DiffCluster {
    start: usize,
    end: usize,
    a_bytes: Vec<u8>,
    b_bytes: Vec<u8>,
}

fn collect_diff_clusters(
    a: &[u8],
    b: &[u8],
    range: std::ops::Range<usize>,
    coalesce: usize,
) -> Vec<DiffCluster> {
    let mut clusters = Vec::new();
    let mut current: Option<DiffCluster> = None;
    let mut last_diff: Option<usize> = None;
    for off in range.clone() {
        let differs = a.get(off) != b.get(off);
        if differs {
            // Coalesce into the previous cluster only if (a) we're still
            // inside it or (b) the gap from the previous diff byte is
            // <= `coalesce`. With coalesce==0 every differing byte is
            // its own cluster.
            let extend = match (current.as_ref(), last_diff) {
                (Some(c), _) if off == c.end => true,
                (Some(_), Some(prev)) if coalesce > 0 && off - prev <= coalesce => true,
                _ => false,
            };
            if extend {
                if let Some(c) = current.as_mut() {
                    while c.end < off {
                        let i = c.end;
                        c.a_bytes.push(*a.get(i).unwrap_or(&0));
                        c.b_bytes.push(*b.get(i).unwrap_or(&0));
                        c.end += 1;
                    }
                    c.a_bytes.push(*a.get(off).unwrap_or(&0));
                    c.b_bytes.push(*b.get(off).unwrap_or(&0));
                    c.end = off + 1;
                }
            } else {
                if let Some(c) = current.take() {
                    clusters.push(c);
                }
                current = Some(DiffCluster {
                    start: off,
                    end: off + 1,
                    a_bytes: vec![*a.get(off).unwrap_or(&0)],
                    b_bytes: vec![*b.get(off).unwrap_or(&0)],
                });
            }
            last_diff = Some(off);
        }
    }
    if let Some(c) = current.take() {
        clusters.push(c);
    }
    clusters
}

fn sc_diff(
    a_path: &Path,
    b_path: &Path,
    save_index: usize,
    range_str: Option<&str>,
    coalesce: usize,
) -> Result<()> {
    let a = read_sc_block(a_path, save_index)?;
    let b = read_sc_block(b_path, save_index)?;
    if a.len() != b.len() {
        anyhow::bail!(
            "SC blocks have different lengths ({} vs {})",
            a.len(),
            b.len()
        );
    }
    let default_end = RETAIL_GAME_DATA_OFFSET + RETAIL_CHAR_RECORD_HEADER_SIZE;
    let range = match range_str {
        Some(s) => parse_range(s, a.len())?,
        None => 0..default_end,
    };
    if range.end > a.len() {
        anyhow::bail!("range end 0x{:X} > SC block length {}", range.end, a.len());
    }
    println!("a: {} ({} bytes)", a_path.display(), a.len());
    println!("b: {} ({} bytes)", b_path.display(), b.len());
    println!(
        "diff range: 0x{:04X}..0x{:04X}  coalesce={}",
        range.start, range.end, coalesce
    );
    let clusters = collect_diff_clusters(&a, &b, range.clone(), coalesce);
    let total_diff_bytes: usize = clusters.iter().map(|c| c.end - c.start).sum();
    println!(
        "{} differing cluster(s); {} bytes changed across {} bytes scanned",
        clusters.len(),
        total_diff_bytes,
        range.end - range.start
    );
    if clusters.is_empty() {
        return Ok(());
    }
    println!();
    println!(
        "{:>6}  {:>4}  {:<48}  {:<24}  {:<24}",
        "off", "len", "region", "a (hex)", "b (hex)"
    );
    println!("{}", "-".repeat(110));
    for c in &clusters {
        let len = c.end - c.start;
        let region = annotate_sc_offset(c.start);
        let preview = |bs: &[u8]| -> String {
            let mut out = String::new();
            for (i, b) in bs.iter().take(12).enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(&format!("{b:02X}"));
            }
            if bs.len() > 12 {
                out.push_str(" ...");
            }
            out
        };
        println!(
            "0x{:04X}  {:>4}  {:<48}  {:<24}  {:<24}",
            c.start,
            len,
            region,
            preview(&c.a_bytes),
            preview(&c.b_bytes),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cluster_no_gap_bridging_when_coalesce_zero() {
        // Adjacent diffs (off == prev_cluster.end) always coalesce
        // because there's no gap between them. The coalesce knob only
        // controls bridging across non-diff bytes - so with
        // coalesce==0 a 1-byte gap stays as a fresh cluster.
        let a = vec![0u8; 32];
        let mut b = a.clone();
        b[5] = 1;
        b[6] = 2; // adjacent to 5 - same cluster.
        b[8] = 3; // 1-byte gap from 6 - new cluster with coalesce==0.
        b[20] = 4;
        let cs = collect_diff_clusters(&a, &b, 0..32, 0);
        assert_eq!(cs.len(), 3);
        assert_eq!((cs[0].start, cs[0].end), (5, 7));
        assert_eq!((cs[1].start, cs[1].end), (8, 9));
        assert_eq!((cs[2].start, cs[2].end), (20, 21));
    }

    #[test]
    fn cluster_coalesces_adjacent_runs() {
        let a = vec![0u8; 32];
        let mut b = a.clone();
        b[5] = 1;
        b[6] = 2;
        b[20] = 3;
        let cs = collect_diff_clusters(&a, &b, 0..32, 8);
        assert_eq!(cs.len(), 2);
        // First cluster covers offsets 5..7 (back-to-back diffs).
        assert_eq!(cs[0].start, 5);
        assert_eq!(cs[0].end, 7);
        // Second cluster is the isolated diff at 20.
        assert_eq!(cs[1].start, 20);
        assert_eq!(cs[1].end, 21);
    }

    #[test]
    fn cluster_bridges_gap_within_coalesce_window() {
        let a = vec![0u8; 32];
        let mut b = a.clone();
        b[5] = 1;
        b[10] = 2; // gap of 4 between previous diff (5) and this diff (10)
        let cs = collect_diff_clusters(&a, &b, 0..32, 8);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].start, 5);
        assert_eq!(cs[0].end, 11);
    }

    #[test]
    fn annotate_sc_offset_pins_documented_regions() {
        assert_eq!(annotate_sc_offset(0), "SC magic");
        assert_eq!(annotate_sc_offset(0x60), "icon palette");
        assert_eq!(annotate_sc_offset(0x80), "icon pixels");
        assert_eq!(annotate_sc_offset(0x200), "location name");
        assert_eq!(
            annotate_sc_offset(0x208 + RETAIL_GAME_DATA_OFFSET),
            "scene CDNAME (current)"
        );
        // Late display header (before the record array at game+0x3C8).
        let header_late = RETAIL_GAME_DATA_OFFSET + 0x300;
        assert_eq!(annotate_sc_offset(header_late), "header (late)");
        // Inside char record [0] (its base is game+0x3C8).
        let rec0 = RETAIL_GAME_DATA_OFFSET + RETAIL_CHAR_RECORD_HEADER_SIZE + 4;
        assert_eq!(annotate_sc_offset(rec0), "char record [0]");
        // The story-flag bitmap and inventory overlap record [3]'s tail.
        assert_eq!(
            annotate_sc_offset(RETAIL_STORY_FLAGS_OFFSET + 4),
            "story flags (record [3] tail)"
        );
        assert_eq!(
            annotate_sc_offset(RETAIL_INVENTORY_OFFSET + 4),
            "inventory (record [3] tail)"
        );
    }

    #[test]
    fn parse_range_decimal_and_hex() {
        let r = parse_range("0..16", 32).unwrap();
        assert_eq!(r, 0..16);
        let r = parse_range("0x10..0x20", 32).unwrap();
        assert_eq!(r, 0x10..0x20);
        let r = parse_range("0x10..", 32).unwrap();
        assert_eq!(r, 0x10..32);
    }
}
