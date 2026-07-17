//! `seq` - inspector for PsyQ SEQ files.
//!
//! Subcommands:
//!
//! - `info <PATH>` - print header (version, PPQN, tempo, time signature, BPM).
//! - `events <PATH>` - disassemble every event with absolute tick + delta.
//! - `json <PATH>` - full SEQ as machine-readable JSON.
//! - `find <PATH>` - locate every `pQES` magic in a blob and report offsets.
//!
//! `info`/`events`/`json` accept `--offset N` (decimal or 0x-hex) for SEQ
//! data embedded at a non-zero offset, and auto-scan for the first parseable
//! `pQES` when offset 0 doesn't parse.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use legaia_seq::{ChannelMessage, EventBody, MetaMessage, SEQ_MAGIC, Seq, parse_header};

#[derive(Parser)]
#[command(name = "seq", version, about = "Inspect PsyQ SEQ files")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

const OFFSET_HELP: &str = "Byte offset of the SEQ data inside the file (decimal or 0x-hex). \
Retail BGM PROT entries wrap the SEQ: [chunk hdr][VAB][chunk hdr][SEQ] - use `seq find` to \
locate the offset. Without --offset, the file is parsed at 0 and, failing that, at the first \
parseable pQES magic (auto-scan).";

#[derive(Subcommand)]
enum Cmd {
    /// Print header + summary (event counts).
    ///
    /// Input: a standalone SEQ, or a BGM PROT entry from `legaia-extract
    /// <disc.bin> --out extracted` (e.g. extracted/PROT/0990_music_01.BIN,
    /// where the SEQ sits at a non-zero offset - see --offset / `seq find`).
    Info {
        path: PathBuf,
        #[arg(long, value_parser = parse_offset, help = OFFSET_HELP)]
        offset: Option<usize>,
    },
    /// Disassemble every event in source order.
    ///
    /// Input: a standalone SEQ or a wrapped BGM PROT entry from
    /// `legaia-extract` (see `seq find` / --offset for wrapped entries).
    Events {
        path: PathBuf,
        /// Cap output to the first N events.
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, value_parser = parse_offset, help = OFFSET_HELP)]
        offset: Option<usize>,
    },
    /// Emit the full parse as JSON.
    ///
    /// Input: a standalone SEQ or a wrapped BGM PROT entry from
    /// `legaia-extract` (see `seq find` / --offset for wrapped entries).
    Json {
        path: PathBuf,
        #[arg(long, value_parser = parse_offset, help = OFFSET_HELP)]
        offset: Option<usize>,
    },
    /// Scan any blob for `pQES` magics and report each candidate offset
    /// with its header (and whether the full event stream parses).
    ///
    /// Use this on wrapped BGM PROT entries from `legaia-extract`
    /// (e.g. extracted/PROT/0990_music_01.BIN) to find the value to pass
    /// as `--offset` to info/events/json.
    Find { path: PathBuf },
}

/// Accept `0x1F2A` / `0X1F2A` hex or plain decimal.
fn parse_offset(s: &str) -> std::result::Result<usize, String> {
    let t = s.trim();
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        usize::from_str_radix(h, 16).map_err(|e| e.to_string())
    } else {
        t.parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())
    }
}

/// Rust ignores SIGPIPE by default; restore SIG_DFL so `seq json f | head`
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
        Cmd::Info { path, offset } => cmd_info(&path, offset),
        Cmd::Events {
            path,
            limit,
            offset,
        } => cmd_events(&path, limit, offset),
        Cmd::Json { path, offset } => cmd_json(&path, offset),
        Cmd::Find { path } => cmd_find(&path),
    }
}

/// Every byte offset at which the `pQES` magic occurs in `bytes`.
fn magic_offsets(bytes: &[u8]) -> Vec<usize> {
    if bytes.len() < SEQ_MAGIC.len() {
        return Vec::new();
    }
    bytes
        .windows(SEQ_MAGIC.len())
        .enumerate()
        .filter_map(|(i, w)| (w == SEQ_MAGIC).then_some(i))
        .collect()
}

/// Read `path` and parse the SEQ at `offset` (or offset 0, falling back to
/// an auto-scan for the first parseable `pQES` magic in the blob). Returns
/// the parsed SEQ, the effective offset, and the total file size.
fn load_seq(path: &Path, offset: Option<usize>) -> Result<(Seq, usize, usize)> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let size = bytes.len();
    if let Some(off) = offset {
        if off >= bytes.len() {
            bail!(
                "--offset 0x{off:X} is past the end of {} ({} bytes)",
                path.display(),
                bytes.len()
            );
        }
        let seq = Seq::parse(&bytes[off..])
            .with_context(|| format!("parsing SEQ at offset 0x{off:X} of {}", path.display()))?;
        return Ok((seq, off, size));
    }
    match Seq::parse(&bytes) {
        Ok(seq) => Ok((seq, 0, size)),
        Err(first_err) => {
            // Auto-scan fallback: retail BGM wraps the SEQ at a non-zero
            // offset; find the first pQES that parses cleanly.
            for off in magic_offsets(&bytes) {
                if off == 0 {
                    continue;
                }
                if let Ok(seq) = Seq::parse(&bytes[off..]) {
                    eprintln!(
                        "note: no SEQ at offset 0 of {}; auto-detected pQES at offset 0x{off:X} \
                         (pass --offset 0x{off:X} to silence this, or run `seq find` to list all)",
                        path.display()
                    );
                    return Ok((seq, off, size));
                }
            }
            Err(first_err.context(format!(
                "no parseable SEQ found in {} (at offset 0 or any pQES magic); \
                 run `seq find` to inspect candidates",
                path.display()
            )))
        }
    }
}

