//! `art` CLI — inspect art tables, decode raw RAM bytes, dry-run trigger logic.
//!
//! Subcommands:
//!
//! - `art tables`            — print per-character art-name + learned-art-slot tables.
//! - `art constants`         — print the full action-constant table.
//! - `art parse <PATH>`      — best-effort parse of a raw art record from a binary blob.
//! - `art miracle <character> <commands>` — try a Miracle Art trigger from a command string.
//! - `art super <character> <queue-bytes>`  — try a Super Art trigger from queue bytes.
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

use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use legaia_art::{
    ActionConstant, ActionQueue, Character, Command, MIRACLE_ARTS, MiracleMatcher, SUPER_ARTS,
    SuperMatcher, art_name, learned_art_action,
};

#[derive(Parser, Debug)]
#[command(
    name = "art",
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

fn main() -> Result<()> {
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
    }
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
        for slot in 0..50u8 {
            let Some(action) = learned_art_action(c, slot) else {
                break;
            };
            let name = art_name(c, action).unwrap_or("?");
            println!(
                "  slot {:>2}  action 0x{:02X}  {}",
                slot,
                action.as_byte(),
                name
            );
        }
    }
    Ok(())
}

fn cmd_parse(path: &PathBuf, offset: usize) -> Result<()> {
    let bytes = fs::read(path)?;
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
