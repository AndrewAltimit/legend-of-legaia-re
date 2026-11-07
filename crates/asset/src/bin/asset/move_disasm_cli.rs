//! `asset move-disasm` - list move-VM programs (`FUN_80023070`'s u16
//! bytecode, with the `0x2F` overlay sub-ops) out of a code image or a
//! prescript stager table.
//!
//! Modes:
//!
//! - `file <PATH>`: walk one program, or one record (`--record`), at `--at`
//!   (a VA when `--base` is given, else a file offset).
//! - `slot-b <PATH>`: every spawn record of a slot-B cast / summon module
//!   image (PROT 0903..0966), bounded by `slot_b_module`'s record band.
//! - `prescript <PATH>`: every record of a scene's prescript stager table
//!   (`[u16 count][u16 offsets]`, `scene_event_scripts::move_stager_records`'
//!   layout).
//!
//! The decoder is `legaia_overlay_images::move_disasm`; nothing here runs the
//! VM.

use std::fs;

use anyhow::{Context, Result, anyhow, bail};
use clap::Subcommand;

use legaia_overlay_images::move_disasm::{
    Addressing, WalkOptions, disassemble, disassemble_record, format_insn, format_record,
};
use legaia_overlay_images::slot_b_module;

#[derive(Subcommand, Debug)]
pub(crate) enum MoveDisasmCmd {
    /// Walk one program (or record) in a raw buffer.
    File {
        path: String,
        /// The buffer's load base (e.g. `0x801F69D8` for a slot-B module).
        /// With it, `--at` / `--end` are VAs and the listing prints VAs.
        #[arg(long, value_parser = parse_u32)]
        base: Option<u32>,
        /// Where to start: a VA with `--base`, else a file offset.
        #[arg(long, value_parser = parse_u32, default_value = "0")]
        at: u32,
        /// Stop before this address (same space as `--at`).
        #[arg(long, value_parser = parse_u32)]
        end: Option<u32>,
        /// `--at` names a record (`[model_sel][reserved]` header first).
        #[arg(long)]
        record: bool,
        /// Keep walking past a `HALT` (consecutive records in a band).
        #[arg(long)]
        through_halt: bool,
        /// Stop after this many instructions.
        #[arg(long, default_value_t = 4096)]
        max_insns: usize,
    },
    /// Every spawn record of a slot-B cast / summon module image.
    SlotB {
        path: String,
        /// Link base; defaults to the band's `0x801F69D8`.
        #[arg(long, value_parser = parse_u32)]
        base: Option<u32>,
        /// Only the record at this VA.
        #[arg(long, value_parser = parse_u32)]
        record: Option<u32>,
        /// Also list the records chained above the highest credited one.
        #[arg(long)]
        chained: bool,
    },
    /// Every record of a scene prescript stager table.
    Prescript {
        path: String,
        /// Only this record id.
        #[arg(long)]
        id: Option<usize>,
    },
}

fn parse_u32(s: &str) -> Result<u32, String> {
    let t = s.trim();
    let r = match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(h) => u32::from_str_radix(h, 16),
        None => t.parse(),
    };
    r.map_err(|e| format!("{s}: {e}"))
}

/// Run one `asset move-disasm` subcommand.
pub(crate) fn move_disasm(cmd: MoveDisasmCmd) -> Result<()> {
    match cmd {
        MoveDisasmCmd::File {
            path,
            base,
            at,
            end,
            record,
            through_halt,
            max_insns,
        } => {
            let bytes = fs::read(&path).with_context(|| format!("reading {path}"))?;
            let off = |a: u32| -> Result<usize> {
                let o = match base {
                    Some(b) => a
                        .checked_sub(b)
                        .ok_or_else(|| anyhow!("{a:#X} below the base {b:#X}"))?,
                    None => a,
                };
                Ok(o as usize)
            };
            let start = off(at)?;
            if start >= bytes.len() {
                bail!("{at:#X} is past the buffer ({} bytes)", bytes.len());
            }
            let opts = WalkOptions {
                through_halt,
                max_insns,
                end: end.map(off).transpose()?,
            };
            let addr = Addressing { base };
            if record {
                let r = disassemble_record(&bytes, start, opts)
                    .ok_or_else(|| anyhow!("no record header at {at:#X}"))?;
                print!("{}", format_record(&r, addr));
            } else {
                for i in disassemble(&bytes, start, opts) {
                    println!("{}", format_insn(&i, addr));
                }
            }
            Ok(())
        }
        MoveDisasmCmd::SlotB {
            path,
            base,
            record,
            chained,
        } => {
            let bytes = fs::read(&path).with_context(|| format!("reading {path}"))?;
            let base = base.unwrap_or(slot_b_module::SLOT_B_LINK_BASE);
            let layout = slot_b_module::parse_at(&bytes, base);
            let addr = Addressing { base: Some(base) };
            let mut spans: Vec<_> = layout.records.iter().collect();
            if chained {
                spans.extend(layout.chained_records.iter());
            }
            println!(
                "// {path}: {} records ({} chained) at base {base:08X}",
                layout.records.len(),
                layout.chained_records.len()
            );
            for s in spans {
                let va = base.wrapping_add(s.start as u32);
                if record.is_some_and(|r| r != va) {
                    continue;
                }
                let opts = WalkOptions {
                    end: Some(s.end),
                    ..WalkOptions::default()
                };
                if let Some(r) = disassemble_record(&bytes, s.start, opts) {
                    print!("{}", format_record(&r, addr));
                }
            }
            Ok(())
        }
        MoveDisasmCmd::Prescript { path, id } => {
            let bytes = fs::read(&path).with_context(|| format!("reading {path}"))?;
            let records = legaia_scene_formats::scene_event_scripts::move_stager_records(&bytes)
                .ok_or_else(|| anyhow!("{path}: no prescript stager table"))?;
            let addr = Addressing::default();
            for (k, rec) in records.iter().enumerate() {
                if id.is_some_and(|i| i != k) {
                    continue;
                }
                println!("// record {k}");
                let opts = WalkOptions {
                    end: Some(rec.bytecode.end),
                    ..WalkOptions::default()
                };
                if let Some(r) = disassemble_record(&bytes, rec.record_off, opts) {
                    print!("{}", format_record(&r, addr));
                }
            }
            Ok(())
        }
    }
}
