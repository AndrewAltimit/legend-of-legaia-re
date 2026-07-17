//! `cheat-tool` - inspect / classify / diff cheat databases.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use legaia_cheats::{Category, Database, classify_address, parse_gs_text, parse_mednafen_cht};
use std::path::{Path, PathBuf};

/// The community-sourced Legaia NTSC-U databases from `data/cheats/` (no Sony
/// bytes), baked in so the tool works from a release archive with zero input
/// files.
const BUILTIN_GS_TEXT: &str = include_str!("../../../../data/cheats/legaia-ntsc-u.gs.txt");
const BUILTIN_CHT: &str = include_str!("../../../../data/cheats/legaia-ntsc-u.cht");

/// Top-level CLI.
#[derive(Parser, Debug)]
#[command(version, about = "Inspect Legend of Legaia cheat databases", long_about = None)]
struct Cli {
    /// Subcommand.
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Parse a cheat file and print the typed JSON.
    Parse {
        /// Path to the cheat file. Omit to use the built-in Legaia NTSC-U
        /// databases (GameShark dump + mednafen .cht, baked into the binary).
        path: Option<PathBuf>,
        /// Drop identical duplicate entries (the GameShark "Have 99
        /// Items" sprawl) before printing.
        #[arg(long)]
        dedupe: bool,
    },
    /// Print one line per cheat entry: `[CATEGORY] addr  description`.
    List {
        /// Path to the cheat file. Omit to use the built-in Legaia NTSC-U
        /// databases.
        path: Option<PathBuf>,
        /// Drop identical duplicate entries.
        #[arg(long)]
        dedupe: bool,
    },
    /// Group entries by [`Category`] and print a per-category roll-up.
    Classify {
        /// Path to the cheat file. Omit to use the built-in Legaia NTSC-U
        /// databases.
        path: Option<PathBuf>,
        /// Drop identical duplicate entries.
        #[arg(long)]
        dedupe: bool,
    },
    /// Print entries that exist in `a` but not `b`, then vice versa.
    Diff {
        /// First cheat file.
        a: PathBuf,
        /// Second cheat file.
        b: PathBuf,
    },
    /// Print only the addresses that fall inside a per-character record,
    /// grouped by character + offset. Useful for checking that the cheat
    /// database covers every named field in `docs/formats/save-record.md`.
    ExtractOffsets {
        /// Path to the cheat file. Omit to use the built-in Legaia NTSC-U
        /// databases.
        path: Option<PathBuf>,
    },
    /// Render a Markdown table of the per-character record offsets the
    /// database touches. Drops into `docs/reference/cheats.md`.
    OffsetTable {
        /// Path to the cheat file.
        path: PathBuf,
    },
}

/// Restore the default SIGPIPE disposition so piping into `head` etc.
/// terminates the process quietly instead of panicking on a broken pipe.
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
        Cmd::Parse { path, dedupe } => cmd_parse(path.as_deref(), dedupe),
        Cmd::List { path, dedupe } => cmd_list(path.as_deref(), dedupe),
        Cmd::Classify { path, dedupe } => cmd_classify(path.as_deref(), dedupe),
        Cmd::Diff { a, b } => cmd_diff(&a, &b),
        Cmd::ExtractOffsets { path } => cmd_extract_offsets(path.as_deref()),
        Cmd::OffsetTable { path } => cmd_offset_table(&path),
    }
}

fn load(path: &Path, dedupe: bool) -> Result<Database> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut db = if path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("cht"))
        .unwrap_or(false)
    {
        parse_mednafen_cht(&text)?
    } else {
        parse_gs_text(&text)?
    };
    if db.entries.is_empty() {
        eprintln!(
            "warning: 0 entries recognized in {} - expected a GameShark text dump \
             (.gs.txt) or mednafen .cht cheat file",
            path.display()
        );
    }
    if dedupe {
        db.dedupe_identical();
    }
    Ok(db)
}

/// The built-in Legaia NTSC-U corpus: the GameShark dump merged with the
/// mednafen `.cht` databases shipped in `data/cheats/`.
fn load_builtin(dedupe: bool) -> Result<Database> {
    eprintln!("using built-in Legaia NTSC-U cheat databases (pass a path to read a file)");
    let mut db = parse_gs_text(BUILTIN_GS_TEXT).context("parse built-in GameShark database")?;
    let cht = parse_mednafen_cht(BUILTIN_CHT).context("parse built-in mednafen database")?;
    db.entries.extend(cht.entries);
    if dedupe {
        db.dedupe_identical();
    }
    Ok(db)
}

/// Read the given cheat file, or fall back to the built-in databases.
fn load_or_builtin(path: Option<&Path>, dedupe: bool) -> Result<Database> {
    match path {
        Some(p) => load(p, dedupe),
        None => load_builtin(dedupe),
    }
}

fn cmd_parse(path: Option<&Path>, dedupe: bool) -> Result<()> {
    let db = load_or_builtin(path, dedupe)?;
    println!("{}", serde_json::to_string_pretty(&db)?);
    Ok(())
}

