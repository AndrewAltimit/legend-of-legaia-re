//! `seq` — inspector for PsyQ SEQ files.
//!
//! Three subcommands:
//!
//! - `info <PATH>` — print header (version, PPQN, tempo, time signature, BPM).
//! - `events <PATH>` — disassemble every event with absolute tick + delta.
//! - `json <PATH>` — full SEQ as machine-readable JSON.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use legaia_seq::{ChannelMessage, EventBody, MetaMessage, Seq};

#[derive(Parser)]
#[command(name = "seq", about = "Inspect PsyQ SEQ files")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print header + summary (event counts).
    Info { path: PathBuf },
    /// Disassemble every event in source order.
    Events {
        path: PathBuf,
        /// Cap output to the first N events.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Emit the full parse as JSON.
    Json { path: PathBuf },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Info { path } => cmd_info(&path),
        Cmd::Events { path, limit } => cmd_events(&path, limit),
        Cmd::Json { path } => cmd_json(&path),
    }
}

fn cmd_info(path: &PathBuf) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let seq = Seq::parse(&bytes).context("parsing SEQ")?;
    let s = seq.event_summary();
    println!("file:       {}", path.display());
    println!("size:       {} bytes", bytes.len());
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
    println!("  set_tempo:      {}", s.set_tempo);
    println!("  end_of_track:   {}", s.end_of_track);
    println!("  other_meta:     {}", s.other_meta);
    Ok(())
}

fn cmd_events(path: &PathBuf, limit: Option<usize>) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let seq = Seq::parse(&bytes).context("parsing SEQ")?;
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

fn cmd_json(path: &PathBuf) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let seq = Seq::parse(&bytes).context("parsing SEQ")?;
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
