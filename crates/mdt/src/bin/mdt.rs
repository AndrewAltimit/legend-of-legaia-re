//! `mdt` CLI - inspect `move.mdt`-style buffers.
//!
//! `mdt classify <file>` runs both the consumer-expected offset-table parser and
//! the flat 128-byte-record parser, prints which one fits.
//!
//! `mdt records <file>` dumps the flat record table view (the structure
//! 0972/0973 actually have).
//!
//! `mdt slots <file>` dumps the offset-table view (what the runtime consumer
//! `FUN_800204f8` expects to read).

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use legaia_mdt::{MoveBuffer, RecordTable, classify};

#[derive(Parser)]
#[command(about = "Inspect move.mdt-style buffers (Tactical Arts move table)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run both interpretations and print which one fits.
    Classify {
        file: PathBuf,
        #[arg(long, help = "emit JSON instead of human-readable")]
        json: bool,
    },
    /// Dump the flat 128-byte-record view (what 0972/0973 actually look like).
    Records {
        file: PathBuf,
        #[arg(
            long,
            default_value_t = 16,
            help = "limit how many non-empty records to list"
        )]
        limit: usize,
        #[arg(long, help = "emit JSON instead of human-readable")]
        json: bool,
    },
    /// Dump the offset-table view (what FUN_800204f8 expects to read).
    Slots {
        file: PathBuf,
        #[arg(long, default_value_t = 32, help = "limit how many used slots to list")]
        limit: usize,
        #[arg(long, help = "emit JSON instead of human-readable")]
        json: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Classify { file, json } => {
            let buf = std::fs::read(&file).with_context(|| format!("read {}", file.display()))?;
            let c = classify(&buf)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&c)?);
            } else {
                println!("file: {}", file.display());
                println!("size: {} bytes", c.size);
                println!();
                println!("offset-table view (consumer FUN_800204f8 expectation):");
                println!("  used slots: {}", c.offset_table_used_slots);
                println!("  bogus offsets (out of buffer): {}", c.offset_table_bogus);
                println!("  fitness score: {}", c.offset_table_fit);
                println!();
                println!("flat 128-byte record view:");
                println!("  record count: {}", c.flat_table_records);
                println!("  non-empty: {}", c.flat_table_non_empty);
                println!("  trailing bytes: {}", c.flat_table_trailing);
                println!();
                println!("verdict: {:?}", c.verdict);
            }
        }
        Cmd::Records { file, limit, json } => {
            let buf = std::fs::read(&file).with_context(|| format!("read {}", file.display()))?;
            let rt = RecordTable::parse(&buf);
            if json {
                println!("{}", serde_json::to_string_pretty(&rt)?);
            } else {
                println!("file: {} ({} bytes)", file.display(), rt.size);
                println!(
                    "stride: {}, records: {}, trailing: {}",
                    rt.stride, rt.record_count, rt.trailing_bytes
                );
                println!("non-empty: {}", rt.non_empty_count());
                println!();
                println!("first {} non-empty records:", limit);
                for r in rt.records.iter().filter(|r| !r.all_zero).take(limit) {
                    let head = r
                        .head_8
                        .iter()
                        .map(|b| format!("{:02x}", b))
                        .collect::<Vec<_>>()
                        .join(" ");
                    println!(
                        "  rec[{:4}] head={}  body_nz={}  first_nz_off={:?}  head_tail=0x{:02x}",
                        r.index,
                        head,
                        r.body_nonzero_bytes,
                        r.body_first_nonzero_offset,
                        r.head_tail_byte,
                    );
                }
            }
        }
        Cmd::Slots { file, limit, json } => {
            let buf = std::fs::read(&file).with_context(|| format!("read {}", file.display()))?;
            let mb = MoveBuffer::parse(&buf)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&mb)?);
            } else {
                println!("file: {} ({} bytes)", file.display(), mb.size);
                println!("table entries probed: {}", mb.table_entries);
                println!(
                    "used slots: {}, bogus: {}",
                    mb.used_slots.len(),
                    mb.bogus_offsets
                );
                println!();
                println!("first {} used slots:", limit);
                for s in mb.used_slots.iter().take(limit) {
                    println!(
                        "  id={:4}  raw_off=0x{:08x}  in_table={}  past_end={}",
                        s.move_id, s.raw_offset, s.points_into_table, s.points_past_end,
                    );
                }
                println!();
                println!("decoded records ({} unique):", mb.records.len());
                for r in mb.records.iter().take(limit) {
                    println!(
                        "  off=0x{:05x}  flags=0x{:02x}  use_div={}  max_pos*16={}  div={}  ids={:?}",
                        r.offset,
                        r.flags,
                        r.use_divisor,
                        r.max_position_x16,
                        r.divisor,
                        r.referenced_by_ids,
                    );
                }
            }
        }
    }
    Ok(())
}
