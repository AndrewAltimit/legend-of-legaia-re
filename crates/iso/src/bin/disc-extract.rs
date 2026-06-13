use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use legaia_iso::iso9660;
use legaia_iso::raw::{RawDisc, USER_DATA_SIZE};
use legaia_iso::region;
use sha2::{Digest, Sha256};

/// Known-good SHA-256 hashes for supported disc images.
///
/// The .bin hashes here are full-image SHA-256 for Mode2/2352 dumps. Different
/// dumping tools / settings may produce different hashes for the same disc;
/// canonical per-track verification belongs to the Redump project. These
/// hashes are recorded against the project author's dump as a sanity check.
const KNOWN_HASHES: &[(&str, &str)] = &[(
    "Legend of Legaia (USA) - SCUS-94254 - Mode2/2352 .bin",
    "e6120a5d70716dd2f026a2da32d0171d52651971b52c4347a68541299f75258c",
)];

#[derive(Parser)]
#[command(
    name = "disc-extract",
    about = "Read a PSX Mode2/2352 .bin and walk ISO9660"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List every file on the disc with its size and LBA.
    List { bin: PathBuf },
    /// Extract every file on the disc to `<out>`.
    Extract { bin: PathBuf, out: PathBuf },
    /// Compute the .bin SHA-256 and compare against known good hashes.
    ///
    /// Pass --expected to compare against a specific hex; otherwise we look
    /// up against KNOWN_HASHES (currently NA). The volume label and sector
    /// count are also reported so users can sanity-check their dump.
    Verify {
        bin: PathBuf,
        /// Expected SHA-256 in hex (case-insensitive). If absent, compare
        /// against the built-in known list.
        #[arg(long)]
        expected: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::List { bin } => list(&bin),
        Cmd::Extract { bin, out } => extract(&bin, &out),
        Cmd::Verify { bin, expected } => verify(&bin, expected.as_deref()),
    }
}

fn list(bin: &Path) -> Result<()> {
    let mut disc = RawDisc::open(bin)?;
    let vol = iso9660::read_volume(&mut disc)?;
    println!("volume: {:?}", vol.volume_id);
    println!("sectors: {}", disc.sector_count());
    println!();
    println!("{:>10}  {:>7}  path", "size", "lba");
    for (path, entry) in iso9660::walk_files(&mut disc, &vol.root)? {
        println!("{:>10}  {:>7}  {}", entry.size, entry.lba, path);
    }
    Ok(())
}

fn verify(bin: &Path, expected: Option<&str>) -> Result<()> {
    let hash = sha256_file(bin).with_context(|| format!("hashing {}", bin.display()))?;
    println!("file:    {}", bin.display());
    println!("sha256:  {}", hash);

    // Also peek at the volume label, sector count, and region for context.
    match RawDisc::open(bin) {
        Ok(mut disc) => {
            println!("sectors: {}", disc.sector_count());
            if let Ok(vol) = iso9660::read_volume(&mut disc) {
                println!("volume:  {:?}", vol.volume_id);
            }
            // Find SYSTEM.CNF and detect region from it.
            if let Ok(vol) = iso9660::read_volume(&mut disc)
                && let Ok(files) = iso9660::walk_files(&mut disc, &vol.root)
                && let Some((_, entry)) = files
                    .iter()
                    .find(|(p, _)| p.eq_ignore_ascii_case("SYSTEM.CNF"))
            {
                let mut buf = Vec::new();
                let n = entry.size.div_ceil(USER_DATA_SIZE as u32);
                if disc.read_user_data(entry.lba, n, &mut buf).is_ok() {
                    buf.truncate(entry.size as usize);
                    match region::parse(&buf) {
                        Ok(d) => println!(
                            "region:  {} (executable={}, prefix={})",
                            d.region.name(),
                            d.executable,
                            d.prefix
                        ),
                        Err(e) => println!("region:  [parse failed: {}]", e),
                    }
                }
            }
        }
        Err(e) => eprintln!("[warn] could not parse as Mode2/2352 disc: {}", e),
    }

    println!();
    if let Some(want) = expected {
        let want = want.trim().to_lowercase();
        if want == hash {
            println!("[ok] hash matches --expected");
            Ok(())
        } else {
            println!("[mismatch] expected: {}", want);
            println!("[mismatch] actual:   {}", hash);
            bail!("SHA-256 mismatch")
        }
    } else {
        let mut matched = None;
        for (label, h) in KNOWN_HASHES {
            if h.eq_ignore_ascii_case(&hash) {
                matched = Some(*label);
                break;
            }
        }
        match matched {
            Some(label) => {
                println!("[ok] matches: {}", label);
                Ok(())
            }
            None => {
                println!("[unknown] hash does not match any KNOWN_HASHES entry.");
                println!("          This is fine if your dump tool/settings differ; check");
                println!("          per-track SHA-1s against Redump for canonical verification.");
                Ok(())
            }
        }
    }
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn extract(bin: &Path, out: &Path) -> Result<()> {
    let mut disc = RawDisc::open(bin)?;
    let vol = iso9660::read_volume(&mut disc)?;
    let files = iso9660::walk_files(&mut disc, &vol.root)?;
    std::fs::create_dir_all(out)?;
    let mut buf = Vec::new();
    for (path, entry) in &files {
        let full = out.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let sector_count = entry.size.div_ceil(USER_DATA_SIZE as u32);
        disc.read_user_data(entry.lba, sector_count, &mut buf)?;
        buf.truncate(entry.size as usize);
        std::fs::write(&full, &buf)?;
        println!("{:>10}  {}", entry.size, path);
    }
    println!();
    println!("extracted {} files into {}", files.len(), out.display());
    Ok(())
}