fn cmd_find(path: &Path) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let hits = magic_offsets(&bytes);
    if hits.is_empty() {
        bail!(
            "no pQES magic found in {} ({} bytes) - not SEQ-bearing data?",
            path.display(),
            bytes.len()
        );
    }
    println!("file:  {} ({} bytes)", path.display(), bytes.len());
    println!("found {} pQES magic(s):", hits.len());
    for off in hits {
        match parse_header(&bytes[off..]) {
            Ok(h) => {
                let full = match Seq::parse(&bytes[off..]) {
                    Ok(seq) => format!("parses ok ({} events)", seq.events.len()),
                    Err(e) => format!("header ok, event stream fails: {e}"),
                };
                println!(
                    "  0x{off:08X}  v{}  ppqn={}  tempo={}us/qn ({:.1} BPM)  {}",
                    h.version,
                    h.ppqn,
                    h.tempo_us_per_qn,
                    h.bpm(),
                    full
                );
            }
            Err(e) => println!("  0x{off:08X}  header fails: {e}"),
        }
    }
    Ok(())
}

fn cmd_info(path: &Path, offset: Option<usize>) -> Result<()> {
    let (seq, off, size) = load_seq(path, offset)?;
    let s = seq.event_summary();
    println!("file:       {}", path.display());
    println!("size:       {} bytes", size);
    println!("offset:     0x{:X}", off);
    println!("version:    {}", seq.header.version);
    println!("ppqn:       {}", seq.header.ppqn);
    println!(
        "tempo:      {} us/qn  ({:.1} BPM)",
        seq.header.tempo_us_per_qn,
        seq.header.bpm()
    );
    println!(
        "time-sig:   {}/{}",
        seq.header.time_sig_num,
        1u32 << seq.header.time_sig_denom_pow2
    );
    println!(
        "events:     {}  (total ticks {})",
        seq.events.len(),
        seq.total_ticks()
    );
    println!("  note_on:        {}", s.note_on);
    println!("  note_off:       {}", s.note_off);
    println!("  program_change: {}", s.program_change);
    println!("  control_change: {}", s.control_change);
    println!("  pitch_bend:     {}", s.pitch_bend);
    println!("  chan_aftertouch:{}", s.channel_aftertouch);
    println!("  poly_aftertouch:{}", s.poly_aftertouch);
    println!("  set_tempo:      {}", s.set_tempo);
    println!("  end_of_track:   {}", s.end_of_track);
    println!("  other_meta:     {}", s.other_meta);
    Ok(())
}

fn cmd_events(path: &Path, limit: Option<usize>, offset: Option<usize>) -> Result<()> {
    let (seq, _off, _size) = load_seq(path, offset)?;
    let mut tick: u64 = 0;
    let cap = limit.unwrap_or(usize::MAX);
    for (i, ev) in seq.events.iter().enumerate() {
        if i >= cap {
            println!("(truncated at {} events)", cap);
            break;
        }
        tick += ev.delta as u64;
        match &ev.body {
            EventBody::Channel { channel, message } => {
                println!(
                    "{:>10}  +{:<5}  ch{:<2}  {}",
                    tick,
                    ev.delta,
                    channel,
                    fmt_channel(message)
                );
            }
            EventBody::Meta(m) => {
                println!("{:>10}  +{:<5}  meta  {}", tick, ev.delta, fmt_meta(m));
            }
        }
    }
    Ok(())
}

fn cmd_json(path: &Path, offset: Option<usize>) -> Result<()> {
    let (seq, _off, _size) = load_seq(path, offset)?;
    let s = serde_json::to_string_pretty(&seq)?;
    println!("{}", s);
    Ok(())
}

fn fmt_channel(m: &ChannelMessage) -> String {
    match m {
        ChannelMessage::NoteOn { key, velocity } => {
            if *velocity == 0 {
                format!("note-off (running)  key={:<3}", key)
            } else {
                format!("note-on             key={:<3}  vel={}", key, velocity)
            }
        }
        ChannelMessage::NoteOff { key, velocity } => {
            format!("note-off            key={:<3}  vel={}", key, velocity)
        }
        ChannelMessage::ProgramChange { program } => {
            format!("program-change      prog={}", program)
        }
        ChannelMessage::ControlChange { control, value } => {
            format!("control-change      ctrl={:<3}  val={}", control, value)
        }
        ChannelMessage::PitchBend { value } => format!("pitch-bend          val={}", value),
        ChannelMessage::PolyAftertouch { key, value } => {
            format!("poly-aftertouch     key={:<3}  val={}", key, value)
        }
        ChannelMessage::ChannelAftertouch { value } => format!("ch-aftertouch       val={}", value),
    }
}

fn fmt_meta(m: &MetaMessage) -> String {
    match m {
        MetaMessage::EndOfTrack => "end-of-track".into(),
        MetaMessage::SetTempo { us_per_qn } => {
            format!(
                "tempo               us/qn={}  ({:.1} BPM)",
                us_per_qn,
                60_000_000.0 / *us_per_qn as f32
            )
        }
        MetaMessage::TimeSignature {
            numerator,
            denominator_pow2,
            ..
        } => format!(
            "time-sig            {}/{}",
            numerator,
            1u32 << denominator_pow2
        ),
        MetaMessage::KeySignature { sharps, minor } => {
            format!("key-sig             sharps={}  minor={}", sharps, minor)
        }
        MetaMessage::Other { kind, data } => {
            format!("other(kind={:#x})    bytes={}", kind, data.len())
        }
    }
}
