use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use legaia_mes::{Token, iter_tokens, parse};

#[derive(Parser)]
#[command(name = "mes", about = "Legaia MES (asset type 0x04) inspector")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Detect format and print the structural header / table layout.
    Info { path: PathBuf },
    /// Greedy bytecode disassembly. For [`Format::Compact`] starts at
    /// the bytecode offset; for `Records`, starts at byte 0 (record
    /// content is interleaved with markers).
    Disasm {
        path: PathBuf,
        /// Override the start offset for the bytecode walk.
        #[arg(long, value_parser = parse_hex_usize)]
        start: Option<usize>,
        /// Stop after this many tokens (0 = no limit).
        #[arg(long, default_value_t = 0)]
        limit: usize,
    },
    /// Emit a JSON dump of the parsed structure (for tooling).
    Json { path: PathBuf },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Info { path } => info(&path),
        Cmd::Disasm { path, start, limit } => disasm(&path, start, limit),
        Cmd::Json { path } => json(&path),
    }
}

fn info(path: &PathBuf) -> Result<()> {
    let raw = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let blob = parse(&raw).with_context(|| format!("parse {}", path.display()))?;
    println!("file:    {}", path.display());
    println!("size:    {} bytes", blob.size);
    println!("format:  {}", blob.format.name());
    if let Some(rh) = blob.runtime_header {
        println!("runtime header @ +0x28:");
        println!("  back_ptr      = 0x{:08X}", rh.back_ptr);
        println!("  forward_ptr   = 0x{:08X}", rh.forward_ptr);
        println!(
            "  expanded_size = 0x{:X} ({})",
            rh.expanded_size, rh.expanded_size
        );
        println!("  count         = {}", rh.count);
    }
    if let Some(table) = &blob.offset_table {
        println!("offset table: {} u24 entries", table.len());
        for (i, v) in table.iter().enumerate().take(16) {
            println!("  [{:>2}] 0x{:06X} ({})", i, v, v);
        }
        if table.len() > 16 {
            println!("  ... +{} more", table.len() - 16);
        }
    }
    if let Some(off) = blob.bytecode_offset {
        println!("bytecode region: starts at offset 0x{:X}", off);
    }
    if let Some(records) = &blob.records {
        println!("records: {} marker boundaries", records.len());
        let mut prev = 0usize;
        for (i, r) in records.iter().enumerate().take(8) {
            let gap = if i == 0 { r.offset } else { r.offset - prev };
            println!(
                "  [{:>2}] @0x{:04X}  (+{} bytes from prev)",
                i, r.offset, gap
            );
            prev = r.offset;
        }
        if records.len() > 8 {
            println!("  ... +{} more", records.len() - 8);
        }
    }
    Ok(())
}

fn disasm(path: &PathBuf, start: Option<usize>, limit: usize) -> Result<()> {
    let raw = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let blob = parse(&raw).with_context(|| format!("parse {}", path.display()))?;
    let start = start.or(blob.bytecode_offset).unwrap_or(0);
    println!(
        "# bytecode disasm of {} (format={}, start=0x{:X})",
        path.display(),
        blob.format.name(),
        start
    );
    for (count, (off, tok)) in iter_tokens(&raw, start).enumerate() {
        if limit > 0 && count >= limit {
            println!("# ... stopped at limit {}", limit);
            break;
        }
        let label = render_token(tok);
        println!("  {:>6X}: {}", off, label);
    }
    Ok(())
}

fn render_token(t: Token) -> String {
    match t {
        Token::End => "END".to_string(),
        Token::Glyph(g) => format!("GLYPH 0x{:02X}", g),
        Token::Op65(a) => format!("op65   0x{:02X}", a),
        Token::Op4c(a) => format!("op4C   0x{:02X}", a),
        Token::Op26 { arg } => {
            let note = if arg == 0xFFFE {
                "  ; possible page-break"
            } else {
                ""
            };
            format!("op26   0x{:04X}{}", arg, note)
        }
        Token::Unknown(b) => format!("?      0x{:02X}", b),
    }
}

fn json(path: &PathBuf) -> Result<()> {
    let raw = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let blob = parse(&raw).with_context(|| format!("parse {}", path.display()))?;
    let s = serde_json::to_string_pretty(&blob)?;
    println!("{}", s);
    Ok(())
}

fn parse_hex_usize(s: &str) -> std::result::Result<usize, String> {
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    usize::from_str_radix(s, 16).map_err(|e| e.to_string())
}
