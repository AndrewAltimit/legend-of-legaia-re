//! `vab` CLI: scan a buffer for Sony VAB instrument banks; list / extract
//! sample bodies and per-VAG metadata. Decoder produces 22050 Hz mono WAV
//! by default (Sony VAGs are pitch-modulated at runtime; the source rate
//! is per-tone and not stored in the body).
//!
//! Subcommands:
//!   list     - find + describe every VAB in the input file
//!   extract  - dump VAG bodies + metadata.json under `<out>/<vab_idx>/`

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use legaia_vab::{decode_vag, find_vabs, parse, write_wav};

#[derive(Parser)]
#[command(
    name = "vab",
    version,
    about = "Sony VAB parser + VAG sample extractor for Legaia"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Find + describe every VAB in `<file>`.
    ///
    /// Input: any PROT entry carrying VAB data from `legaia-extract
    /// <disc.bin> --out extracted`, e.g. extracted/PROT/0990_music_01.BIN.
    /// Wrapped BGM entries may end in a truncated trailing bank; those are
    /// reported as warnings and skipped.
    List { file: PathBuf },
    /// Extract VAG sample bodies (and optionally decode to WAV).
    ///
    /// Input: a PROT entry carrying VAB data from `legaia-extract`
    /// (e.g. extracted/PROT/0990_music_01.BIN). Truncated trailing banks
    /// (common in wrapped BGM entries) are skipped with a warning.
    Extract {
        file: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
        /// Also decode each VAG to a 22050 Hz mono WAV for audition.
        #[arg(long)]
        wav: bool,
        /// Sample rate to assume when emitting WAV (default 22050).
        #[arg(long, default_value_t = 22050)]
        sample_rate: u32,
    },
}

/// Rust ignores SIGPIPE by default; restore SIG_DFL so `vab ... | head`
/// exits quietly instead of panicking on a broken pipe.
fn reset_sigpipe() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

fn main() -> Result<()> {
    reset_sigpipe();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::List { file } => list(&file),
        Cmd::Extract {
            file,
            out,
            wav,
            sample_rate,
        } => extract(&file, &out, wav, sample_rate),
    }
}

fn list(file: &PathBuf) -> Result<()> {
    let data = std::fs::read(file).with_context(|| format!("read {}", file.display()))?;
    let hits = find_vabs(&data);
    if hits.is_empty() {
        eprintln!("no VAB headers found in {}", file.display());
        return Ok(());
    }
    println!(
        "found {} VAB(s) in {} ({} bytes)",
        hits.len(),
        file.display(),
        data.len()
    );
    let mut skipped = 0usize;
    for (i, &off) in hits.iter().enumerate() {
        // A truncated / overrunning bank (common as the trailing bank of
        // wrapped BGM entries, whose header claims an fsize past the file
        // end) must not abort the whole scan - warn and keep going.
        let report = match parse(&data, off) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[warn] VAB[{}] @ 0x{:08X}: {} - skipped", i, off, e);
                skipped += 1;
                continue;
            }
        };
        let h = &report.header;
        let total_samples_bytes: usize = report.vag_samples.iter().map(|s| s.size).sum();
        println!(
            "\n[{}] @ 0x{:08X}  v{}  vab_id={}  fsize={}  programs={}  tones={}  samples={}  master_vol={}  pan={}",
            i, off, h.version, h.vab_id, h.fsize, h.ps, h.ts, h.vs, h.mvol, h.pan
        );
        println!(
            "    sample bodies: {} bytes total ({:.1} KB)  vag_table[0]={}",
            total_samples_bytes,
            total_samples_bytes as f64 / 1024.0,
            report.vag_table_spacer,
        );
        for s in &report.vag_samples {
            println!(
                "      vag[{:3}]: 0x{:08X}  size={:>7} bytes",
                s.index, s.byte_offset, s.size
            );
        }
    }
    if skipped > 0 {
        eprintln!(
            "list done: {} of {} VAB header(s) skipped (truncated/implausible)",
            skipped,
            hits.len()
        );
    }
    Ok(())
}

fn extract(file: &PathBuf, out: &PathBuf, wav: bool, sample_rate: u32) -> Result<()> {
    let data = std::fs::read(file).with_context(|| format!("read {}", file.display()))?;
    let hits = find_vabs(&data);
    if hits.is_empty() {
        eprintln!("no VAB headers found in {}", file.display());
        return Ok(());
    }
    std::fs::create_dir_all(out).with_context(|| format!("create out dir {}", out.display()))?;
    let mut total_samples = 0usize;
    let mut skipped = 0usize;
    for (i, &off) in hits.iter().enumerate() {
        // Same tolerance as `list`: a truncated trailing bank (wrapped BGM
        // entries claim an fsize past the buffer end) is warned about and
        // skipped so every valid bank before it still extracts, exit 0.
        let report = match parse(&data, off) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[warn] VAB[{}] @ 0x{:08X}: {} - skipped", i, off, e);
                skipped += 1;
                continue;
            }
        };
        let vab_dir = out.join(format!("vab{:02}_at_{:08X}", i, off));
        std::fs::create_dir_all(&vab_dir)
            .with_context(|| format!("create {}", vab_dir.display()))?;

        // metadata.json - full header + programs + tones for downstream tooling.
        let meta_path = vab_dir.join("metadata.json");
        let json = serde_json::to_string_pretty(&report)?;
        std::fs::write(&meta_path, json)
            .with_context(|| format!("write {}", meta_path.display()))?;

        // Each VAG body as raw .vag (just the ADPCM stream - no Sony VAG-format
        // 48-byte header, since we don't know the source pitch / rate per body).
        for s in &report.vag_samples {
            let raw = &data[s.byte_offset..s.byte_offset + s.size];
            let raw_path = vab_dir.join(format!("{:03}.vag", s.index));
            std::fs::write(&raw_path, raw)
                .with_context(|| format!("write {}", raw_path.display()))?;
            if wav {
                match decode_vag(raw) {
                    Ok(pcm) => {
                        let wav_path = vab_dir.join(format!("{:03}.wav", s.index));
                        let mut fh = std::fs::File::create(&wav_path)
                            .with_context(|| format!("create {}", wav_path.display()))?;
                        write_wav(&mut fh, &pcm, sample_rate)?;
                    }
                    Err(e) => eprintln!("[warn] VAG {} (vab {}): decode failed: {}", s.index, i, e),
                }
            }
            total_samples += 1;
        }

        println!(
            "vab[{}] @ 0x{:08X}  -> {} samples, {} bytes",
            i,
            off,
            report.vag_samples.len(),
            report.vag_samples.iter().map(|s| s.size).sum::<usize>()
        );
    }
    eprintln!(
        "extract done: {} samples written to {}{}",
        total_samples,
        out.display(),
        if skipped > 0 {
            format!(" ({skipped} truncated/implausible VAB header(s) skipped)")
        } else {
            String::new()
        }
    );
    Ok(())
}
