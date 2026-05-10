//! `mednafen-state` - CLI orchestrator for the mednafen-automation toolkit.
//!
//! Subcommands:
//!   info       Inspect a save state's section table.
//!   extract    Slice a PSX-virtual-address window out of a save state's main RAM.
//!   diff       Diff main RAM between two save states.
//!   bisect     Walk a sequence of save states looking for the first one in
//!              which a target address transitions from a "good" to "bad" value.
//!   trace      Trace a u32 across a sequence of save states.
//!   watch      Run all watchpoints for a scenario from `scenarios.toml`
//!              against its sister states.
//!   scenarios  List the scenarios known to the manifest.
//!
//! See `docs/tooling/mednafen-automation.md`.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use legaia_mednafen::{
    SaveState, ScenarioManifest, bisect,
    diff::{DiffOptions, diff_ram, sort_by_size},
    extract::{PSX_RAM_KSEG0, PSX_RAM_SIZE, ram_slice},
    psx::PsxMain,
};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "mednafen-state", version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Inspect a save state's section table and CPU registers.
    Info {
        save: PathBuf,
        /// Print every sub-entry in every section instead of just the
        /// MAIN section.
        #[arg(long)]
        all: bool,
    },
    /// Extract a PSX-virtual-address window out of a save state's main RAM.
    Extract {
        save: PathBuf,
        #[arg(long, value_parser = parse_addr, default_value = "0x801C0000")]
        start: u32,
        #[arg(long, value_parser = parse_addr, default_value = "0x80200000")]
        end: u32,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Diff main RAM between two save states.
    Diff {
        left: PathBuf,
        right: PathBuf,
        #[arg(long, value_parser = parse_addr)]
        start: Option<u32>,
        #[arg(long, value_parser = parse_addr)]
        end: Option<u32>,
        /// Write diff to JSON at this path (in addition to stdout summary).
        #[arg(long)]
        json: Option<PathBuf>,
        /// Drop regions smaller than this many changed bytes.
        #[arg(long, default_value_t = 4)]
        min_changed: usize,
        /// Coalesce regions whose gap is at most this many bytes.
        #[arg(long, default_value_t = 16)]
        merge_gap: usize,
        /// Top-N regions to show on stdout (sorted by bytes_changed).
        #[arg(long, default_value_t = 32)]
        top: usize,
    },
    /// Walk a sequence of save states; report the first one in which a
    /// target address has a non-zero value.
    Bisect {
        #[arg(long, value_parser = parse_addr)]
        addr: u32,
        /// Predicate: "nonzero" (default) or "zero".
        #[arg(long, default_value = "nonzero")]
        predicate: String,
        /// Save states to walk, in order.
        saves: Vec<PathBuf>,
    },
    /// Trace a u32 across a sequence of save states.
    Trace {
        #[arg(long, value_parser = parse_addr)]
        addr: u32,
        saves: Vec<PathBuf>,
    },
    /// Run all watchpoints for the given scenario against its diff_against
    /// sister states.
    Watch {
        /// Scenario label (looked up in scenarios.toml).
        label: String,
        /// Scenario manifest (default: scripts/mednafen/scenarios.toml).
        #[arg(long, default_value = "scripts/mednafen/scenarios.toml")]
        manifest: PathBuf,
        /// Optional output JSON path.
        #[arg(long)]
        json: Option<PathBuf>,
    },
    /// List the scenarios known to the manifest.
    Scenarios {
        #[arg(long, default_value = "scripts/mednafen/scenarios.toml")]
        manifest: PathBuf,
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
        Cmd::Info { save, all } => cmd_info(&save, all),
        Cmd::Extract {
            save,
            start,
            end,
            out,
        } => cmd_extract(&save, start, end, out.as_deref()),
        Cmd::Diff {
            left,
            right,
            start,
            end,
            json,
            min_changed,
            merge_gap,
            top,
        } => cmd_diff(
            &left,
            &right,
            DiffArgs {
                start,
                end,
                json: json.as_deref(),
                min_changed,
                merge_gap,
                top,
            },
        ),
        Cmd::Bisect {
            addr,
            predicate,
            saves,
        } => cmd_bisect(addr, &predicate, &saves),
        Cmd::Trace { addr, saves } => cmd_trace(addr, &saves),
        Cmd::Watch {
            label,
            manifest,
            json,
        } => cmd_watch(&label, &manifest, json.as_deref()),
        Cmd::Scenarios { manifest } => cmd_scenarios(&manifest),
    }
}

