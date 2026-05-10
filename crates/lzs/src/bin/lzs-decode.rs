use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "lzs-decode", about = "Legaia LZS decompressor")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Decompress a raw LZS stream of known output size.
    Raw {
        input: PathBuf,
        /// Expected decompressed output size in bytes.
        #[arg(long)]
        size: usize,
        /// Skip N bytes from the start of input before treating it as an LZS stream.
        #[arg(long, default_value_t = 0)]
        skip: usize,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Parse an `.lzs` container and decompress every section.
    Container {
        input: PathBuf,
        /// Output directory; one file per section.
        out: PathBuf,
    },
    /// Try to interpret a file as an LZS container and report.
    Probe { input: PathBuf },
    /// Probe every file in a directory and list the ones that look like
    /// valid LZS containers.
    Scan { dir: PathBuf },
    /// Decode every LZS container in a directory and group results by
    /// `(total_decoded_size, first_24_bytes_of_decoded_payload)` for
    /// cluster identification. Verification at scale.
    Audit {
        dir: PathBuf,
        /// Optional path: write a one-line summary per file as TSV
        /// (`name<TAB>sections<TAB>decoded_total<TAB>head_hex<TAB>md5_decoded_total`).
        #[arg(long)]
        tsv: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Raw {
            input,
            size,
            skip,
            output,
        } => raw(&input, size, skip, output.as_ref()),
        Cmd::Container { input, out } => container(&input, &out),
        Cmd::Probe { input } => probe(&input),
        Cmd::Scan { dir } => scan(&dir),
        Cmd::Audit { dir, tsv } => audit(&dir, tsv.as_ref()),
    }
}

fn raw(input: &PathBuf, size: usize, skip: usize, out: Option<&PathBuf>) -> Result<()> {
    let raw = std::fs::read(input)?;
    if skip > raw.len() {
        bail!("--skip {} larger than input ({}b)", skip, raw.len());
    }
    let body = &raw[skip..];
    let decoded = legaia_lzs::decompress(body, size)?;
    eprintln!(
        "[ok] body={}b output={}b ratio={:.2}x",
        body.len(),
        decoded.len(),
        decoded.len() as f64 / body.len() as f64
    );
    match out {
        Some(p) => std::fs::write(p, &decoded)?,
        None => print_preview(&decoded),
    }
    Ok(())
}

fn container(input: &PathBuf, out_dir: &PathBuf) -> Result<()> {
    let raw = std::fs::read(input)?;
    let c = legaia_lzs::parse_container(&raw)?;
    eprintln!(
        "[container] meta=[0x{:08X}, 0x{:08X}]  sections={}",
        c.header_meta[0],
        c.header_meta[1],
        c.sections.len()
    );
    std::fs::create_dir_all(out_dir)?;
    for (i, sec) in c.sections.iter().enumerate() {
        let body = &raw[sec.byte_offset as usize..];
        let decoded = legaia_lzs::decompress(body, sec.size as usize)?;
        let stem = input
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let out_path = out_dir.join(format!("{}.s{:02}.bin", stem, i));
        std::fs::write(&out_path, &decoded)?;
        eprintln!(
            "  s{:02}  off=0x{:06X}  size={:>8}  -> {}",
            i,
            sec.byte_offset,
            decoded.len(),
            out_path.display()
        );
    }
    Ok(())
}

fn probe(input: &PathBuf) -> Result<()> {
    let raw = std::fs::read(input)?;
    match legaia_lzs::parse_container(&raw) {
        Ok(c) => {
            eprintln!(
                "[container ok] meta=[0x{:08X}, 0x{:08X}]  sections={}",
                c.header_meta[0],
                c.header_meta[1],
                c.sections.len()
            );
            for (i, s) in c.sections.iter().enumerate() {
                eprintln!(
                    "  s{:02}  off=0x{:06X}  decoded_size={}",
                    i, s.byte_offset, s.size
                );
            }
            // Try decompressing each - only success if every section decodes
            for (i, sec) in c.sections.iter().enumerate() {
                let body = &raw[sec.byte_offset as usize..];
                match legaia_lzs::decompress(body, sec.size as usize) {
                    Ok(_) => eprintln!("  s{:02} decompressed cleanly", i),
                    Err(e) => eprintln!("  s{:02} FAILED: {}", i, e),
                }
            }
        }
        Err(e) => eprintln!("[no container] {}", e),
    }
    Ok(())
}

/// Lower bound on the total decoded payload of a "real" LZS container.
/// Empirically determined: real PROT containers decode to ≥ 6 KB; the loose
/// header heuristic plus our greedy decoder emits sub-100-byte garbage on
/// non-LZS files (TIM-packs whose `(size, off)` u32 pairs happen to satisfy
/// the container check). 256 B is a comfortable margin.
const MIN_REAL_DECODE_BYTES: usize = 256;

