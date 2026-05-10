//! field-disasm: walk field-VM bytecode and print mnemonics.
//!
//! Modes:
//!
//! - `file <PATH>`: treat the entire file as one field-VM script and walk it.
//! - `scene-event-scripts <PATH>`: parse the leading prescript table and
//!   walk every record body individually. The prescript shape is
//!   `[u16 count][u16 offsets[count]][record bytecode...]`. Detector lives
//!   in `legaia_asset::scene_event_scripts`.
//! - `scan-prot --disc <PATH> [--cdname <PATH>]`: walk every PROT.DAT entry,
//!   detect scene-event-scripts containers, and print every FMV trigger
//!   (`0x4C 0xE2`) found, annotated with the CDNAME label of the enclosing
//!   PROT entry. This is the "lift the per-scene MV index" workflow.
//!
//! In any mode, `--fmv-only` filters the output to FMV trigger lines only.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};

use legaia_asset::scene_event_scripts;
use legaia_engine_vm::field_disasm::{
    InsnInfo, LinearWalker, MenuCtrlKind, find_fmv_triggers, format_instruction,
};
use legaia_prot::archive::Archive;
use legaia_prot::cdname;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Disassemble field-VM bytecode (FUN_801DE840 opcode set)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Treat `path` as a raw field-VM script body and walk it linearly.
    File {
        path: String,
        /// Filter output to FMV trigger lines only.
        #[arg(long)]
        fmv_only: bool,
        /// Stop after `n` instructions (useful for huge buffers).
        #[arg(long)]
        max_insns: Option<usize>,
    },
    /// Detect a scene-event-scripts prescript at the start of `path` and
    /// walk every record body.
    SceneEventScripts {
        path: String,
        #[arg(long)]
        fmv_only: bool,
        /// Print only the per-record summary (one line per record).
        #[arg(long)]
        summary: bool,
        /// Restrict output to a single record by index.
        #[arg(long)]
        record: Option<usize>,
    },
    /// Walk every PROT.DAT entry; for each scene-event-scripts hit, list
    /// the FMV triggers found inside.
    ScanProt {
        /// Path to a Legaia PROT.DAT file (e.g. extracted/PROT.DAT).
        #[arg(long)]
        disc: String,
        /// Path to the CDNAME.TXT name map (optional but recommended).
        #[arg(long)]
        cdname: Option<String>,
        /// Restrict to PROT entries whose CDNAME label contains this substring.
        #[arg(long)]
        scene: Option<String>,
        /// By default, the scan only reports FMV triggers whose fmv_id is in
        /// the retail valid range (0..=5 = MV1.STR..MV6.STR). Pass this to
        /// disable filtering and see every coincidental 0x4C 0xE2 match.
        #[arg(long)]
        no_filter: bool,
        /// Run the scan over every PROT entry's raw bytes (not just
        /// detected scene-event-scripts containers). Reports byte-pattern
        /// matches for `0x4C 0xE2 lo hi 00 00` where the FMV index is in
        /// the retail valid range.
        #[arg(long)]
        bytewise: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::File {
            path,
            fmv_only,
            max_insns,
        } => cmd_file(&path, fmv_only, max_insns),
        Cmd::SceneEventScripts {
            path,
            fmv_only,
            summary,
            record,
        } => cmd_scene_event_scripts(&path, fmv_only, summary, record),
        Cmd::ScanProt {
            disc,
            cdname,
            scene,
            no_filter,
            bytewise,
        } => cmd_scan_prot(
            &disc,
            cdname.as_deref(),
            scene.as_deref(),
            no_filter,
            bytewise,
        ),
    }
}

fn cmd_file(path: &str, fmv_only: bool, max_insns: Option<usize>) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {path}"))?;
    println!(
        "// {}: {} bytes, walking from PC=0",
        Path::new(path).display(),
        bytes.len()
    );
    walk_buffer(&bytes, fmv_only, max_insns);
    Ok(())
}

fn cmd_scene_event_scripts(
    path: &str,
    fmv_only: bool,
    summary: bool,
    record: Option<usize>,
) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {path}"))?;
    let detection = scene_event_scripts::detect(&bytes).ok_or_else(|| {
        anyhow!(
            "{}: no scene-event-scripts prescript detected (try `file` mode if this isn't one)",
            path
        )
    })?;
    let ranges = scene_event_scripts::record_ranges(&bytes)
        .ok_or_else(|| anyhow!("malformed prescript table"))?;
    println!(
        "// {}: {} records (frame-opener rate {:.0}%)",
        Path::new(path).display(),
        detection.records,
        detection.frame_opener_rate * 100.0
    );
    for (i, &(start, end)) in ranges.iter().enumerate() {
        if let Some(only) = record
            && only != i
        {
            continue;
        }
        let body = &bytes[start..end];
        if summary {
            let triggers = find_fmv_triggers(body);
            let trigger_str = if triggers.is_empty() {
                String::from("-")
            } else {
                triggers
                    .iter()
                    .map(|(_, id)| format!("MV{}", id + 1))
                    .collect::<Vec<_>>()
                    .join(",")
            };
            println!(
                "  record[{:3}]  start=0x{:04X} len={:5}  fmv={}",
                i,
                start,
                body.len(),
                trigger_str
            );
            continue;
        }
        // Records begin with a 4-byte field-VM frame sentinel (`0xFFFF
        // 0x0000`) when the dispatcher's loop expects a fresh frame.
        // Skip past it before walking the opcode stream.
        let opcode_start = if body.len() >= 4
            && body[0] == 0xFF
            && body[1] == 0xFF
            && body[2] == 0
            && body[3] == 0
        {
            4
        } else {
            0
        };
        if fmv_only {
            let triggers = find_fmv_triggers(&body[opcode_start..]);
            if triggers.is_empty() {
                continue;
            }
            println!(
                "// record[{:3}]  start=0x{:04X} len={}",
                i,
                start,
                body.len()
            );
            for (pc, fmv_id) in triggers {
                println!(
                    "  0x{:04X}  FmvTrigger fmv_id={} ({})",
                    pc + opcode_start,
                    fmv_id,
                    legaia_engine_vm::field_disasm::fmv_filename(fmv_id)
                );
            }
            continue;
        }
        println!(
            "// record[{:3}]  start=0x{:04X} len={} (frame sentinel: {})",
            i,
            start,
            body.len(),
            if opcode_start > 0 { "yes" } else { "no" }
        );
        walk_buffer_at(body, opcode_start, false, None);
    }
    Ok(())
}