fn cmd_info(save: &Path, all: bool) -> Result<()> {
    let s = SaveState::from_path(save)?;
    println!("[info] {}", save.display());
    println!("[info] decompressed payload: {} bytes", s.payload.len());
    println!("[info] {} top-level sections", s.sections.len());
    for sec in &s.sections {
        println!(
            "  - {:<24}  body=0x{:X}..0x{:X} ({} bytes, {} entries)",
            sec.name,
            sec.body_offset,
            sec.body_offset + sec.body_len,
            sec.body_len,
            sec.entries.len()
        );
        if all || sec.name == "MAIN" {
            for e in &sec.entries {
                println!(
                    "      {:<24}  value=0x{:X}..0x{:X} ({} bytes)",
                    e.name,
                    e.value_offset,
                    e.value_offset + e.value_len,
                    e.value_len
                );
            }
        }
    }
    let regs = PsxMain::new(&s).cpu_regs();
    if let Some(pc) = regs.pc {
        println!("[info] CPU.PC = 0x{pc:08X}");
    }
    if let Ok(ram) = s.main_ram() {
        println!("[info] main RAM resolved: {} bytes", ram.len());
    }
    Ok(())
}

fn cmd_extract(save: &Path, start: u32, end: u32, out: Option<&Path>) -> Result<()> {
    if start < PSX_RAM_KSEG0 || end > PSX_RAM_KSEG0 + PSX_RAM_SIZE as u32 {
        bail!(
            "slice outside main RAM [0x{:08X}..0x{:08X})",
            PSX_RAM_KSEG0,
            PSX_RAM_KSEG0 + PSX_RAM_SIZE as u32
        );
    }
    let s = SaveState::from_path(save)?;
    let ram = s.main_ram()?;
    let slice = ram_slice(ram, start, end)?;
    let default_out = format!("/tmp/legaia_ram_{start:08X}_{end:08X}.bin");
    let path = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(&default_out));
    std::fs::write(&path, slice).with_context(|| format!("writing {}", path.display()))?;
    println!(
        "[ok] wrote {}: {} bytes (RAM 0x{start:08X}..0x{end:08X})",
        path.display(),
        slice.len()
    );

    // Quick MIPS-shape sanity check (mirrors the python script).
    let jr_ra: [u8; 4] = [0x08, 0x00, 0xE0, 0x03];
    let n_jr = slice.chunks_exact(4).filter(|w| *w == jr_ra).count();
    let n_sp = slice
        .chunks_exact(4)
        .filter(|w| w[1] == 0xFF && w[2] == 0xBD && w[3] == 0x27)
        .count();
    let nonzero = slice.iter().filter(|&&b| b != 0).count();
    println!(
        "[info] {} nonzero bytes ({:.1}%); {} `jr $ra`; {} SP prologues",
        nonzero,
        100.0 * nonzero as f64 / slice.len() as f64,
        n_jr,
        n_sp
    );
    Ok(())
}

struct DiffArgs<'a> {
    start: Option<u32>,
    end: Option<u32>,
    json: Option<&'a Path>,
    min_changed: usize,
    merge_gap: usize,
    top: usize,
}

