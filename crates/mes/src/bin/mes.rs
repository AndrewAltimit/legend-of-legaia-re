use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use legaia_mes::{
    EventStats, Format, Interpreter, SubstituteOpcode, Token, extract_all_messages, iter_tokens,
    parse,
};

#[derive(Parser)]
#[command(
    name = "mes",
    version,
    about = "Legaia MES (asset type 0x04) inspector"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

/// Records-variant detection is a heuristic (>= 4 hits of the `0x44 0x78`
/// record marker anywhere in the blob). Big non-MES inputs (e.g. whole scene
/// bundles) can cross that bar by accident; flag the low-density case.
/// Real Records blobs put a marker every few hundred bytes - one marker per
/// > 4 KB is almost certainly a false positive.
const RECORDS_SPARSE_GAP: usize = 4096;

#[derive(Subcommand)]
enum Cmd {
    /// Detect format and print the structural header / table layout.
    ///
    /// Input: a MES dialog blob (asset type 0x04) pulled out of a scene
    /// bundle by `legaia-extract <disc.bin> --out extracted` (see
    /// extracted/streaming/). For readable dialog text, prefer
    /// `legaia-patcher translate export`.
    Info { path: PathBuf },
    /// Greedy bytecode disassembly. For Compact, starts at the bytecode
    /// offset; for Records, starts at byte 0 (record content is
    /// interleaved with markers).
    /// Input: a MES blob from `legaia-extract` (see extracted/streaming/).
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
    /// Input: a MES blob from `legaia-extract` (see extracted/streaming/).
    Json { path: PathBuf },
    /// Walk the bytecode interpreter for a single message and print events.
    /// Input: a Compact-variant MES blob from `legaia-extract`.
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
    /// Input: a Compact-variant MES blob from `legaia-extract`.
    StatsAll { path: PathBuf },
    /// Group a conversation branch into **dialog boxes** - the page-at-a-time
    /// view, rather than the segment-at-a-time one the runtime steps through.
    ///
    /// Retail's per-actor dialog SM (`FUN_80039B7C`) shows up to three `0x1F`
    /// lines per window and then reads the control byte that follows the last
    /// one to decide what the pager does next. This walks that grouping from a
    /// starting `0x1F` lead and prints one line per box: its rows and the
    /// dispatch that ends it. Useful when editing a MAN's dialog, where what
    /// matters is which lines share a window.
    Boxes {
        /// A blob containing `0x1F`-lead dialog segments - a MES blob from
        /// `legaia-extract` (see extracted/streaming/), or a decompressed MAN.
        path: PathBuf,
        /// Byte offset of the first `0x1F` lead. Defaults to the first one in
        /// the file.
        #[arg(long, value_parser = parse_hex_usize)]
        start: Option<usize>,
        /// Stop after this many boxes (also the runaway guard).
        #[arg(long, default_value_t = 32)]
        limit: usize,
        /// Walk every `0x1F` lead in the file, not just one branch. A raw
        /// scene entry is full of `0x1F` bytes that are not dialog leads, so
        /// this filters to boxes with a recognised dispatch byte and no empty
        /// row - pass `--unfiltered` to see every hit.
        #[arg(long)]
        all: bool,
        /// With `--all`, print every `0x1F` hit including the coincidental ones.
        #[arg(long)]
        unfiltered: bool,
    },
}

/// Rust ignores SIGPIPE by default; restore SIG_DFL so `mes disasm f | head`
/// exits quietly instead of panicking on a broken pipe.
fn reset_sigpipe() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

fn main() -> Result<()> {
    reset_sigpipe();
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
        Cmd::Boxes {
            path,
            start,
            limit,
            all,
            unfiltered,
        } => boxes(&path, start, limit, all, unfiltered),
    }
}

/// Group `0x1F`-lead dialog segments into boxes and print them.
fn boxes(
    path: &PathBuf,
    start: Option<usize>,
    limit: usize,
    all: bool,
    unfiltered: bool,
) -> Result<()> {
    let raw = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let leads: Vec<usize> = if all {
        raw.iter()
            .enumerate()
            .filter(|(_, b)| **b == 0x1F)
            .map(|(i, _)| i)
            .collect()
    } else {
        let at = match start {
            Some(s) => s,
            None => raw
                .iter()
                .position(|b| *b == 0x1F)
                .context("no 0x1F dialog lead in this file")?,
        };
        vec![at]
    };

    println!("file:  {}", path.display());
    println!("size:  {} bytes", raw.len());
    println!("rows per box: {}", legaia_mes::dialog_box::LINES_PER_BOX);

    let mut total = 0usize;
    let mut covered = 0usize;
    for lead in leads {
        let packed = legaia_mes::dialog_box::pack_boxes(&raw, lead, limit);
        if packed.is_empty() {
            continue;
        }
        if all && packed[0].lead != lead {
            continue;
        }
        // A raw scene entry carries plenty of `0x1F` bytes that are not dialog
        // leads. A real box has a dispatch byte the pager recognises and no
        // zero-length row; both together cut the coincidences.
        if all
            && !unfiltered
            && (matches!(packed[0].dispatch, legaia_mes::Dispatch::Unknown(_))
                || packed[0].lines.iter().any(|r| r.is_empty()))
        {
            continue;
        }
        for b in &packed {
            let rows: Vec<String> = b
                .lines
                .iter()
                .map(|r| format!("{:#x}..{:#x} ({}B)", r.start, r.end, r.len()))
                .collect();
            println!(
                "  box @{:#08x}  {} row(s) [{}]  dispatch {:?} @{:#x}",
                b.lead,
                b.lines.len(),
                rows.join(", "),
                b.dispatch,
                b.dispatch_at
            );
            covered += b.dispatch_at.saturating_sub(b.lead);
        }
        total += packed.len();
        if !all {
            break;
        }
    }
    println!("-- {total} box(es), {covered} bytes grouped");
    Ok(())
}

fn info(path: &PathBuf) -> Result<()> {
    let raw = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let blob = parse(&raw).with_context(|| format!("parse {}", path.display()))?;
    println!("file:    {}", path.display());
    println!("size:    {} bytes", blob.size);
    println!("format:  {}", blob.format.name());
    if blob.format == Format::Records {
        println!(
            "         (Records detection is heuristic: >= 4 hits of the 0x44 0x78 record marker)"
        );
        let n_markers = blob.records.as_ref().map(|r| r.len()).unwrap_or(0);
        if blob.size / n_markers.max(1) > RECORDS_SPARSE_GAP {
            println!(
                "warning: only {} marker hit(s) in {} bytes - input may not be a MES container",
                n_markers, blob.size
            );
        }
    }
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
