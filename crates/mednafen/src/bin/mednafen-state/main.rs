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

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod gpu_ops;
mod prim;
mod ram_ops;
mod world_map;

use gpu_ops::{cmd_clut_trace, cmd_spu, cmd_vram_dump};
use prim::{cmd_prim_dispatch_survey, cmd_prim_dispatch_table, cmd_prim_trace};
use ram_ops::{
    DiffArgs, cmd_bisect, cmd_diff, cmd_extract, cmd_info, cmd_scenarios, cmd_trace, cmd_watch,
    cmd_write_taxonomy,
};
use world_map::cmd_world_map_camera;

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
        /// Instead of the full 1024x512 VRAM, write only the on-screen
        /// framebuffer: the display-area sub-rectangle (`display_fb` origin
        /// sized by the decoded resolution, e.g. 320x240 for Legaia). This
        /// is what the player actually sees - the right crop for comparing
        /// menu / HUD pixels against the engine renderer.
        #[arg(long)]
        display_crop: bool,
    },
    /// Dump the SPU reverb-routing snapshot: master reverb enable (SPUCNT
    /// bit 7), reverb mode, and the per-voice reverb-send mask (EON), plus a
    /// per-voice line (active / reverb-on / volume). This is the read side of
    /// the "which voices does retail reverb" question for the C7-REVERB live
    /// routing hunt - pure-Rust over an existing save state, no live probe.
    Spu {
        save: PathBuf,
        /// Print all 24 voices instead of only the audible ones.
        #[arg(long)]
        all: bool,
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
            display_crop,
        } => cmd_vram_dump(&save, &out, out_bin.as_deref(), regs, display_crop),
        Cmd::Spu { save, all } => cmd_spu(&save, all),
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