fn cmd_diff(left: &Path, right: &Path, args: DiffArgs<'_>) -> Result<()> {
    let start = args.start;
    let end = args.end;
    let json = args.json;
    let min_changed = args.min_changed;
    let merge_gap = args.merge_gap;
    let top = args.top;
    let l = SaveState::from_path(left)?;
    let r = SaveState::from_path(right)?;
    let lram = l.main_ram()?;
    let rram = r.main_ram()?;
    let opts = DiffOptions {
        window: (
            start.unwrap_or(PSX_RAM_KSEG0),
            end.unwrap_or(PSX_RAM_KSEG0 + PSX_RAM_SIZE as u32),
        ),
        merge_gap,
        min_bytes_changed: min_changed,
    };
    let mut d = diff_ram(
        lram,
        rram,
        left.file_name().and_then(|s| s.to_str()).unwrap_or("left"),
        right
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("right"),
        &opts,
    );
    sort_by_size(&mut d);
    println!("[diff] {} <-> {}", d.left_label, d.right_label);
    println!(
        "[diff] window 0x{:08X}..0x{:08X}  merge_gap={}  min_changed={}",
        opts.window.0, opts.window.1, opts.merge_gap, opts.min_bytes_changed
    );
    println!(
        "[diff] {} regions, {} bytes changed total",
        d.regions.len(),
        d.total_bytes_changed
    );
    println!("[diff] top {top} by bytes_changed:");
    println!(
        "    {:>12}  {:>12}  {:>10}  left -> right (16 bytes)",
        "start", "end", "changed"
    );
    for r in d.regions.iter().take(top) {
        println!(
            "    0x{:08X}  0x{:08X}  {:>10}  {} -> {}",
            r.start_addr,
            r.end_addr,
            r.bytes_changed,
            hex_short(&r.left_sample),
            hex_short(&r.right_sample),
        );
    }
    if let Some(p) = json {
        let text = serde_json::to_string_pretty(&d)?;
        std::fs::write(p, text)?;
        println!("[ok] wrote diff JSON to {}", p.display());
    }
    Ok(())
}

fn hex_short(bytes: &[u8]) -> String {
    let mut out = String::new();
    for b in bytes.iter().take(16) {
        out.push_str(&format!("{b:02X}"));
    }
    out
}

fn cmd_bisect(addr: u32, predicate: &str, saves: &[PathBuf]) -> Result<()> {
    if saves.is_empty() {
        bail!("provide at least one save-state path");
    }
    let parsed: Vec<SaveState> = saves
        .iter()
        .map(SaveState::from_path)
        .collect::<Result<_>>()?;
    let labelled: Vec<(String, &[u8])> = parsed
        .iter()
        .zip(saves.iter())
        .map(|(s, p)| {
            let label = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_owned();
            let ram = s.main_ram().unwrap_or(&[]);
            (label, ram)
        })
        .collect();
    let pred: Box<dyn Fn(u32) -> bool> = match predicate {
        "nonzero" => Box::new(|v: u32| v != 0),
        "zero" => Box::new(|v: u32| v == 0),
        other => bail!("unknown predicate {other:?}; expected 'nonzero' or 'zero'"),
    };
    let snaps: Vec<(&str, &[u8])> = labelled.iter().map(|(l, r)| (l.as_str(), *r)).collect();
    let outcome = bisect::bisect_first_bad(&snaps, addr, pred);
    println!("[bisect] addr=0x{addr:08X} predicate={predicate}");
    for (label, ram) in &snaps {
        let bytes = ram_slice(ram, addr, addr + 4)?;
        let v = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        println!("    {label:<48}  0x{v:08X}");
    }
    println!("[bisect] outcome = {outcome:?}");
    Ok(())
}

fn cmd_trace(addr: u32, saves: &[PathBuf]) -> Result<()> {
    if saves.is_empty() {
        bail!("provide at least one save-state path");
    }
    println!("[trace] addr=0x{addr:08X}");
    for p in saves {
        let s = SaveState::from_path(p)?;
        let ram = s.main_ram()?;
        let bytes = ram_slice(ram, addr, addr + 4)?;
        let v = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        println!(
            "    {:<60}  0x{v:08X}",
            p.file_name().and_then(|s| s.to_str()).unwrap_or("?")
        );
    }
    Ok(())
}

