use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use legaia_prot::archive::{Archive, Header};
use legaia_prot::{cdname, timpack};
use serde::Serialize;

#[derive(Parser)]
#[command(
    name = "prot-extract",
    version,
    about = "Extract Legaia PROT.DAT-style archives"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print header, TOC summary, and per-entry table.
    List {
        /// PROT.DAT (or DMY.DAT) archive - the file `disc-extract extract`
        /// or `legaia-extract` writes into the output root.
        prot: PathBuf,
        /// Optional CDNAME.TXT (extracted next to PROT.DAT on the disc) to
        /// label entries with their block name.
        #[arg(long)]
        cdname: Option<PathBuf>,
    },
    /// Extract every entry to `<out>`; also unpack TIM packs and write manifest.json.
    Extract {
        /// PROT.DAT (or DMY.DAT) archive - the file `disc-extract extract`
        /// or `legaia-extract` writes into the output root.
        prot: PathBuf,
        /// Output directory (created if missing; resolved against the
        /// current directory when relative). One `.BIN` per entry.
        out: PathBuf,
        /// Optional CDNAME.TXT (extracted next to PROT.DAT on the disc) to
        /// name each entry file after its block, e.g. `0004_town01.BIN`.
        #[arg(long)]
        cdname: Option<PathBuf>,
    },
}

/// Restore default SIGPIPE behaviour so piping into `head` etc. exits
/// quietly instead of panicking on a broken-pipe write.
fn reset_sigpipe() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

fn main() -> Result<()> {
    reset_sigpipe();
    match Cli::parse().cmd {
        Cmd::List { prot, cdname } => list(&prot, cdname.as_deref()),
        Cmd::Extract { prot, out, cdname } => extract(&prot, &out, cdname.as_deref()),
    }
}

/// Open the archive with the path attached to any error.
fn open_archive(prot: &Path) -> Result<Archive> {
    Archive::open(prot).with_context(|| format!("opening PROT archive {}", prot.display()))
}

fn list(prot: &Path, cdname_path: Option<&Path>) -> Result<()> {
    let archive = open_archive(prot)?;
    print_header(
        &archive.header,
        archive.toc.len(),
        archive.entries.len(),
        archive.file_len(),
    );

    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };

    println!();
    println!(
        "{:>5}  {:>10}  {:>10}  {:>10}  block",
        "idx", "byte_off", "size", "lba"
    );
    for e in &archive.entries {
        let block = names
            .as_ref()
            .and_then(|m| cdname::block_for(m, e.index))
            .unwrap_or("");
        println!(
            "{:>5}  0x{:08X}  {:>10}  {:>10}  {}",
            e.index, e.byte_offset, e.size_bytes, e.start_lba, block
        );
    }
    Ok(())
}

fn extract(prot: &Path, out: &Path, cdname_path: Option<&Path>) -> Result<()> {
    let mut archive = open_archive(prot)?;
    print_header(
        &archive.header,
        archive.toc.len(),
        archive.entries.len(),
        archive.file_len(),
    );

    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };

    std::fs::create_dir_all(out)?;
    let tim_root = out.join("tim");
    let mut buf = Vec::new();
    let mut manifest_entries = Vec::with_capacity(archive.entries.len());

    let entries = archive.entries.clone();
    for entry in &entries {
        archive.read_entry(entry, &mut buf)?;

        let block = names
            .as_ref()
            .and_then(|m| cdname::block_for(m, entry.index));
        let stem = match block {
            Some(b) => format!("{:04}_{}", entry.index, b),
            None => format!("{:04}", entry.index),
        };

        let bin_name = format!("{}.BIN", stem);
        std::fs::write(out.join(&bin_name), &buf)?;

        let mut tim_paths: Vec<String> = Vec::new();
        let is_tim = timpack::is_tim_pack(&buf);
        if is_tim {
            let items = timpack::unpack(&buf);
            if !items.is_empty() {
                let dir = tim_root.join(&stem);
                std::fs::create_dir_all(&dir)?;
                for (i, item) in items.iter().enumerate() {
                    let ext = timpack::detected_ext(item);
                    let name = format!("{}_{}.{}", stem, i, ext);
                    std::fs::write(dir.join(&name), item)?;
                    tim_paths.push(format!("tim/{}/{}", stem, name));
                }
            }
        }

        manifest_entries.push(ManifestEntry {
            index: entry.index,
            block: block.map(str::to_owned),
            byte_offset: format!("0x{:08X}", entry.byte_offset),
            size: entry.size_bytes,
            lba: entry.start_lba,
            size_sectors: entry.size_sectors,
            is_tim_pack: is_tim,
            path: bin_name,
            tim_items: tim_paths,
        });
    }

    let manifest = Manifest {
        source: prot.display().to_string(),
        header: archive.header.clone(),
        toc_len: archive.toc.len(),
        entries: manifest_entries,
    };
    let json = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(out.join("manifest.json"), json)?;

    println!();
    println!(
        "extracted {} entries into {} (manifest.json written)",
        archive.entries.len(),
        out.display()
    );
    Ok(())
}

fn print_header(h: &Header, toc_len: usize, entries: usize, file_len: u64) {
    println!(
        "header: offset=0x{:X}  file_num={}  header_sectors={}  toc_u32={}  entries={}  archive={}b",
        h.header_offset, h.file_num, h.header_sectors, toc_len, entries, file_len
    );
}

#[derive(Serialize)]
struct Manifest {
    source: String,
    header: Header,
    toc_len: usize,
    entries: Vec<ManifestEntry>,
}

#[derive(Serialize)]
struct ManifestEntry {
    index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    block: Option<String>,
    byte_offset: String,
    size: u64,
    lba: u32,
    size_sectors: u32,
    is_tim_pack: bool,
    path: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tim_items: Vec<String>,
}
