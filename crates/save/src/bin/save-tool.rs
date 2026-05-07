//! `save-tool` — read PSX memory-card images, surface Legaia save
//! blocks, and parse the per-character record region.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
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
        /// Number of characters to read (default 5 — Legaia's max party).
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