fn scan(dir: &PathBuf) -> Result<()> {
    let mut hits = 0;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let raw = match std::fs::read(&path) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let decoded = match legaia_lzs::decompress_container_strict(&raw) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let total_out: usize = decoded.iter().map(|d| d.len()).sum();
        if total_out < MIN_REAL_DECODE_BYTES {
            continue;
        }
        hits += 1;
        println!(
            "{}  sections={}  decompressed={}b",
            path.file_name().unwrap_or_default().to_string_lossy(),
            decoded.len(),
            total_out
        );
    }
    eprintln!("scan done: {} files validated as real LZS containers", hits);
    Ok(())
}

/// Per-file row in the audit TSV: `(name, sections, ratio)`.
type FileRow = (String, usize, f64);
/// Cluster key: `(total_decoded_bytes, head_hex)`.
type ClusterKey = (usize, String);

fn audit(dir: &PathBuf, tsv: Option<&PathBuf>) -> Result<()> {
    use std::collections::BTreeMap;
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());

    let mut by_cluster: BTreeMap<ClusterKey, Vec<FileRow>> = BTreeMap::new();
    let mut total_files = 0usize;
    let mut strict_hits = 0usize;
    let mut lenient_only_hits: Vec<String> = Vec::new();
    let mut tsv_lines: Vec<String> = Vec::new();

    for entry in entries {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        total_files += 1;
        let raw = match std::fs::read(&path) {
            Ok(r) => r,
            Err(_) => continue,
        };
        // First try strict; if that rejects, see if lenient parses anything
        // non-trivial - that bucket is "real LZS-shaped data with non-standard
        // section layout" (e.g., music_01).
        let strict = legaia_lzs::decompress_container_strict(&raw);
        let lenient_total = legaia_lzs::decompress_container(&raw)
            .map(|v| v.iter().map(|d| d.len()).sum::<usize>())
            .ok();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();

        let decoded = match strict {
            Ok(d) => d,
            Err(_) => {
                if let Some(lenient) = lenient_total
                    && lenient >= MIN_REAL_DECODE_BYTES
                {
                    lenient_only_hits.push(format!("{} ({} B lenient)", name, lenient));
                }
                continue;
            }
        };
        let total: usize = decoded.iter().map(|d| d.len()).sum();
        if total < MIN_REAL_DECODE_BYTES {
            continue;
        }
        let section_count = decoded.len();
        strict_hits += 1;
        let head: String = decoded
            .iter()
            .flatten()
            .take(24)
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(" ");
        let ratio = total as f64 / raw.len().max(1) as f64;
        by_cluster.entry((total, head.clone())).or_default().push((
            name.clone(),
            section_count,
            ratio,
        ));
        if tsv.is_some() {
            tsv_lines.push(format!(
                "{}\t{}\t{}\t{}\t{:.3}",
                name, section_count, total, head, ratio
            ));
        }
    }

    println!(
        "scanned {} files, {} strict-validated as LZS containers (≥{} B decoded, no overrun)",
        total_files, strict_hits, MIN_REAL_DECODE_BYTES
    );
    if !lenient_only_hits.is_empty() {
        println!(
            "\n{} lenient-only hits (parse_container succeeds but sections overrun):",
            lenient_only_hits.len()
        );
        for line in &lenient_only_hits {
            println!("  {}", line);
        }
    }
    println!();
    let mut by_size: BTreeMap<usize, Vec<(ClusterKey, Vec<FileRow>)>> = BTreeMap::new();
    for (k, v) in by_cluster.into_iter() {
        by_size.entry(k.0).or_default().push((k, v));
    }
    for (size, clusters) in &by_size {
        let total_in_size: usize = clusters.iter().map(|(_, v)| v.len()).sum();
        println!("=== decoded_size = {}b ({} files) ===", size, total_in_size);
        for ((_, head), files) in clusters {
            println!("  head: {}    [{} files]", head, files.len());
            for (n, s, r) in files.iter().take(3) {
                println!("    {} (sections={}, ratio={:.2}x)", n, s, r);
            }
            if files.len() > 3 {
                println!("    ... {} more", files.len() - 3);
            }
        }
        println!();
    }

    if let Some(tsv_path) = tsv {
        std::fs::write(tsv_path, tsv_lines.join("\n"))?;
        eprintln!("wrote {} lines to {}", tsv_lines.len(), tsv_path.display());
    }
    Ok(())
}

fn print_preview(out: &[u8]) {
    let preview: String = out
        .iter()
        .take(64)
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ");
    println!("{}", preview);
}