fn cmd_watch(label: &str, manifest_path: &Path, json: Option<&Path>) -> Result<()> {
    let manifest = ScenarioManifest::from_path(manifest_path)?;
    let scenario = manifest
        .by_label(label)
        .with_context(|| format!("scenario {label:?} not in {}", manifest_path.display()))?;
    let primary_path = manifest.save_path(scenario.slot)?;
    let primary = SaveState::from_path(&primary_path)?;
    let primary_ram = primary.main_ram()?.to_vec();
    println!("[watch] scenario={} slot={}", scenario.label, scenario.slot);
    println!("    primary save: {}", primary_path.display());

    let mut all_results: Vec<serde_json::Value> = Vec::new();
    for &sister_slot in &scenario.diff_against {
        let sister_path = manifest.save_path(sister_slot)?;
        if !sister_path.exists() {
            println!(
                "    skip sister slot {sister_slot}: {} missing",
                sister_path.display()
            );
            continue;
        }
        let sister = SaveState::from_path(&sister_path)?;
        let sister_ram = sister.main_ram()?.to_vec();
        for wp in &scenario.watchpoints {
            let opts = DiffOptions {
                window: (wp.start, wp.end),
                merge_gap: 4,
                min_bytes_changed: 1,
            };
            let mut d = diff_ram(
                &primary_ram,
                &sister_ram,
                &scenario.label,
                &format!(
                    "slot{sister_slot}_{}",
                    manifest
                        .by_slot(sister_slot)
                        .map(|s| s.label.as_str())
                        .unwrap_or("?")
                ),
                &opts,
            );
            sort_by_size(&mut d);
            println!(
                "    [{}] vs slot {sister_slot}: {} regions ({} bytes); hint: {}",
                wp.label,
                d.regions.len(),
                d.total_bytes_changed,
                wp.hint
            );
            for r in d.regions.iter().take(8) {
                println!(
                    "        0x{:08X}..0x{:08X}  {:>6} bytes  {} -> {}",
                    r.start_addr,
                    r.end_addr,
                    r.bytes_changed,
                    hex_short(&r.left_sample),
                    hex_short(&r.right_sample),
                );
            }
            all_results.push(serde_json::json!({
                "watchpoint": wp.label,
                "sister_slot": sister_slot,
                "sister_label": manifest.by_slot(sister_slot).map(|s| s.label.clone()),
                "diff": d,
            }));
        }
    }
    if let Some(p) = json {
        std::fs::write(p, serde_json::to_string_pretty(&all_results)?)?;
        println!("[ok] wrote {}", p.display());
    }
    Ok(())
}

fn cmd_scenarios(manifest_path: &Path) -> Result<()> {
    let manifest = ScenarioManifest::from_path(manifest_path)?;
    println!("[scenarios] {}", manifest_path.display());
    for s in &manifest.scenarios {
        let topics = if s.topics.is_empty() {
            String::new()
        } else {
            format!(" [{}]", s.topics.join(", "))
        };
        println!(
            "  mc{}  {:<28}  {}{}",
            s.slot, s.label, s.description, topics
        );
        if !s.diff_against.is_empty() {
            print!("       diff_against = [");
            for (i, n) in s.diff_against.iter().enumerate() {
                if i > 0 {
                    print!(", ");
                }
                print!(
                    "mc{n} ({})",
                    manifest
                        .by_slot(*n)
                        .map(|x| x.label.as_str())
                        .unwrap_or("?")
                );
            }
            println!("]");
        }
        for wp in &s.watchpoints {
            println!(
                "       watch {:<24}  0x{:08X}..0x{:08X}  {}",
                wp.label, wp.start, wp.end, wp.hint
            );
        }
    }
    Ok(())
}
