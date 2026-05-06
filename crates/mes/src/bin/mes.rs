use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use legaia_mes::{
    EventStats, Interpreter, SubstituteOpcode, Token, extract_all_messages, iter_tokens, parse,
};

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
    /// Greedy bytecode disassembly. For Compact, starts at the bytecode
    /// offset; for Records, starts at byte 0 (record content is
    /// interleaved with markers).
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
    /// Walk the bytecode interpreter for a single message and print events.
    Events {
        path: PathBuf,
        #[arg(long, default_value_t = 0)]
        index: usize,
        /// Print as one event per line (`Glyph(0x9D)`), else use the
        /// compact `render_summary` form.
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },
    /// Walk every offset-table entry, print event-stats for each message.
    StatsAll { path: PathBuf },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Info { path } => info(&path),
        Cmd::Disasm { path, start, limit } => disasm(&path, start, limit),
        Cmd::Json { path } => json(&path),
        Cmd::Events {
            path,
            index,
            verbose,
        } => events(&path, index, verbose),
        Cmd::StatsAll { path } => stats_all(&path),
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
        Token::EndOfMessage(b) => format!("END    0x{:02X}", b),
        Token::Glyph(g) => format!("GLYPH  0x{:02X}", g),
        Token::WideGlyph(op, arg) => format!("WIDE   0x{:02X} 0x{:02X}", op, arg),
        Token::Substitute { kind, arg } => {
            let tag = match kind {
                SubstituteOpcode::CharacterName => "char_name",
                SubstituteOpcode::ItemName => "item_name",
                SubstituteOpcode::MagicName => "magic_name",
                SubstituteOpcode::ItemNameAlt => "item_name(alt)",
                SubstituteOpcode::SpellName => "spell_name",
                SubstituteOpcode::QuestName => "quest_name",
            };
            format!("SUBST  {}  arg=0x{:02X}", tag, arg)
        }
        Token::Spacing(n) => format!("SPACE  0x{:02X}", n),
        Token::SkipTwo(n) => format!("SKIP   0x{:02X}", n),
        Token::Control(b) => format!("CTRL   0x{:02X}  ; page-break / wait", b),
        Token::Truncated(op) => format!("TRUNC  0x{:02X}", op),
    }
}

fn json(path: &PathBuf) -> Result<()> {
    let raw = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let blob = parse(&raw).with_context(|| format!("parse {}", path.display()))?;
    let s = serde_json::to_string_pretty(&blob)?;
    println!("{}", s);
    Ok(())
}

fn events(path: &PathBuf, index: usize, verbose: bool) -> Result<()> {
    let raw = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let blob = parse(&raw).with_context(|| format!("parse {}", path.display()))?;
    let mut interp = Interpreter::new_compact(&blob, &raw, index)?;
    let events = interp.collect_events();
    println!(
        "# message {} from {} ({} events)",
        index,
        path.display(),
        events.len()
    );
    if verbose {
        for ev in &events {
            println!("  {ev:?}");
        }
    } else {
        println!("{}", Interpreter::render_summary(&events));
    }
    Ok(())
}

fn stats_all(path: &PathBuf) -> Result<()> {
    let raw = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let messages = extract_all_messages(&raw)
        .with_context(|| format!("extract messages from {}", path.display()))?;
    println!(
        "# {} messages from {} ({} bytes)",
        messages.len(),
        path.display(),
        raw.len()
    );
    let mut totals = EventStats::default();
    for (i, evs) in messages.iter().enumerate() {
        let s = EventStats::from_events(evs);
        totals.glyphs += s.glyphs;
        totals.wide_glyphs += s.wide_glyphs;
        totals.substitutes += s.substitutes;
        totals.spacing += s.spacing;
        totals.skip_two += s.skip_two;
        totals.controls += s.controls;
        totals.truncated += s.truncated;
        totals.end_of_message += s.end_of_message;
        if i < 16 {
            println!(
                "  [{:>3}] {} glyphs, {} wide, {} subst, {} ctrl, {} trunc, {} ev total",
                i,
                s.glyphs,
                s.wide_glyphs,
                s.substitutes,
                s.controls,
                s.truncated,
                evs.len(),
            );
        }
    }
    if messages.len() > 16 {
        println!("  ... +{} more messages", messages.len() - 16);
    }
    println!(
        "totals: {} glyphs, {} wide, {} subst, {} space, {} skip, {} ctrl, {} trunc",
        totals.glyphs,
        totals.wide_glyphs,
        totals.substitutes,
        totals.spacing,
        totals.skip_two,
        totals.controls,
        totals.truncated,
    );
    Ok(())
}

fn parse_hex_usize(s: &str) -> std::result::Result<usize, String> {
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    usize::from_str_radix(s, 16).map_err(|e| e.to_string())
}