fn cmd_list(path: Option<&Path>, dedupe: bool) -> Result<()> {
    let db = load_or_builtin(path, dedupe)?;
    println!(
        "{} entries, {} unconditional writes",
        db.entries.len(),
        db.write_count()
    );
    for entry in &db.entries {
        let first_addr = entry.codes.first().map(|c| c.addr).unwrap_or(0);
        let cls = classify_address(first_addr);
        println!(
            "[{:?}] 0x{:08X}  {}",
            cls.category, first_addr, entry.description
        );
    }
    Ok(())
}

fn cmd_classify(path: Option<&Path>, dedupe: bool) -> Result<()> {
    let db = load_or_builtin(path, dedupe)?;
    let groups = db.classify();
    println!(
        "{} entries across {} categories",
        db.entries.len(),
        groups.len()
    );
    for (cat, entries) in &groups {
        println!("\n=== {cat:?} ({} entries) ===", entries.len());
        for entry in entries {
            let addrs: Vec<String> = entry
                .codes
                .iter()
                .filter(|c| c.is_write())
                .map(|c| format!("0x{:08X}", c.addr))
                .collect();
            println!("  {}  {}", addrs.join(","), entry.description);
        }
    }
    Ok(())
}

fn cmd_diff(a: &Path, b: &Path) -> Result<()> {
    let mut da = load(a, true)?;
    let mut db = load(b, true)?;
    let key =
        |e: &legaia_cheats::CheatEntry| -> Vec<u32> { e.codes.iter().map(|c| c.addr).collect() };
    let kb: std::collections::HashSet<Vec<u32>> = db.entries.iter().map(key).collect();
    let ka: std::collections::HashSet<Vec<u32>> = da.entries.iter().map(key).collect();
    da.entries.retain(|e| !kb.contains(&key(e)));
    db.entries.retain(|e| !ka.contains(&key(e)));
    println!("Only in {}:", a.display());
    for e in &da.entries {
        println!("  {}", e.description);
    }
    println!();
    println!("Only in {}:", b.display());
    for e in &db.entries {
        println!("  {}", e.description);
    }
    Ok(())
}

fn cmd_extract_offsets(path: Option<&Path>) -> Result<()> {
    let db = load_or_builtin(path, true)?;
    let mut by_char: std::collections::BTreeMap<&'static str, Vec<(u32, String)>> =
        std::collections::BTreeMap::new();
    for entry in &db.entries {
        for code in entry.writes() {
            let cls = classify_address(code.addr);
            if cls.category != Category::CharacterRecord {
                continue;
            }
            // detail = "vahn_record:hp_curr_live(+0x106)"
            let Some((who, field)) = cls.detail.split_once('_') else {
                continue;
            };
            // Drop the "_record:..." prefix from `who`.
            let who = match who {
                "vahn" => "vahn",
                "noa" => "noa",
                "gala" => "gala",
                "slot3" => "slot3",
                _ => continue,
            };
            let _ = field;
            // Compute the record-relative offset.
            let base = legaia_cheats::CHAR_RECORD_BASES
                .iter()
                .find_map(|(b, n)| if *n == who { Some(*b) } else { None })
                .unwrap();
            let off = code.addr - base;
            by_char
                .entry(who)
                .or_default()
                .push((off, entry.description.clone()));
        }
    }
    for (who, mut rows) in by_char {
        rows.sort_by_key(|(off, _)| *off);
        rows.dedup();
        println!("== {who} ({} unique offsets) ==", rows.len());
        for (off, desc) in &rows {
            println!("  +0x{off:03X}  {desc}");
        }
    }
    Ok(())
}

fn cmd_offset_table(path: &Path) -> Result<()> {
    let db = load(path, true)?;
    println!("| Offset | Width | Cheat label | Field name |");
    println!("|---:|---:|---|---|");
    let mut rows: std::collections::BTreeMap<u32, Vec<(u8, String, String)>> =
        std::collections::BTreeMap::new();
    for entry in &db.entries {
        for code in entry.writes() {
            let cls = classify_address(code.addr);
            if cls.category != Category::CharacterRecord {
                continue;
            }
            // Use Vahn's record as the canonical view.
            let Some(base) = legaia_cheats::CHAR_RECORD_BASES
                .iter()
                .find(|(b, _)| code.addr >= *b && code.addr < *b + 0x414)
                .map(|(b, _)| *b)
            else {
                continue;
            };
            let off = code.addr - base;
            rows.entry(off)
                .or_default()
                .push((code.width, entry.description.clone(), cls.detail));
        }
    }
    for (off, mut hits) in rows {
        hits.sort();
        hits.dedup();
        let (width, label, field) =
            hits.first()
                .cloned()
                .unwrap_or((0, String::new(), String::new()));
        // Strip the leading "<who>_record:" prefix from the field detail.
        let field = field.split_once(':').map(|(_, t)| t).unwrap_or(&field);
        println!("| +0x{off:03X} | u{} | {} | {} |", width * 8, label, field);
    }
    Ok(())
}
