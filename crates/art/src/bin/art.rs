//! `art` CLI - inspect art tables, decode raw RAM bytes, dry-run trigger logic.
//!
//! Subcommands:
//!
//! - `art tables`            - print per-character art-name + learned-art-slot tables.
//! - `art constants`         - print the full action-constant table.
//! - `art parse <PATH>`      - best-effort parse of a raw art record from a binary blob.
//! - `art miracle <character> <commands>` - try a Miracle Art trigger from a command string.
//! - `art super <character> <queue-bytes>`  - try a Super Art trigger from queue bytes.
//!
//! Examples:
//! ```text
//! art constants
//! art tables --character vahn
//! art miracle vahn rdlulurdl                  # → Vahn's Craze
//! art super vahn 1927 0F19 1F0E 1927          # → Tri-Somersault
//! ```

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use legaia_art::{
    ActionConstant, ActionQueue, Character, Command, MIRACLE_ARTS, MiracleMatcher, SUPER_ARTS,
    SuperMatcher, art_anim_max_slot, art_anim_name, art_name, learned_art_action,
    learned_art_max_slot,
};

#[derive(Parser, Debug)]
#[command(
    name = "art",
    version,
    about = "Inspect Legaia Tactical Arts tables and trigger Super/Miracle Arts."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Print the full ActionConstant table (0x00..=0x32).
    Constants,
    /// Print per-character art-name + learned-art-slot tables.
    Tables {
        #[arg(long, value_enum)]
        character: Option<CharArg>,
    },
    /// Best-effort parse of a raw art record from a binary blob.
    ///
    /// Input: a raw art-record blob, e.g. sliced out of PROT entry 0x05C4
    /// in `legaia-extract <disc.bin> --out extracted` output (see
    /// docs/formats/art-data.md).
    Parse {
        path: PathBuf,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// Try a Miracle Art trigger from a command string (case-insensitive
    /// LRDU letters).
    Miracle {
        #[arg(value_enum)]
        character: CharArg,
        commands: String,
    },
    /// Try a Super Art trigger from queue bytes (whitespace/colon-delimited
    /// hex).
    Super {
        #[arg(value_enum)]
        character: CharArg,
        bytes: Vec<String>,
    },
    /// List Super Arts for a character.
    SuperArts {
        #[arg(value_enum)]
        character: CharArg,
    },
    /// List Miracle Arts.
    MiracleArts,
    /// Decode the arts-name table (name + AP + command directions) from a
    /// `SCUS_942.54` image.
    ///
    /// Input: the game executable extracted by `legaia-extract <disc.bin>
    /// --out extracted` (or `disc-extract extract`). The default path is
    /// resolved against the current directory.
    ArtsTable {
        #[arg(long, default_value = "extracted/SCUS_942.54")]
        scus: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CharArg {
    Vahn,
    Noa,
    Gala,
}

impl From<CharArg> for Character {
    fn from(c: CharArg) -> Self {
        match c {
            CharArg::Vahn => Character::Vahn,
            CharArg::Noa => Character::Noa,
            CharArg::Gala => Character::Gala,
        }
    }
}

/// Rust ignores SIGPIPE by default; restore SIG_DFL so `art ... | head`
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
        Cmd::Constants => cmd_constants(),
        Cmd::Tables { character } => cmd_tables(character),
        Cmd::Parse { path, offset } => cmd_parse(&path, offset),
        Cmd::Miracle {
            character,
            commands,
        } => cmd_miracle(character.into(), &commands),
        Cmd::Super { character, bytes } => cmd_super(character.into(), &bytes),
        Cmd::SuperArts { character } => cmd_super_arts(character.into()),
        Cmd::MiracleArts => cmd_miracle_arts(),
        Cmd::ArtsTable { scus } => cmd_arts_table(&scus),
    }
}

fn cmd_arts_table(scus: &std::path::Path) -> Result<()> {
    let bytes =
        std::fs::read(scus).with_context(|| format!("read SCUS image {}", scus.display()))?;
    let entries = legaia_art::arts_table::parse_from_scus(&bytes)
        .context("not a PSX-EXE / arts table out of range")?;
    println!("char  idx  ap   command            name");
    for e in &entries {
        let cmd: String = e
            .commands
            .iter()
            .map(|c| match c {
                Command::Left => 'L',
                Command::Right => 'R',
                Command::Down => 'D',
                Command::Up => 'U',
            })
            .collect();
        let tag = if e.is_miracle { " [Miracle]" } else { "" };
        println!(
            "{:<5} {:>3}  {:>3}  {:<18} {}{}",
            e.character.name(),
            e.index,
            e.ap,
            cmd,
            e.name,
            tag
        );
    }
    println!("({} arts)", entries.len());
    Ok(())
}

fn cmd_constants() -> Result<()> {
    println!("byte  name");
    for action in ActionConstant::all() {
        println!("0x{:02X}  {:?}", action.as_byte(), action);
    }
    Ok(())
}

