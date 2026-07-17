//! `mdt` CLI - inspect `move.mdt`-style buffers.
//!
//! `mdt classify <file>` runs both the consumer-expected offset-table parser and
//! the flat 128-byte-record parser, prints which one fits.
//!
//! `mdt records <file>` dumps the flat record table view (the loose shape the
//! extraction files named 0972/0973 parse as).
//!
//! NOTE: under the CDNAME +2 filename shift (`docs/formats/cdname.md`
//! § numbering space) the files named `0972/0973_move_program_no` are really
//! `other_game` minigame overlays (fishing + the `OTHER2` dev module); the
//! `move_program_no` define covers extraction 0970..0971, a `\DATA\MOV*.STR`
//! FMV program table. Neither is move-table data.
//!
//! `mdt slots <file>` dumps the offset-table view (what the runtime consumer
//! `FUN_800204f8` expects to read).

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use legaia_mdt::{MoveBuffer, RecordTable, Verdict, classify};

#[derive(Parser)]
#[command(
    name = "mdt",
    version,
    about = "Inspect move.mdt-style buffers (Tactical Arts move table)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run both interpretations and print which one fits.
    ///
    /// Input: an LZS-decoded PROT entry or per-scene Move buffer (e.g. from
    /// `lzs-decode` / `legaia-extract <disc.bin> --out extracted` output).
    Classify {
        file: PathBuf,
        #[arg(long, help = "emit JSON instead of human-readable")]
        json: bool,
    },
    /// Dump the flat 128-byte-record view (what 0972/0973 actually look like).
    /// Input: an LZS-decoded PROT entry (see `classify`).
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
    /// Input: an LZS-decoded PROT entry / scene Move buffer (see `classify`).
    Slots {
        file: PathBuf,
        #[arg(long, default_value_t = 32, help = "limit how many used slots to list")]
        limit: usize,
        #[arg(long, help = "emit JSON instead of human-readable")]
        json: bool,
    },
}

/// Rust ignores SIGPIPE by default; restore SIG_DFL so `mdt ... | head`
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
                if c.verdict_low_confidence {
                    // Never print a confident verdict that contradicts the
                    // strict fitness score printed just above.
                    println!("verdict: {:?} (LOW CONFIDENCE)", c.verdict);
                    println!("  note: only the relaxed move-buffer predicate matched; the strict");
                    println!(
                        "  offset-table fitness score is negative ({}), so this buffer does not",
                        c.offset_table_fit
                    );
                    println!(
                        "  cleanly match the runtime offset-table layout. Real per-scene Move"
                    );
                    println!("  buffers also score negative (short-table over-read), but so does");
                    println!(
                        "  unrelated data such as overlay code - cross-check with `mdt records`."
                    );
                } else if c.verdict == Verdict::Unknown {
                    println!("verdict: {:?} (does not match either layout)", c.verdict);
                } else {
                    println!("verdict: {:?}", c.verdict);
                }
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
