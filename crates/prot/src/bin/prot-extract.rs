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
    /// Find which entry owns a PROT.DAT byte offset (reverse lookup).
    ///
    /// Entry extents partition the archive, so every offset has exactly one
    /// owner. Use `--in-entry` to hand it an offset your hex editor showed
    /// inside one extracted `.BIN`; it reports the owner and says plainly when
    /// the offset runs past that entry's end (which a `.BIN` written before
    /// the entry size was corrected can do).
    Locate {
        /// PROT.DAT (or DMY.DAT) archive.
        prot: PathBuf,
        /// Byte offset to locate. Absolute in PROT.DAT by default; hex (`0x…`)
        /// or decimal. With `--in-entry N`, this is instead the offset within
        /// entry N's extracted `.BIN` file.
        offset: String,
        /// Treat `offset` as relative to this extraction entry's `.BIN` file
        /// (e.g. the offset your hex editor shows in `0866_battle_data.BIN`).
        #[arg(long, value_name = "N")]
        in_entry: Option<u32>,
        /// Optional CDNAME.TXT to label entries with their block name.
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
        /// Deprecated no-op. Every `.BIN` is already the entry's own sectors
        /// (the span to the next entry) - there is no larger window to trim
        /// to. Accepted so existing invocations keep working.
        #[arg(long)]
        clamp_footprint: bool,
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
        Cmd::Locate {
            prot,
            offset,
            in_entry,
            cdname,
        } => locate_cmd(&prot, &offset, in_entry, cdname.as_deref()),
        Cmd::Extract {
            prot,
            out,
            cdname,
            clamp_footprint,
        } => extract(&prot, &out, cdname.as_deref(), clamp_footprint),
    }
}

/// Parse a `0x…` hex or decimal offset string.
fn parse_offset(s: &str) -> Result<u64> {
    let t = s.trim();
    let parsed = if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)
    } else {
        t.parse::<u64>()
    };
    parsed.with_context(|| format!("invalid offset {t:?} (use decimal or 0x-hex)"))
}

fn locate_cmd(
    prot: &Path,
    offset: &str,
    in_entry: Option<u32>,
    cdname_path: Option<&Path>,
) -> Result<()> {
    use legaia_prot::locate;

    let archive = open_archive(prot)?;
    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };
    let block = |idx: u32| -> String {
        names
            .as_ref()
            .and_then(|m| cdname::block_for(m, idx))
            .map(|b| format!(" {b}"))
            .unwrap_or_default()
    };

    let raw = parse_offset(offset)?;
    // Resolve the queried offset to an absolute PROT.DAT offset.
    let abs = match in_entry {
        Some(n) => locate::abs_from_entry_offset(&archive.entries, n, raw)
            .with_context(|| format!("no extraction entry with index {n} in {}", prot.display()))?,
        None => raw,
    };

    let loc = locate::locate(&archive.toc, &archive.entries, abs);

    println!(
        "query:      0x{raw:X}{}",
        match in_entry {
            Some(n) => format!("  (offset within entry {n}'s .BIN file)"),
            None => "  (absolute PROT.DAT offset)".to_string(),
        }
    );
    if in_entry.is_some() {
        println!("absolute:   0x{abs:X}  (PROT.DAT byte offset)");
    }
    println!();

    match loc.owner {
        Some(i) => {
            let e = &archive.entries[i];
            let footprint = locate::footprint_bytes(&archive.toc, e);
            let local = abs - e.byte_offset;
            println!(
                "true owner: entry {}{}  (start 0x{:08X}, footprint 0x{:X})",
                e.index,
                block(e.index),
                e.byte_offset,
                footprint,
            );
            println!("            offset 0x{local:X} into that entry's own data");
        }
        None => {
            println!("true owner: (none) - offset is past every entry's footprint (tail padding)");
        }
    }

    // If the offset was queried against a specific entry, say plainly when it
    // runs past that entry - a `.BIN` written before the entry size was
    // corrected carries a neighbour's bytes in its tail, and an offset read
    // out of one lands here.
    let queried_entry = in_entry.and_then(|n| {
        let e = archive.entries.iter().find(|e| e.index == n)?;
        Some((n, e))
    });
    if let Some((n, e)) = queried_entry {
        let footprint = locate::footprint_bytes(&archive.toc, e);
        if raw >= footprint {
            let owner_note = loc
                .owner
                .map(|i| {
                    let o = &archive.entries[i];
                    format!("entry {}{}", o.index, block(o.index))
                })
                .unwrap_or_else(|| "another entry".to_string());
            println!();
            println!(
                "NOTE: 0x{raw:X} is PAST entry {n}'s end (0x{footprint:X}) - a stale extraction \
                 window. Those bytes belong to {owner_note}, not entry {n}."
            );
        }
    }

    // Windows this crate produces don't overlap, so this only fires for a
    // caller-supplied entry set (or a stale one).
    if loc.covering.len() > 1 {
        println!();
        println!("also covered by these entries' windows:");
        for &i in &loc.covering {
            let e = &archive.entries[i];
            let tag = if Some(i) == loc.owner {
                "true source"
            } else {
                "over-read copy"
            };
            println!("  entry {}{}  ({tag})", e.index, block(e.index));
        }
    }

    Ok(())
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
        "{:>5}  {:>10}  {:>10}  {:>10}  {:>10}  {:>3}  block",
        "idx", "byte_off", "size", "decl_span", "lba", "ovr"
    );
    for e in &archive.entries {
        let block = names
            .as_ref()
            .and_then(|m| cdname::block_for(m, e.index))
            .unwrap_or("");
        // `ovr` marks the entries where the historical `toc[p+5] - toc[p+3] +
        // 4` window (`decl_span`) reaches past the entry - those are the ones
        // whose pre-correction `.BIN` carried a neighbour's bytes in its tail.
        let ovr = if e.declared_span_bytes > e.size_bytes {
            "OVR"
        } else {
            ""
        };
        println!(
            "{:>5}  0x{:08X}  {:>10}  {:>10}  {:>10}  {:>3}  {}",
            e.index, e.byte_offset, e.size_bytes, e.declared_span_bytes, e.start_lba, ovr, block
        );
    }
    Ok(())
}

fn extract(
    prot: &Path,
    out: &Path,
    cdname_path: Option<&Path>,
    clamp_footprint: bool,
) -> Result<()> {
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
            size: buf.len() as u64,
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
    if clamp_footprint {
        println!("note: --clamp-footprint is a no-op; every .BIN is the entry's own sectors");
    }
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