fn cmd_tables(character: Option<CharArg>) -> Result<()> {
    let chars: Vec<Character> = match character {
        Some(c) => vec![c.into()],
        None => Character::all().to_vec(),
    };
    for c in chars {
        println!("=== {} ===", c.name());
        println!("  Learned Art Constant slots:");
        for slot in 0..=learned_art_max_slot(c) {
            match learned_art_action(c, slot) {
                Some(action) => {
                    let name = art_name(c, action).unwrap_or("?");
                    println!(
                        "    slot 0x{:02X}  action 0x{:02X}  {}",
                        slot,
                        action.as_byte(),
                        name
                    );
                }
                None => {
                    println!("    slot 0x{:02X}  (hole)", slot);
                }
            }
        }
        println!("  Art Anim Data slots:");
        for anim in 0..=art_anim_max_slot(c) {
            match art_anim_name(c, anim) {
                Some(name) => {
                    println!("    anim 0x{:02X}  {}", anim, name);
                }
                None => {
                    println!("    anim 0x{:02X}  (hole)", anim);
                }
            }
        }
    }
    Ok(())
}

fn cmd_parse(path: &PathBuf, offset: usize) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if offset > bytes.len() {
        bail!("offset {offset} past end of file ({} bytes)", bytes.len());
    }
    let parsed = legaia_art::parse_record(&bytes[offset..])?;
    let json = serde_json::to_string_pretty(&parsed)?;
    println!("{json}");
    Ok(())
}

fn parse_command_string(s: &str) -> Result<Vec<Command>> {
    let mut out = Vec::new();
    for ch in s.chars() {
        let cmd = match ch.to_ascii_lowercase() {
            'l' => Command::Left,
            'r' => Command::Right,
            'd' => Command::Down,
            'u' => Command::Up,
            ' ' | '\t' => continue,
            _ => bail!("unknown command character: '{}'", ch),
        };
        out.push(cmd);
    }
    Ok(out)
}

fn cmd_miracle(character: Character, commands: &str) -> Result<()> {
    let cmds = parse_command_string(commands)?;
    let matcher = MiracleMatcher::with_default_table();
    let mut q = ActionQueue::new();
    if matcher.try_trigger(character, &cmds, &mut q) {
        let bytes: Vec<String> = q
            .actions()
            .iter()
            .map(|a| format!("0x{:02X}", a.as_byte()))
            .collect();
        println!(
            "TRIGGER  {} Miracle Art ({} actions): {}",
            character.name(),
            q.len(),
            bytes.join(" ")
        );
    } else {
        println!(
            "no Miracle Art match for {} commands {:?}",
            character.name(),
            cmds
        );
    }
    Ok(())
}

fn cmd_super(character: Character, byte_strs: &[String]) -> Result<()> {
    let mut q = ActionQueue::new();
    for s in byte_strs {
        for tok in s
            .split(|c: char| !c.is_ascii_hexdigit())
            .filter(|t| !t.is_empty())
        {
            let chunks: Vec<&str> = if tok.len() <= 2 {
                vec![tok]
            } else {
                tok.as_bytes()
                    .chunks(2)
                    .map(|c| std::str::from_utf8(c).unwrap())
                    .collect()
            };
            for chunk in chunks {
                let v = u8::from_str_radix(chunk, 16)?;
                let Some(action) = ActionConstant::from_byte(v) else {
                    bail!("byte 0x{v:02X} is not a valid ActionConstant");
                };
                q.push(action);
            }
        }
    }
    let matcher = SuperMatcher::with_default_table();
    if let Some(hit) = matcher.try_trigger_at_tail(character, &mut q) {
        let bytes: Vec<String> = q
            .actions()
            .iter()
            .map(|a| format!("0x{:02X}", a.as_byte()))
            .collect();
        println!(
            "TRIGGER  {} Super Art finisher 0x{:02X} (matched {} bytes, appended {})\n  result: {}",
            character.name(),
            hit.finisher,
            hit.matched_len,
            hit.appended_len,
            bytes.join(" ")
        );
    } else {
        println!("no Super Art match for {} queue", character.name());
    }
    Ok(())
}

fn cmd_super_arts(character: Character) -> Result<()> {
    println!("=== {} Super Arts ===", character.name());
    for entry in SUPER_ARTS.iter().filter(|s| s.character == character) {
        let find: Vec<String> = entry.find.iter().map(|b| format!("{:02X}", b)).collect();
        let replace: Vec<String> = entry.replace.iter().map(|b| format!("{:02X}", b)).collect();
        println!(
            "  0x{:02X}  {}\n     find:    {}\n     replace: {}",
            entry.finisher,
            entry.name,
            find.join(" "),
            replace.join(" ")
        );
    }
    Ok(())
}

fn cmd_miracle_arts() -> Result<()> {
    println!("=== Miracle Arts ===");
    for art in MIRACLE_ARTS {
        let cmds: String = art
            .commands
            .iter()
            .map(|c| match c {
                Command::Left => 'L',
                Command::Right => 'R',
                Command::Down => 'D',
                Command::Up => 'U',
            })
            .collect();
        let replace: Vec<String> = art
            .replacement
            .iter()
            .map(|a| format!("{:02X}", a.as_byte()))
            .collect();
        println!(
            "  {} ({})\n     command:     {}\n     replacement: {}",
            art.name,
            art.character.name(),
            cmds,
            replace.join(" ")
        );
    }
    Ok(())
}