fn cmd_scan_prot(
    disc: &str,
    cdname_path: Option<&str>,
    scene_filter: Option<&str>,
    no_filter: bool,
    bytewise: bool,
) -> Result<()> {
    let mut archive = Archive::open(Path::new(disc))?;
    let map = match cdname_path {
        Some(p) => cdname::parse(Path::new(p))?,
        None => cdname::IndexMap::new(),
    };

    let mut buf = Vec::new();
    let mut total_hits = 0usize;
    let mut total_triggers = 0usize;
    let mode = if bytewise {
        "bytewise (every PROT entry)"
    } else {
        "scene-event-scripts containers only"
    };
    println!(
        "// scanning {} PROT entries from {} ({}, filter={})",
        archive.entries.len(),
        disc,
        mode,
        if no_filter {
            "off"
        } else {
            "on (fmv_id 0..=5)"
        }
    );
    let entries = archive.entries.clone();
    for entry in &entries {
        archive.read_entry(entry, &mut buf)?;
        let label = cdname::block_for(&map, entry.index).unwrap_or("");
        if let Some(filter) = scene_filter
            && !label.contains(filter)
        {
            continue;
        }
        let mut entry_triggers: Vec<(Option<usize>, usize, i16)> = Vec::new();
        if bytewise {
            // Brute-force byte-pattern scan for `0x4C 0xE2 lo hi`. The
            // dispatcher's PC math reserves the two trailing operand bytes
            // but doesn't check them, so we don't either - the in-range
            // fmv_id alone (0..=5) is a strong-enough fingerprint to surface
            // the seven retail FMV-bearing scenes. Window step = 1.
            let mut p = 0usize;
            while p + 4 <= buf.len() {
                if buf[p] == 0x4C && buf[p + 1] == 0xE2 {
                    let fmv_id = i16::from_le_bytes([buf[p + 2], buf[p + 3]]);
                    if no_filter || (0..=5).contains(&fmv_id) {
                        entry_triggers.push((None, p, fmv_id));
                    }
                }
                p += 1;
            }
        } else {
            let Some(_d) = scene_event_scripts::detect(&buf) else {
                continue;
            };
            let Some(ranges) = scene_event_scripts::record_ranges(&buf) else {
                continue;
            };
            for (i, &(start, end)) in ranges.iter().enumerate() {
                let body = &buf[start..end];
                for (pc, fmv_id) in find_fmv_triggers(body) {
                    if !no_filter && !(0..=5).contains(&fmv_id) {
                        continue;
                    }
                    entry_triggers.push((Some(i), start + pc, fmv_id));
                }
            }
        }
        if entry_triggers.is_empty() {
            continue;
        }
        total_hits += 1;
        total_triggers += entry_triggers.len();
        println!(
            "PROT[{:4}]  {:24}  triggers={}",
            entry.index,
            label,
            entry_triggers.len()
        );
        for (rec, pc, fmv_id) in entry_triggers {
            let rec_str = match rec {
                Some(r) => format!("record[{:3}]", r),
                None => "byte-scan ".into(),
            };
            println!(
                "    {}  pc=0x{:06X}  FmvTrigger fmv_id={} ({})",
                rec_str,
                pc,
                fmv_id,
                legaia_engine_vm::field_disasm::fmv_filename(fmv_id)
            );
        }
    }
    println!(
        "// done: {} entries with FMV triggers, {} triggers total",
        total_hits, total_triggers
    );
    Ok(())
}

fn walk_buffer(bytecode: &[u8], fmv_only: bool, max_insns: Option<usize>) {
    walk_buffer_at(bytecode, 0, fmv_only, max_insns);
}

fn walk_buffer_at(bytecode: &[u8], start_pc: usize, fmv_only: bool, max_insns: Option<usize>) {
    for (count, r) in LinearWalker::new(bytecode, start_pc).enumerate() {
        if let Some(limit) = max_insns
            && count >= limit
        {
            println!("// (truncated at {limit} instructions)");
            return;
        }
        match r {
            Ok(insn) => {
                if fmv_only {
                    let is_fmv = matches!(
                        insn.info,
                        InsnInfo::MenuCtrl {
                            kind: MenuCtrlKind::FmvTrigger { .. },
                            ..
                        }
                    );
                    if !is_fmv {
                        continue;
                    }
                }
                println!("{}", format_instruction(&insn, bytecode));
            }
            Err((pc, err)) => {
                if fmv_only {
                    continue;
                }
                println!(
                    "  0x{:04X}  {:24}  ; decode error: {}",
                    pc,
                    format!("{:02X}", bytecode[pc]),
                    err
                );
            }
        }
    }
}
