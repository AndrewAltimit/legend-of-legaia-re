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
//!   vram-dump  Extract the 1 MiB GPU VRAM and write it as a 1024x512 PNG
//!              (+ optional .bin) so engine-side runtime-VRAM comparisons
//!              have a ground-truth reference.
//!   scenarios  List the scenarios known to the manifest.
//!   world-map-camera
//!              Decode the world-map top-view camera-state RAM globals
//!              (negated map-origin scrolls, azimuth, zoom/mode) from one
//!              or more save states.
//!
//! See `docs/tooling/mednafen-automation.md`.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use legaia_mednafen::{
    PsxGpu, SaveState, ScenarioManifest, VRAM_HEIGHT, VRAM_WIDTH, bisect,
    diff::{DiffOptions, diff_ram, sort_by_size},
    extract::{PSX_RAM_KSEG0, PSX_RAM_SIZE, ram_slice},
    gpu::{nonzero_rows, vram_to_rgba8},
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
    /// Diff two save states and roll the changed bytes up into a per-region
    /// write taxonomy (via `legaia_cheats::classify_address`): how many bytes
    /// changed in each subsystem (inventory / character record / battle actor
    /// / story flags / script-VM scratch / ...), flagging writes that land
    /// outside every known data region. The classification half of a
    /// gameplay-driven write tracer.
    WriteTaxonomy {
        left: PathBuf,
        right: PathBuf,
        #[arg(long, value_parser = parse_addr)]
        start: Option<u32>,
        #[arg(long, value_parser = parse_addr)]
        end: Option<u32>,
        /// Sample classifications to print per region bucket.
        #[arg(long, default_value_t = 8)]
        samples: usize,
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
        /// Scenario manifest (default: scripts/scenarios.toml).
        #[arg(long, default_value = "scripts/scenarios.toml")]
        manifest: PathBuf,
        /// Optional output JSON path.
        #[arg(long)]
        json: Option<PathBuf>,
    },
    /// Extract the 1 MiB GPU VRAM from a save state and write it as a
    /// 1024x512 RGBA8 PNG (BGR555 + STP-as-alpha). Optionally also writes
    /// the raw byte blob to `--out-bin`. Mirrors the encoding used by
    /// `tmd vram-dump` so engine-side comparisons line up.
    VramDump {
        save: PathBuf,
        /// Output PNG path.
        #[arg(long, default_value = "vram.png")]
        out: PathBuf,
        /// Optional raw 1 MiB BGR555 little-endian dump.
        #[arg(long)]
        out_bin: Option<PathBuf>,
        /// Print the GPU control-register snapshot (clip rect, draw
        /// offset, texture window, texture page) alongside the dump.
        #[arg(long)]
        regs: bool,
    },
    /// List the scenarios known to the manifest.
    Scenarios {
        #[arg(long, default_value = "scripts/scenarios.toml")]
        manifest: PathBuf,
    },
    /// Byte-match decoded `battle_data` pack records against a save state's
    /// VRAM to pin the post-TMD CLUT-pool descriptor at record `u32[3..0x20]`.
    /// For every decoded record, slide a 32-byte halfword-aligned window past
    /// the embedded TMD body and search VRAM for an exact match; print one
    /// row per `(record, VRAM coord)` hit so the descriptor encoding can be
    /// reverse-engineered from the corpus. Optional `--json` writes the same
    /// data structured.
    ClutTrace {
        /// PROT entry to decode as a battle_data pack (e.g. 0865_battle_data.BIN).
        #[arg(long)]
        pack: PathBuf,
        /// One or more mednafen save states whose VRAM should be matched
        /// against the decoded records.
        saves: Vec<PathBuf>,
        /// Write the full corpus to a JSON file alongside the stdout summary.
        #[arg(long)]
        json: Option<PathBuf>,
        /// Skip CLUT-shaped windows that land inside the embedded TMD body
        /// (default; matches the analysis methodology). When set, also scan
        /// the TMD body for windows that happen to look CLUT-shaped - useful
        /// for double-checking the TMD-end heuristic.
        #[arg(long, default_value_t = false)]
        include_tmd_body: bool,
    },
    /// Walk the live PSX GPU prim pool out of a save state's main RAM,
    /// cluster POLY_FT4 packets into unique tile signatures
    /// (`clut + tpage + sorted UVs`), and search a RAM-window for byte
    /// sequences matching those signatures. Reports stride statistics for
    /// each window so a per-tile source-data table emerges as a
    /// fixed-stride match cluster. The 139 KB region
    /// `0x801913F5..0x801B5FD0` and the 91 KB region
    /// `0x8016E44C..0x80184BD0` are the prime suspects for the
    /// world-map continent terrain's per-tile descriptor table - both
    /// scanned by default.
    PrimTrace {
        save: PathBuf,
        /// Pool start address (kuseg). Default = `0x800AD400`
        /// (`prim_pool::POOL_BASE_DEFAULT`).
        #[arg(long, value_parser = parse_addr, default_value = "0x800AD400")]
        pool_base: u32,
        /// Pool end address (exclusive). Default = `0x80102000`.
        #[arg(long, value_parser = parse_addr, default_value = "0x80102000")]
        pool_end: u32,
        /// Additional `<start>:<end>` RAM windows to search for tile
        /// fingerprints. May be supplied multiple times. The two
        /// default candidate regions (`0x80190000:0x801B6000` for the
        /// 139 KB region, `0x8016E000:0x80185000` for the 91 KB region)
        /// are always scanned in addition to whatever the user adds.
        #[arg(long, value_parser = parse_window)]
        window: Vec<(u32, u32)>,
        /// How many top tile signatures to report (sorted by hit count).
        #[arg(long, default_value_t = 16)]
        top: usize,
        /// Scan the FULL 2 MiB main RAM for cluster-#0's fingerprint(s),
        /// reporting every hit's address. Use this to discover where the
        /// source data actually lives when the default windows come up
        /// empty.
        #[arg(long, default_value_t = false)]
        scan_all_ram: bool,
        /// Write the full analysis as JSON to this path (stdout still
        /// prints a summary).
        #[arg(long)]
        json: Option<PathBuf>,
    },
    /// Decode the per-primitive renderer dispatch tables consumed by
    /// `FUN_80043390`: the SCUS-resident table at `0x8007657C` and the
    /// world-map-overlay variant at `0x801F8968`. Reports every populated
    /// slot's target address, classifies it (SCUS / overlay / other), and
    /// surfaces the eight overlay-resident high-mode prim renderers - the
    /// per-prim emit leaves the world-map top-view routes its TMD prims
    /// through. The overlay table reports as all-zero when the world-map
    /// overlay isn't paged into the save state.
    PrimDispatchTable {
        save: PathBuf,
        /// Print only the unique high-mode target addresses from the
        /// overlay table (suitable for piping to per-function dump tools).
        #[arg(long)]
        overlay_targets_only: bool,
    },
    /// Survey the dispatch tables across multiple saves in one pass.
    /// Prints a side-by-side comparison table showing which saves have
    /// the world-map overlay paged in (overlay dispatch populated) vs.
    /// not, and asserts the SCUS-resident table is byte-identical across
    /// every save (it lives in code so RAM writes can't legally touch
    /// it). Useful for spot-checking after the user adds a new save
    /// capture.
    PrimDispatchSurvey {
        /// Two or more save states to survey. Order is preserved in the
        /// output.
        saves: Vec<PathBuf>,
    },
    /// Decode the world-map top-view camera-state globals
    /// (`_DAT_80089120`, `_DAT_80089118`, `_DAT_8007B794`,
    /// `_DAT_8007B6F4`) from one or more save states.
    ///
    /// The X/Z scrolls are stored as negated map-origin coordinates
    /// (`-(int)*(short *)(actor + 0x14)` in overlay_0978 /
    /// overlay_slot_machine), so the printed `cam_x` / `cam_z` are the
    /// negations - the camera target in world units. The view-mode
    /// flag (`DAT_801F2B94`) is also printed: `0` = walk-view (D-pad
    /// does not pump the camera globals), `1` = top-view debug mode
    /// (D-pad actively scrolls / rotates / zooms).
    ///
    /// Use this to seed per-kingdom defaults in the world-overview
    /// viewer once a save capture in top-view mode exists.
    WorldMapCamera {
        /// One or more save states. Order preserved in the output.
        saves: Vec<PathBuf>,
        /// Print a tabular summary instead of one block per save.
        #[arg(long)]
        table: bool,
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

fn parse_window(s: &str) -> Result<(u32, u32), String> {
    let (a, b) = s
        .split_once(':')
        .ok_or_else(|| format!("expected `<start>:<end>`, got '{s}'"))?;
    let lo = parse_addr(a)?;
    let hi = parse_addr(b)?;
    if hi <= lo {
        return Err(format!("window end <= start in '{s}'"));
    }
    Ok((lo, hi))
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
        Cmd::WriteTaxonomy {
            left,
            right,
            start,
            end,
            samples,
        } => cmd_write_taxonomy(&left, &right, start, end, samples),
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
        Cmd::VramDump {
            save,
            out,
            out_bin,
            regs,
        } => cmd_vram_dump(&save, &out, out_bin.as_deref(), regs),
        Cmd::Scenarios { manifest } => cmd_scenarios(&manifest),
        Cmd::ClutTrace {
            pack,
            saves,
            json,
            include_tmd_body,
        } => cmd_clut_trace(&pack, &saves, json.as_deref(), include_tmd_body),
        Cmd::PrimTrace {
            save,
            pool_base,
            pool_end,
            window,
            top,
            scan_all_ram,
            json,
        } => cmd_prim_trace(
            &save,
            pool_base,
            pool_end,
            &window,
            top,
            scan_all_ram,
            json.as_deref(),
        ),
        Cmd::PrimDispatchTable {
            save,
            overlay_targets_only,
        } => cmd_prim_dispatch_table(&save, overlay_targets_only),
        Cmd::PrimDispatchSurvey { saves } => cmd_prim_dispatch_survey(&saves),
        Cmd::WorldMapCamera { saves, table } => cmd_world_map_camera(&saves, table),
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

fn cmd_write_taxonomy(
    left: &Path,
    right: &Path,
    start: Option<u32>,
    end: Option<u32>,
    samples: usize,
) -> Result<()> {
    let l = SaveState::from_path(left)?;
    let r = SaveState::from_path(right)?;
    let lram = l.main_ram()?;
    let rram = r.main_ram()?;
    let lo = start.unwrap_or(PSX_RAM_KSEG0).saturating_sub(PSX_RAM_KSEG0) as usize;
    let hi = end
        .unwrap_or(PSX_RAM_KSEG0 + PSX_RAM_SIZE as u32)
        .saturating_sub(PSX_RAM_KSEG0)
        .min(PSX_RAM_SIZE as u32) as usize;

    // Exact per-byte changed addresses (no region merging) so each byte is
    // classified into its own subsystem.
    let changed = (lo..hi)
        .filter(|&i| lram[i] != rram[i])
        .map(|i| PSX_RAM_KSEG0 + i as u32);
    let tax = legaia_cheats::classify_writes_with_samples(changed, samples);

    println!(
        "[taxonomy] {} <-> {}",
        left.file_name().and_then(|s| s.to_str()).unwrap_or("left"),
        right
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("right"),
    );
    println!("[taxonomy] {} changed bytes in main RAM", tax.total);
    if let Some(d) = tax.dominant() {
        println!(
            "[taxonomy] dominant region: {:?} ({} bytes)",
            d.category, d.count
        );
    }
    println!("[taxonomy] per-region:");
    for b in &tax.buckets {
        println!("  {:<18?} {:>8} bytes", b.category, b.count);
        for s in &b.samples {
            println!("        0x{:08X}  {}", s.addr, s.detail);
        }
    }
    let interesting: Vec<_> = tax.interesting().collect();
    if interesting.is_empty() {
        println!("[taxonomy] no writes landed outside known data regions.");
    } else {
        println!("[taxonomy] !! writes outside known data regions (attack-surface candidates):");
        for b in interesting {
            println!("  {:<18?} {:>8} bytes", b.category, b.count);
        }
    }
    Ok(())
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

fn cmd_vram_dump(save: &Path, out: &Path, out_bin: Option<&Path>, regs: bool) -> Result<()> {
    let s = SaveState::from_path(save)?;
    let gpu = PsxGpu::new(&s);
    let bytes = gpu
        .vram_bytes()
        .ok_or_else(|| anyhow::anyhow!("save state has no GPU.&GPURAM[0][0] entry"))?;
    let rgba = vram_to_rgba8(bytes);
    write_png(out, &rgba, VRAM_WIDTH as u32, VRAM_HEIGHT as u32)
        .with_context(|| format!("writing PNG to {}", out.display()))?;
    println!(
        "[ok] wrote {} ({}x{} BGR555 + STP-as-alpha, {} non-zero rows of {})",
        out.display(),
        VRAM_WIDTH,
        VRAM_HEIGHT,
        nonzero_rows(bytes),
        VRAM_HEIGHT,
    );
    if let Some(bin) = out_bin {
        std::fs::write(bin, bytes)
            .with_context(|| format!("writing raw VRAM to {}", bin.display()))?;
        println!(
            "[ok] wrote raw VRAM to {} ({} bytes)",
            bin.display(),
            bytes.len()
        );
    }
    if regs {
        let r = gpu.regs();
        println!("[regs] clip            = {:?}", r.clip);
        println!("[regs] draw_offset     = {:?}", r.draw_offset);
        println!(
            "[regs] tex_window      = {:?}  (mask_x, mask_y, off_x, off_y)",
            r.tex_window
        );
        println!("[regs] tex_page (x,y)  = {:?}", r.tex_page);
        println!("[regs] tex_mode        = {:?}", r.tex_mode);
        println!("[regs] display_fb      = {:?}", r.display_fb);
        println!("[regs] display_h_range = {:?}", r.display_h_range);
        println!("[regs] display_v_range = {:?}", r.display_v_range);
        println!("[regs] display_off     = {:?}", r.display_off);
        println!("[regs] display_mode_raw= {:?}", r.display_mode_raw);
    }
    Ok(())
}

fn cmd_clut_trace(
    pack_path: &Path,
    saves: &[PathBuf],
    json_out: Option<&Path>,
    include_tmd_body: bool,
) -> Result<()> {
    use legaia_asset::battle_data_pack;

    let pack_bytes = std::fs::read(pack_path)
        .with_context(|| format!("reading PROT entry {}", pack_path.display()))?;
    let pack = battle_data_pack::parse(&pack_bytes)
        .with_context(|| format!("parsing {} as battle_data pack", pack_path.display()))?;

    println!(
        "[pack] {}  records={}  data_base=0x{:x}",
        pack_path.display(),
        pack.records.len(),
        pack.data_base
    );

    // Decode every record once.
    struct DecodedRecord {
        record_idx: usize,
        record_id: u32,
        decoded: battle_data_pack::DecodedEntry,
    }
    let mut decoded_records = Vec::new();
    for r in &pack.records {
        match battle_data_pack::decode_record(&pack_bytes, &pack, r.index) {
            Ok(d) => decoded_records.push(DecodedRecord {
                record_idx: r.index,
                record_id: r.id,
                decoded: d,
            }),
            Err(e) => {
                eprintln!("[warn] record {} decode failed: {}", r.index, e);
            }
        }
    }

    #[derive(serde::Serialize)]
    struct CorpusEntry {
        save_state: String,
        record_idx: usize,
        record_id: u32,
        header_u32s: [String; 8],
        record_byte_offset: usize,
        tmd_end: Option<usize>,
        post_tmd_offset: Option<usize>,
        fb_x: u16,
        fb_y: u16,
        vram_byte_offset: usize,
    }
    let mut corpus = Vec::new();
    let mut total_hits = 0usize;

    println!(
        "{:<32} {:>5} {:>5} {:>10} {:>7} {:>7} {:>7}  header[0..8]",
        "save_state", "rec", "id", "rec_off", "post_off", "fb_x", "fb_y"
    );
    println!("{}", "-".repeat(120));

    for save in saves {
        let s = SaveState::from_path(save)
            .with_context(|| format!("loading save state {}", save.display()))?;
        let gpu = PsxGpu::new(&s);
        let Some(vram) = gpu.vram_bytes() else {
            eprintln!("[skip] {} has no GPU.&GPURAM[0][0]", save.display());
            continue;
        };
        let save_label = save
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();

        for rec in &decoded_records {
            let matches = if include_tmd_body {
                // Construct a fake DecodedEntry that lies about tmd_range so
                // find_clut_in_vram scans the whole record.
                let phony = battle_data_pack::DecodedEntry {
                    record: rec.decoded.record,
                    bytes: rec.decoded.bytes.clone(),
                    tmd_range: None,
                };
                battle_data_pack::find_clut_in_vram(&phony, vram)
            } else {
                battle_data_pack::find_clut_in_vram(&rec.decoded, vram)
            };
            let header = battle_data_pack::record_header_u32s(&rec.decoded);
            let tmd_end = rec.decoded.tmd_range.as_ref().map(|r| r.end);
            for m in &matches {
                total_hits += 1;
                let post_tmd_offset = tmd_end.map(|end| m.record_byte_offset.saturating_sub(end));
                println!(
                    "{:<32} {:>5} 0x{:02x} 0x{:08x} {:>7} {:>7} {:>7}  {}",
                    save_label,
                    rec.record_idx,
                    rec.record_id,
                    m.record_byte_offset,
                    post_tmd_offset
                        .map(|p| format!("0x{:x}", p))
                        .unwrap_or_else(|| "-".into()),
                    m.fb_x,
                    m.fb_y,
                    header
                        .iter()
                        .map(|w| format!("{:08x}", w))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
                corpus.push(CorpusEntry {
                    save_state: save_label.clone(),
                    record_idx: rec.record_idx,
                    record_id: rec.record_id,
                    header_u32s: header.map(|w| format!("0x{:08x}", w)),
                    record_byte_offset: m.record_byte_offset,
                    tmd_end,
                    post_tmd_offset,
                    fb_x: m.fb_x,
                    fb_y: m.fb_y,
                    vram_byte_offset: m.vram_byte_offset(),
                });
            }
        }
    }
    println!();
    println!(
        "[done] {} match(es) across {} save state(s) and {} record(s)",
        total_hits,
        saves.len(),
        decoded_records.len()
    );

    if let Some(path) = json_out {
        let json = serde_json::to_string_pretty(&corpus)
            .with_context(|| "encoding corpus JSON".to_string())?;
        std::fs::write(path, json)
            .with_context(|| format!("writing JSON to {}", path.display()))?;
        println!("[ok] wrote corpus to {}", path.display());
    }
    Ok(())
}

fn write_png(out: &Path, rgba: &[u8], w: u32, h: u32) -> Result<()> {
    let f = std::fs::File::create(out)?;
    let bw = std::io::BufWriter::new(f);
    let mut enc = png::Encoder::new(bw, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(rgba)?;
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

/// Default RAM windows to scan for tile-signature matches. Both come from
/// the documented mc1↔mc2 diff regions that lack any known format marker
/// and are sized in the same ballpark as ~10k POLY_FT4 records.
const DEFAULT_WINDOWS: &[(u32, u32)] = &[
    (0x80190000, 0x801B6000), // 144 KB, contains 0x801913F5..0x801B5FD0
    (0x8016E000, 0x80185000), // 92 KB,  contains 0x8016E44C..0x80184BD0
];

fn cmd_prim_trace(
    save_path: &Path,
    pool_base: u32,
    pool_end: u32,
    extra_windows: &[(u32, u32)],
    top: usize,
    scan_all_ram: bool,
    json_out: Option<&Path>,
) -> Result<()> {
    use legaia_mednafen::{prim_pool, source_hunt};
    use serde::Serialize;

    if pool_end <= pool_base {
        bail!("pool_end <= pool_base");
    }
    let save = SaveState::from_path(save_path)?;
    let ram = save.main_ram()?;
    let pool_lo = (pool_base - PSX_RAM_KSEG0) as usize;
    let pool_hi = (pool_end - PSX_RAM_KSEG0) as usize;
    if pool_hi > ram.len() {
        bail!("pool end past end of main RAM");
    }
    let pool = &ram[pool_lo..pool_hi];

    // 1) Walk pool + topology check.
    let topology = prim_pool::chain_topology(pool, pool_base);
    let prims = prim_pool::decode(pool, pool_base);
    let mut counts: std::collections::HashMap<&'static str, usize> =
        std::collections::HashMap::new();
    for p in &prims {
        let key: &'static str = match p {
            prim_pool::Prim::PolyFt4 { .. } => "POLY_FT4",
            prim_pool::Prim::PolyGt4 { .. } => "POLY_GT4",
            prim_pool::Prim::PolyFt3 { .. } => "POLY_FT3",
            prim_pool::Prim::PolyGt3 { .. } => "POLY_GT3",
            prim_pool::Prim::Sprt16 { .. } => "SPRT_16",
            prim_pool::Prim::Sprt8 { .. } => "SPRT_8",
        };
        *counts.entry(key).or_insert(0) += 1;
    }

    println!("[prim-trace] save  {}", save_path.display());
    println!(
        "[prim-trace] pool  0x{pool_base:08X}..0x{pool_end:08X} ({} KB)",
        pool.len() / 1024
    );
    println!("[prim-trace] {} accepted prims", prims.len());
    let mut count_pairs: Vec<(&str, usize)> = counts.iter().map(|(k, v)| (*k, *v)).collect();
    count_pairs.sort_by_key(|b| std::cmp::Reverse(b.1));
    for (k, v) in &count_pairs {
        println!("    {k:<10} {v}");
    }
    println!(
        "[topology] {} tagged prims, {} chain head(s), {} terminator(s), {} linked",
        topology.total_tags,
        topology.heads.len(),
        topology.terminators,
        topology.linked
    );
    if !topology.heads.is_empty() {
        let first = topology.heads[0];
        let head_addr = pool_base + first as u32;
        println!(
            "[topology] first chain head at pool-offset 0x{first:X} (kuseg 0x{head_addr:08X})"
        );
    }

    // 2) Tile signature clustering.
    let sigs = prim_pool::tile_signatures(&prims);
    println!(
        "[tiles] {} unique POLY_FT4 (clut,tpage,uvs) clusters",
        sigs.len()
    );
    for (i, s) in sigs.iter().take(top).enumerate() {
        println!(
            "  #{:<3} clut=0x{:04X} tpage=0x{:04X} uvs={:?}  hits={}",
            i, s.clut, s.tpage, s.uvs, s.count
        );
    }

    // 3) Search the default + user windows.
    let mut all_windows: Vec<(u32, u32)> = DEFAULT_WINDOWS.to_vec();
    all_windows.extend(extra_windows);

    #[derive(Serialize)]
    struct ClusterReport {
        cluster_index: usize,
        clut: u16,
        tpage: u16,
        uvs: [(u8, u8); 4],
        count_in_pool: usize,
        windows: Vec<WindowHit>,
    }
    #[derive(Serialize)]
    struct WindowHit {
        window_start: u32,
        window_end: u32,
        match_count: usize,
        dominant_gap: Option<usize>,
        dominant_gap_share: f64,
        gap_histogram: Vec<(usize, usize)>,
        first_addrs: Vec<u32>,
    }
    #[derive(Serialize)]
    struct PooledReport {
        window_start: u32,
        window_end: u32,
        total_matches: usize,
        dominant_gap: Option<usize>,
        dominant_gap_share: f64,
        gap_histogram: Vec<(usize, usize)>,
    }
    #[derive(Serialize)]
    struct TraceReport {
        save: String,
        pool_base: u32,
        pool_end: u32,
        prim_count: usize,
        unique_signatures: usize,
        topology_heads: usize,
        topology_terminators: usize,
        top_clusters: Vec<ClusterReport>,
        pooled: Vec<PooledReport>,
    }

    let mut top_clusters = Vec::new();
    let mut pooled_reports = Vec::new();

    for &(ws, we) in &all_windows {
        let wlo = (ws - PSX_RAM_KSEG0) as usize;
        let whi = (we - PSX_RAM_KSEG0) as usize;
        if whi > ram.len() {
            eprintln!("[warn] window 0x{ws:08X}..0x{we:08X} extends past main RAM, clamping");
        }
        let win = &ram[wlo..whi.min(ram.len())];
        println!(
            "[search] window 0x{ws:08X}..0x{we:08X} ({} KB)",
            win.len() / 1024
        );
        let fp_labels = ["rich", "packet", "uv+clut", "uv+tpage", "clut+tpage"];
        // Pooled scan: union of every cluster's hits in this window, per
        // fingerprint shape.
        let mut union_offsets_by_fp: Vec<Vec<usize>> = vec![Vec::new(); fp_labels.len()];
        for (i, s) in sigs.iter().take(top).enumerate() {
            for (fp_idx, fp) in s.fingerprints.iter().enumerate() {
                let offs = source_hunt::search(win, fp);
                union_offsets_by_fp[fp_idx].extend(offs.iter().copied());
                let stride = source_hunt::stride(&offs);
                if stride.match_count == 0 {
                    continue;
                }
                let first_addrs: Vec<u32> = offs.iter().take(6).map(|o| ws + *o as u32).collect();
                let dom = stride
                    .dominant_gap
                    .map(|g| format!("stride {g}"))
                    .unwrap_or_else(|| {
                        format!("mixed (top {:.0}%)", stride.dominant_gap_share * 100.0)
                    });
                println!(
                    "    cluster #{i:<3} clut=0x{:04X} tpage=0x{:04X}  fp={:<8} matches={}  {dom}  first={:?}",
                    s.clut, s.tpage, fp_labels[fp_idx], stride.match_count, first_addrs
                );
                if (i == 0 || stride.dominant_gap.is_some())
                    && (fp_idx <= 1 || stride.match_count >= 10)
                {
                    let wh = WindowHit {
                        window_start: ws,
                        window_end: we,
                        match_count: stride.match_count,
                        dominant_gap: stride.dominant_gap,
                        dominant_gap_share: stride.dominant_gap_share,
                        gap_histogram: stride.gap_histogram.clone(),
                        first_addrs,
                    };
                    if let Some(existing) = top_clusters
                        .iter_mut()
                        .find(|c: &&mut ClusterReport| c.cluster_index == i)
                    {
                        existing.windows.push(wh);
                    } else {
                        top_clusters.push(ClusterReport {
                            cluster_index: i,
                            clut: s.clut,
                            tpage: s.tpage,
                            uvs: s.uvs,
                            count_in_pool: s.count,
                            windows: vec![wh],
                        });
                    }
                }
            }
        }
        // For pooled stride we use the richest fingerprint shape that had any hits.
        let union_offsets = union_offsets_by_fp
            .iter()
            .find(|v| !v.is_empty())
            .cloned()
            .unwrap_or_default();
        for (fp_idx, label) in fp_labels.iter().enumerate() {
            let n = union_offsets_by_fp[fp_idx].len();
            if n > 0 {
                println!("    pooled  fp={label:<8} union_matches={n}");
            }
        }
        let pooled = source_hunt::pooled_stride(std::slice::from_ref(&union_offsets));
        println!(
            "    pooled  matches={}  dominant_gap={:?}  share={:.0}%",
            pooled.match_count,
            pooled.dominant_gap,
            pooled.dominant_gap_share * 100.0
        );
        if !pooled.gap_histogram.is_empty() {
            let topgaps: Vec<String> = pooled
                .gap_histogram
                .iter()
                .take(4)
                .map(|(g, c)| format!("{g}({c})"))
                .collect();
            println!("    pooled  top gaps: {}", topgaps.join(" "));
        }
        pooled_reports.push(PooledReport {
            window_start: ws,
            window_end: we,
            total_matches: pooled.match_count,
            dominant_gap: pooled.dominant_gap,
            dominant_gap_share: pooled.dominant_gap_share,
            gap_histogram: pooled.gap_histogram,
        });
        // Per-window stride autocorrelation. The dominant record stride
        // of a structured region jumps out as a score significantly
        // above the ~1/256 noise floor.
        let strides = [4, 8, 12, 16, 20, 24, 28, 32, 40, 48, 56, 64];
        let auto = source_hunt::autocorr_strides(win, &strides);
        let top: Vec<String> = auto
            .iter()
            .take(4)
            .map(|s| format!("{}:{:.3}", s.stride, s.score))
            .collect();
        println!("    autocorr top: {}", top.join(" "));
    }

    if scan_all_ram {
        println!("[scan-all-ram] searching full 2 MiB main RAM for top-{top} cluster fingerprints");
        let fp_labels = ["rich", "packet", "uv+clut", "uv+tpage", "clut+tpage"];
        // Hide pool window matches - those are self-matches inside the
        // live prim packets, not source data.
        let pool_lo = (pool_base - PSX_RAM_KSEG0) as usize;
        let pool_hi = (pool_end - PSX_RAM_KSEG0) as usize;
        for (i, s) in sigs.iter().take(top).enumerate() {
            for (fp_idx, fp) in s.fingerprints.iter().enumerate() {
                if fp.len() < 4 {
                    continue; // skip too-noisy short fingerprints
                }
                let all_offs = source_hunt::search(ram, fp);
                let non_pool_offs: Vec<usize> = all_offs
                    .into_iter()
                    .filter(|o| !(pool_lo..pool_hi).contains(o))
                    .collect();
                if non_pool_offs.is_empty() {
                    continue;
                }
                let first: Vec<u32> = non_pool_offs
                    .iter()
                    .take(6)
                    .map(|o| PSX_RAM_KSEG0 + *o as u32)
                    .collect();
                let stride = source_hunt::stride(&non_pool_offs);
                let dom = stride
                    .dominant_gap
                    .map(|g| format!("stride {g}"))
                    .unwrap_or_else(|| {
                        format!("mixed (top {:.0}%)", stride.dominant_gap_share * 100.0)
                    });
                println!(
                    "  cluster #{i:<3} clut=0x{:04X} tpage=0x{:04X} fp={:<10} matches={} (excl pool) {dom}  first={:?}",
                    s.clut,
                    s.tpage,
                    fp_labels[fp_idx],
                    non_pool_offs.len(),
                    first
                );
            }
        }
    }

    if let Some(path) = json_out {
        let report = TraceReport {
            save: save_path.display().to_string(),
            pool_base,
            pool_end,
            prim_count: prims.len(),
            unique_signatures: sigs.len(),
            topology_heads: topology.heads.len(),
            topology_terminators: topology.terminators,
            top_clusters,
            pooled: pooled_reports,
        };
        let json = serde_json::to_string_pretty(&report)?;
        std::fs::write(path, json).with_context(|| format!("writing {}", path.display()))?;
        println!("[json] wrote {}", path.display());
    }
    Ok(())
}

fn cmd_prim_dispatch_table(save: &Path, overlay_targets_only: bool) -> Result<()> {
    use legaia_mednafen::prim_dispatch::{
        HIGH_MODE_END, LOW_MODE_END, LOW_MODE_START, SLOT_BYTES, SlotKind, classify, decode_both,
    };

    let s = SaveState::from_path(save)?;
    let ram = s.main_ram()?;
    let (scus_table, overlay_table) = decode_both(ram)?;

    if overlay_targets_only {
        for tgt in overlay_table.high_mode_targets() {
            println!("0x{tgt:08X}");
        }
        return Ok(());
    }

    println!("[info] {}", save.display());
    println!(
        "[info] SCUS table @ 0x{:08X}  ({} alpha rows × {} slots)",
        scus_table.base,
        scus_table.rows.len(),
        scus_table.rows[0].slots.len()
    );
    let overlay_status = if overlay_table.is_empty() {
        "empty - world-map overlay not paged in"
    } else if overlay_table.looks_like_dispatch_table() {
        "populated (world-map overlay loaded)"
    } else {
        "leftover overlay code, NOT a dispatch table"
    };
    println!(
        "[info] overlay table @ 0x{:08X}  ({} alpha row(s); {})",
        overlay_table.base,
        overlay_table.rows.len(),
        overlay_status,
    );
    println!();

    let print_table = |label: &str, t: &legaia_mednafen::prim_dispatch::DispatchTable| {
        println!("=== {label} (base 0x{:08X}) ===", t.base);
        for (row_idx, row) in t.rows.iter().enumerate() {
            println!("  alpha row #{row_idx}  (+0x{:02X})", row.alpha_offset);
            for slot_idx in LOW_MODE_START..HIGH_MODE_END {
                let val = row.slots[slot_idx];
                let kind = classify(val);
                let kind_s = match kind {
                    SlotKind::Zero => "zero",
                    SlotKind::Scus => "SCUS",
                    SlotKind::Overlay => "OVERLAY",
                    SlotKind::Other => "OTHER",
                };
                let band = if slot_idx < LOW_MODE_END {
                    "low "
                } else if slot_idx < HIGH_MODE_END {
                    "high"
                } else {
                    "?"
                };
                let slot_addr = t.base + row.alpha_offset + slot_idx as u32 * SLOT_BYTES;
                println!(
                    "    [{band}] slot {slot_idx:>2}  @ 0x{slot_addr:08X}  ->  \
                     0x{val:08X}  {kind_s}"
                );
            }
        }
    };

    print_table("SCUS-resident dispatch table", &scus_table);
    println!();
    print_table("Overlay-resident dispatch table", &overlay_table);

    if overlay_table.looks_like_dispatch_table() {
        let scus_high = scus_table.high_mode_targets();
        let overlay_high = overlay_table.high_mode_targets();
        println!();
        println!(
            "=== high-mode targets (the per-prim emit leaves) ===\n\
             SCUS    : {} unique\n\
             overlay : {} unique\n\
             swap-in : the overlay-resident high-mode renderers are the\n\
                       bulk-continent emit leaves the world-map top-view\n\
                       routes its TMD prims through.",
            scus_high.len(),
            overlay_high.len()
        );
        for tgt in &overlay_high {
            let in_scus = scus_high.contains(tgt);
            let mark = if in_scus {
                "(shared with SCUS)"
            } else {
                "(overlay-only)"
            };
            println!("  0x{tgt:08X}  {mark}");
        }
        // Quick sanity check: any overlay-table slot whose pointer
        // lands outside the documented overlay window indicates the
        // world-map overlay actually extends past 0x801F9000 - flag it.
        let stragglers: Vec<u32> = overlay_high
            .iter()
            .copied()
            .filter(|p| classify(*p) == SlotKind::Other)
            .collect();
        if !stragglers.is_empty() {
            println!(
                "\nWARNING: {} overlay-table target(s) classified as OTHER \
                 (outside known overlay window); re-extract with a wider \
                 window:\n  {:?}",
                stragglers.len(),
                stragglers
                    .iter()
                    .map(|p| format!("0x{p:08X}"))
                    .collect::<Vec<_>>(),
            );
        }
    }
    Ok(())
}

fn cmd_prim_dispatch_survey(saves: &[PathBuf]) -> Result<()> {
    use legaia_mednafen::prim_dispatch::{
        HIGH_MODE_END, HIGH_MODE_START, SCUS_ALPHA_ROWS, SCUS_TABLE_BASE, SLOT_BYTES, classify,
        decode, decode_both,
    };

    if saves.len() < 2 {
        anyhow::bail!("prim-dispatch-survey requires at least 2 save states");
    }

    println!("[info] surveying {} save state(s)", saves.len());

    let mut entries: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    for path in saves {
        let s = SaveState::from_path(path)?;
        let ram = s.main_ram()?;
        entries.push((path.clone(), ram.to_vec()));
    }

    // SCUS invariant. Use the first save as anchor; compare the
    // populated slot range (12..20) on every alpha row to every other
    // save.
    let (anchor_path, anchor_ram) = &entries[0];
    let anchor = decode(anchor_ram, SCUS_TABLE_BASE, SCUS_ALPHA_ROWS)?;
    let mut drift_count = 0;
    for (path, ram) in &entries[1..] {
        let here = decode(ram, SCUS_TABLE_BASE, SCUS_ALPHA_ROWS)?;
        for (row_idx, (ra, rh)) in anchor.rows.iter().zip(here.rows.iter()).enumerate() {
            for slot_idx in HIGH_MODE_START..HIGH_MODE_END {
                if ra.slots[slot_idx] != rh.slots[slot_idx] {
                    drift_count += 1;
                    if drift_count <= 8 {
                        println!(
                            "WARN: SCUS table drift {}:row{row_idx}:slot{slot_idx} \
                             vs {}: 0x{:08X} != 0x{:08X}",
                            path.display(),
                            anchor_path.display(),
                            rh.slots[slot_idx],
                            ra.slots[slot_idx]
                        );
                    }
                }
            }
        }
    }
    if drift_count == 0 {
        println!(
            "[ok]   SCUS dispatch table @ 0x{:08X} is byte-identical across all \
             surveyed saves (high-mode slots {}..{}).",
            SCUS_TABLE_BASE,
            HIGH_MODE_START,
            HIGH_MODE_END - 1
        );
    } else {
        println!(
            "ERROR: SCUS dispatch table drifted in {drift_count} slot(s) - the SCUS \
             code region should be immutable. Re-extract or re-import the saves."
        );
    }

    println!();
    println!(
        "{:<48}  {:>6}  {:<40}  high-mode targets",
        "save", "status", "summary"
    );
    println!("{}", "-".repeat(140));
    for (path, ram) in &entries {
        let (_scus, overlay) = decode_both(ram)?;
        let (status, summary) = if overlay.is_empty() {
            ("empty", "world-map overlay NOT paged in".to_string())
        } else if overlay.looks_like_dispatch_table() {
            (
                "POP",
                format!(
                    "world-map overlay loaded ({} high-mode targets)",
                    overlay.high_mode_targets().len()
                ),
            )
        } else {
            (
                "stale",
                "leftover overlay code, not a dispatch table".to_string(),
            )
        };
        let targets = overlay
            .high_mode_targets()
            .iter()
            .map(|t| {
                use legaia_mednafen::prim_dispatch::SlotKind;
                let mark = match classify(*t) {
                    SlotKind::Overlay => "",
                    SlotKind::Scus => "(SCUS!)",
                    SlotKind::Zero => "(zero!)",
                    SlotKind::Other => "(OTHER!)",
                };
                format!("0x{t:08X}{mark}")
            })
            .collect::<Vec<_>>()
            .join(" ");
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let truncated = if name.len() > 48 {
            format!("…{}", &name[name.len() - 47..])
        } else {
            name
        };
        println!("{truncated:<48}  {status:>6}  {summary:<40}  {targets}");
    }
    let row_bytes = SLOT_BYTES * legaia_mednafen::prim_dispatch::SLOTS_PER_ROW as u32;
    println!(
        "\n[info] row stride = 0x{row_bytes:X} bytes; high-mode slots = {}..{}",
        HIGH_MODE_START,
        HIGH_MODE_END - 1
    );
    if drift_count > 0 {
        anyhow::bail!("SCUS dispatch table drift detected; see warnings above");
    }
    Ok(())
}

/// World-map top-view camera-state globals. See `docs/subsystems/world-map.md`
/// section "Globals used". The X/Z scrolls are stored as negated
/// map-origin coordinates; the negation is applied here so `cam_x` /
/// `cam_z` are camera-target world units.
const CAM_X_SCROLL: u32 = 0x80089120;
const CAM_Z_SCROLL: u32 = 0x80089118;
const CAM_AZIMUTH: u32 = 0x8007B794;
const CAM_ZOOM_MODE: u32 = 0x8007B6F4;
const VIEW_MODE_FLAG: u32 = 0x801F2B94;

#[derive(Debug)]
struct CameraState {
    raw_x: i32,
    raw_z: i32,
    raw_az: i32,
    raw_zoom_mode: u32,
    view_mode: u8,
}

impl CameraState {
    fn from_ram(ram: &[u8]) -> Result<Self> {
        let raw_x = read_i32_le(ram, CAM_X_SCROLL)?;
        let raw_z = read_i32_le(ram, CAM_Z_SCROLL)?;
        let raw_az = read_i32_le(ram, CAM_AZIMUTH)?;
        let raw_zoom_mode = read_u32_le(ram, CAM_ZOOM_MODE)?;
        let view_mode = ram_slice(ram, VIEW_MODE_FLAG, VIEW_MODE_FLAG + 1)?[0];
        Ok(Self {
            raw_x,
            raw_z,
            raw_az,
            raw_zoom_mode,
            view_mode,
        })
    }
    fn cam_x(&self) -> i32 {
        -self.raw_x
    }
    fn cam_z(&self) -> i32 {
        -self.raw_z
    }
    fn view_label(&self) -> &'static str {
        match self.view_mode {
            0 => "walk",
            1 => "top",
            _ => "?",
        }
    }
}

fn read_u32_le(ram: &[u8], addr: u32) -> Result<u32> {
    let s = ram_slice(ram, addr, addr + 4)?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn read_i32_le(ram: &[u8], addr: u32) -> Result<i32> {
    Ok(read_u32_le(ram, addr)? as i32)
}

fn cmd_world_map_camera(saves: &[PathBuf], table: bool) -> Result<()> {
    if saves.is_empty() {
        bail!("at least one save state is required");
    }
    let mut decoded = Vec::with_capacity(saves.len());
    for path in saves {
        let s = SaveState::from_path(path)?;
        let ram = s.main_ram()?;
        decoded.push((path.clone(), CameraState::from_ram(ram)?));
    }
    if table {
        println!(
            "{:<48}  {:>4}  {:>10}  {:>10}  {:>10}  {:>10}  {:>10}",
            "save", "view", "raw_x", "raw_z", "cam_x", "cam_z", "az/zoom"
        );
        println!("{}", "-".repeat(120));
        for (path, c) in &decoded {
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            let truncated = if name.len() > 48 {
                format!("…{}", &name[name.len() - 47..])
            } else {
                name
            };
            println!(
                "{:<48}  {:>4}  {:>10}  {:>10}  {:>10}  {:>10}  az=0x{:04X} zoom=0x{:04X}",
                truncated,
                c.view_label(),
                c.raw_x,
                c.raw_z,
                c.cam_x(),
                c.cam_z(),
                (c.raw_az as u32) & 0xFFFF,
                c.raw_zoom_mode & 0xFFFF
            );
        }
        let top_view_count = decoded.iter().filter(|(_, c)| c.view_mode == 1).count();
        println!();
        println!(
            "[info] {}/{} save state(s) captured in top-view mode (DAT_801F2B94 = 1)",
            top_view_count,
            decoded.len()
        );
        if top_view_count == 0 {
            println!(
                "[warn] all captured saves are in walk-view; cam_x/cam_z reflect \
                 load-time map-origin only, not an interactively-scrolled camera \
                 position. Re-capture in top-view debug mode (dev menu) to get \
                 true camera defaults."
            );
        }
    } else {
        for (path, c) in &decoded {
            println!("{}", path.display());
            println!(
                "  view-mode flag (DAT_801F2B94)        = {} ({})",
                c.view_mode,
                c.view_label()
            );
            println!(
                "  _DAT_80089120  raw i32                = {} (0x{:08X})",
                c.raw_x, c.raw_x as u32
            );
            println!(
                "  _DAT_80089118  raw i32                = {} (0x{:08X})",
                c.raw_z, c.raw_z as u32
            );
            println!("  cam_x = -_DAT_80089120                = {}", c.cam_x());
            println!("  cam_z = -_DAT_80089118                = {}", c.cam_z());
            println!(
                "  _DAT_8007B794  azimuth (low u16)      = 0x{:04X} ({})",
                (c.raw_az as u32) & 0xFFFF,
                c.raw_az & 0xFFFF
            );
            println!(
                "  _DAT_8007B6F4  zoom/mode (low u16)    = 0x{:04X} ({})",
                c.raw_zoom_mode & 0xFFFF,
                c.raw_zoom_mode & 0xFFFF
            );
            println!();
        }
    }
    Ok(())
}

#[cfg(test)]
mod camera_decode_tests {
    use super::*;

    fn synth_ram_with(values: &[(u32, &[u8])]) -> Vec<u8> {
        let mut ram = vec![0u8; PSX_RAM_SIZE];
        for (addr, bytes) in values {
            let off = (*addr - PSX_RAM_KSEG0) as usize;
            ram[off..off + bytes.len()].copy_from_slice(bytes);
        }
        ram
    }

    #[test]
    fn decode_drake_walk_view_capture() {
        // Mirrors a captured world-map walk-view state: raw_x =
        // -8832 (0xFFFFDD80), raw_z = -8832, zoom-mode = 0x0170,
        // view-mode flag = 0.
        let ram = synth_ram_with(&[
            (CAM_X_SCROLL, &(-8832i32).to_le_bytes()),
            (CAM_Z_SCROLL, &(-8832i32).to_le_bytes()),
            (CAM_AZIMUTH, &0u32.to_le_bytes()),
            (CAM_ZOOM_MODE, &0x0170u32.to_le_bytes()),
            (VIEW_MODE_FLAG, &[0u8]),
        ]);
        let c = CameraState::from_ram(&ram).unwrap();
        assert_eq!(c.raw_x, -8832);
        assert_eq!(c.raw_z, -8832);
        assert_eq!(c.cam_x(), 8832);
        assert_eq!(c.cam_z(), 8832);
        assert_eq!(c.raw_zoom_mode & 0xFFFF, 0x0170);
        assert_eq!(c.view_mode, 0);
        assert_eq!(c.view_label(), "walk");
    }

    #[test]
    fn decode_top_view_flag_labels_correctly() {
        let ram = synth_ram_with(&[
            (CAM_X_SCROLL, &0u32.to_le_bytes()),
            (CAM_Z_SCROLL, &0u32.to_le_bytes()),
            (CAM_AZIMUTH, &0u32.to_le_bytes()),
            (CAM_ZOOM_MODE, &0u32.to_le_bytes()),
            (VIEW_MODE_FLAG, &[1u8]),
        ]);
        let c = CameraState::from_ram(&ram).unwrap();
        assert_eq!(c.view_mode, 1);
        assert_eq!(c.view_label(), "top");
    }

    #[test]
    fn cam_negation_matches_overlay_convention() {
        // `_DAT_80089118 = -(int)*(short *)(actor + 0x14)` in
        // overlay_0978 + slot_machine means: cam_z is the negation of
        // the raw cell. A positive raw_z must round-trip to negative
        // cam_z.
        let ram = synth_ram_with(&[
            (CAM_X_SCROLL, &1234i32.to_le_bytes()),
            (CAM_Z_SCROLL, &5678i32.to_le_bytes()),
            (CAM_AZIMUTH, &0u32.to_le_bytes()),
            (CAM_ZOOM_MODE, &0u32.to_le_bytes()),
            (VIEW_MODE_FLAG, &[0u8]),
        ]);
        let c = CameraState::from_ram(&ram).unwrap();
        assert_eq!(c.cam_x(), -1234);
        assert_eq!(c.cam_z(), -5678);
    }
}
