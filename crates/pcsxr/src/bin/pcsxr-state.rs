//! pcsxr-state - CLI over the PCSX-Redux `.sstate` main-RAM reader.
//!
//! The `extract` subcommand mirrors `mednafen-state extract` (same flags,
//! same KSEG0 virtual-address semantics), so a state-reading script can
//! dispatch on file extension and treat the two emulators' states
//! interchangeably - `scripts/mednafen/check-0968-residency.py` is the
//! first consumer. `info` prints the scene / game-mode / player anchors
//! the library exposes.
//!
//! Disc-gated like the library: the RAM anchor search reads
//! `extracted/SCUS_942.54` (or `$LEGAIA_SCUS`).

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use legaia_pcsxr::SaveState;

#[derive(Parser)]
#[command(name = "pcsxr-state", about = "PCSX-Redux .sstate main-RAM reader")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print the scene name, game mode and player position of a state.
    Info { save: PathBuf },
    /// Print scene / game-mode / player position for one or more states.
    ///
    /// Mirrors `mednafen-state identify` so a sweep script can dispatch on file
    /// extension and index both emulators' corpora as one table. Unreadable
    /// states report as an `!` row rather than aborting the sweep.
    Identify {
        saves: Vec<PathBuf>,
        /// Emit one JSON object per state instead of the aligned table.
        #[arg(long)]
        json: bool,
    },
    /// Extract a PSX-virtual-address window out of a state's main RAM.
    Extract {
        save: PathBuf,
        #[arg(long, value_parser = parse_addr, default_value = "0x801C0000")]
        start: u32,
        #[arg(long, value_parser = parse_addr, default_value = "0x80200000")]
        end: u32,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn parse_addr(s: &str) -> Result<u32, String> {
    let s = s.trim();
    let parsed = if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(rest, 16)
    } else {
        s.parse::<u32>()
    };
    parsed.map_err(|e| format!("bad address '{s}': {e}"))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Info { save } => {
            let st = SaveState::from_path(&save)?;
            println!("scene     {}", st.scene_name());
            println!("game mode {:#04x}", st.game_mode());
            match st.player_pos() {
                Some((x, z)) => println!("player    x={x} z={z}"),
                None => println!("player    (no actor)"),
            }
            Ok(())
        }
        Cmd::Identify { saves, json } => {
            anyhow::ensure!(!saves.is_empty(), "no save states given");
            for path in &saves {
                match SaveState::from_path(path) {
                    Ok(st) => {
                        let id = st.identity();
                        let label = legaia_mednafen::game_anchors::game_mode_label(id.game_mode);
                        if json {
                            println!(
                                "{}",
                                serde_json::json!({
                                    "file": path.display().to_string(),
                                    "emulator": "pcsx-redux",
                                    "scene": id.scene,
                                    "game_mode": id.game_mode,
                                    "mode_label": label,
                                    "player": id.player.map(|(x, z)| [x, z]),
                                })
                            );
                        } else {
                            let pos = match id.player {
                                Some((x, z)) => format!("x={x} z={z}"),
                                None => "-".to_string(),
                            };
                            println!(
                                "{:<10} {:<14} {:<16} {}",
                                id.scene,
                                label,
                                pos,
                                path.display()
                            );
                        }
                    }
                    Err(e) => {
                        if json {
                            println!(
                                "{}",
                                serde_json::json!({
                                    "file": path.display().to_string(),
                                    "emulator": "pcsx-redux",
                                    "error": e.to_string(),
                                })
                            );
                        } else {
                            println!(
                                "{:<10} {:<14} {:<16} {}",
                                "!",
                                "unreadable",
                                "-",
                                path.display()
                            );
                        }
                    }
                }
            }
            Ok(())
        }
        Cmd::Extract {
            save,
            start,
            end,
            out,
        } => {
            anyhow::ensure!(start < end, "start {start:#x} must be below end {end:#x}");
            anyhow::ensure!(
                (0x8000_0000..=0x8020_0000).contains(&start) && end <= 0x8020_0000,
                "window {start:#x}..{end:#x} must sit in KSEG0 main RAM \
                 (0x80000000..0x80200000)"
            );
            let st = SaveState::from_path(&save)?;
            let ram = st.main_ram();
            let bytes = &ram[(start - 0x8000_0000) as usize..(end - 0x8000_0000) as usize];
            match out {
                Some(path) => std::fs::write(&path, bytes)?,
                None => {
                    use std::io::Write;
                    std::io::stdout().write_all(bytes)?;
                }
            }
            Ok(())
        }
    }
}
