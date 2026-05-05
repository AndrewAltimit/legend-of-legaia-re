//! `vab` CLI: scan a buffer for Sony VAB instrument banks; list / extract
//! sample bodies and per-VAG metadata. Decoder produces 22050 Hz mono WAV
//! by default (Sony VAGs are pitch-modulated at runtime; the source rate
//! is per-tone and not stored in the body).
//!
//! Subcommands:
//!   list     - find + describe every VAB in the input file
//!   extract  - dump VAG bodies + metadata.json under <out>/<vab_idx>/

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use legaia_vab::{VagSampleSpan, decode_vag, find_vabs, parse, write_wav};

#[derive(Parser)]
#[command(
    name = "vab",
    about = "Sony VAB parser + VAG sample extractor for Legaia"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Find + describe every VAB in `<file>`.
    List { file: PathBuf },
    /// Extract VAG sample bodies (and optionally decode to WAV).
    Extract {
        file: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Also decode each VAG to a 22050 Hz mono WAV for audition.
        #[arg(long)]
        wav: bool,
        /// Sample rate to assume when emitting WAV (default 22050).
        #[arg(long, default_value_t = 22050)]
        sample_rate: u32,
    },
}

fn main() -> Result<()> {
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
    for (i, &off) in hits.iter().enumerate() {
        let report = parse(&data, off)?;
        let h = &report.header;
        let total_samples_bytes: usize = report.vag_samples.iter().map(|s| s.size).sum();
        println!(
            "\n[{}] @ 0x{:08X}  v{}  vab_id={}  fsize={}  programs={}  tones={}  samples={}  master_vol={}  pan={}",
            i, off, h.version, h.vab_id, h.fsize, h.ps, h.ts, h.vs, h.mvol, h.pan
        );
        println!(
            "    sample bodies: {} bytes total ({:.1} KB)",
            total_samples_bytes,
            total_samples_bytes as f64 / 1024.0
        );
        for s in &report.vag_samples {
            println!(
                "      vag[{:3}]: 0x{:08X}  size={:>7} bytes",
                s.index, s.byte_offset, s.size
            );
        }
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
    std::fs::create_dir_all(out)?;
    let mut total_samples = 0usize;
    for (i, &off) in hits.iter().enumerate() {
        let report = parse(&data, off)?;
        let vab_dir = out.join(format!("vab{:02}_at_{:08X}", i, off));
        std::fs::create_dir_all(&vab_dir)?;

        // metadata.json — full header + programs + tones for downstream tooling.
        let meta_path = vab_dir.join("metadata.json");
        let json = serde_json::to_string_pretty(&report)?;
        std::fs::write(&meta_path, json)?;

        // Each VAG body as raw .vag (just the ADPCM stream — no Sony VAG-format
        // 48-byte header, since we don't know the source pitch / rate per body).
        for s in &report.vag_samples {
            let raw = &data[s.byte_offset..s.byte_offset + s.size];
            let raw_path = vab_dir.join(format!("{:03}.vag", s.index));
            std::fs::write(&raw_path, raw)?;
            if wav {
                match decode_vag(raw) {
                    Ok(pcm) => {
                        let wav_path = vab_dir.join(format!("{:03}.wav", s.index));
                        let mut fh = std::fs::File::create(&wav_path)?;
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
        "extract done: {} samples written to {}",
        total_samples,
        out.display()
    );
    let _ = (
        total_samples,
        VagSampleSpan {
            index: 0,
            byte_offset: 0,
            size: 0,
        },
    ); // type ref
    Ok(())
}
