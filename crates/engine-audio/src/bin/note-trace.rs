//! Emit a note-level trace of a BGM track through the engine's own SEQ
//! sequencer + VAB bank + SPU.
//!
//! The engine half of the note-level BGM differential. The recomp side is
//! `scripts/recomp/audio_note_capture.py`; the two emit the same canonical
//! JSONL and are compared by `scripts/recomp/note_diff.py`.
//!
//! Both sides record at the same layer - the instant a voice is keyed on,
//! snapshotting the programmed voice state (ADPCM start address, pitch,
//! per-voice volumes, raw ADSR words) - so a divergence localises directly:
//! a missing note-on means the sequencer never asked for it, a wrong `addr`
//! means tone selection diverged, a wrong `pitch` means the note or bend
//! resolved differently.
//!
//! Addresses are *not* comparable raw across sides: each allocator lays the
//! VAGs out in SPU RAM itself. `note_diff.py` normalises them to dense VAG
//! ids by ascending address, which both sides assign in bank upload order.
//!
//! Disc data is required (`extracted/PROT.DAT` + `CDNAME.TXT`); nothing this
//! writes contains Sony bytes beyond the trace itself, which is a capture
//! artifact and must stay out of git.
//!
//! Usage:
//!
//! ```text
//! note-trace --extracted extracted --track 0 --frames 1800 \
//!     --out /tmp/scratch/engine_notes.jsonl
//! note-trace --extracted extracted --list
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Parser;

use legaia_engine_audio::note_trace::{NoteTrace, SAMPLES_PER_FRAME};
use legaia_engine_audio::sequencer::Sequencer;
use legaia_engine_audio::spu::ram::SpuAllocator;
use legaia_engine_audio::{Spu, VabBank};
use legaia_prot::archive::Archive;
use legaia_prot::cdname;
use legaia_seq::Seq;
use legaia_vab::parse as parse_vab;

#[derive(Parser, Debug)]
#[command(about = "Note-level BGM trace through the engine sequencer")]
struct Args {
    /// Directory holding PROT.DAT + CDNAME.TXT.
    #[arg(long, default_value = "extracted")]
    extracted: PathBuf,
    /// Index of the VAB+SEQ pair within the `music_01` block, in block order.
    #[arg(long, default_value_t = 0)]
    track: usize,
    /// How many retail frames (60 Hz) of the track to play out.
    #[arg(long, default_value_t = 1800)]
    frames: u64,
    /// List the playable tracks and exit.
    #[arg(long)]
    list: bool,
    /// Output JSONL path ("-" = stdout).
    #[arg(long, default_value = "-")]
    out: String,
    /// Print a summary to stderr.
    #[arg(long)]
    summary: bool,
}

/// A `music_01` entry that carries both a VAB and a SEQ.
struct Track {
    entry: u32,
    bytes: Vec<u8>,
    vab_off: usize,
    seq_off: usize,
}

fn find_tracks(extracted: &Path) -> Result<Vec<Track>> {
    let prot = extracted.join("PROT.DAT");
    let cdn = extracted.join("CDNAME.TXT");
    if !prot.exists() || !cdn.exists() {
        bail!(
            "need PROT.DAT and CDNAME.TXT under {} - run legaia-extract first",
            extracted.display()
        );
    }
    let mut archive = Archive::open(&prot).context("open PROT.DAT")?;
    let map = cdname::parse(&cdn).context("parse CDNAME.TXT")?;
    // Extraction frame, not the raw #define window: CDNAME numbers are
    // in-RAM TOC indices and every extraction filename is shifted by +2.
    let (start, end) = cdname::block_range_for_name_extraction(&map, "music_01")
        .context("music_01 block missing from CDNAME")?;

    let mut out = Vec::new();
    for idx in start..end {
        let entry = archive.entries[idx as usize].clone();
        let mut bytes = Vec::new();
        if archive.read_entry(&entry, &mut bytes).is_err() {
            continue;
        }
        let vab_off = bytes.windows(4).position(|w| w == b"pBAV");
        let seq_off = bytes.windows(4).position(|w| w == b"pQES");
        if let (Some(v), Some(s)) = (vab_off, seq_off) {
            out.push(Track {
                entry: idx,
                bytes,
                vab_off: v,
                seq_off: s,
            });
        }
    }
    Ok(out)
}

fn main() -> Result<()> {
    let args = Args::parse();
    let tracks = find_tracks(&args.extracted)?;
    if tracks.is_empty() {
        bail!("no VAB+SEQ pairs found in the music_01 block");
    }

    if args.list {
        for (i, t) in tracks.iter().enumerate() {
            let seq = Seq::parse(&t.bytes[t.seq_off..]).ok();
            let (ppqn, tempo, events) = seq
                .map(|s| (s.header.ppqn, s.header.tempo_us_per_qn, s.events.len()))
                .unwrap_or((0, 0, 0));
            println!(
                "track {i:3}  prot_entry {:4}  ppqn {ppqn}  tempo_us {tempo}  events {events}",
                t.entry
            );
        }
        return Ok(());
    }

    let track = tracks
        .get(args.track)
        .with_context(|| format!("track {} out of range (have {})", args.track, tracks.len()))?;

    let vab = parse_vab(&track.bytes, track.vab_off).context("parse VAB")?;
    let seq = Seq::parse(&track.bytes[track.seq_off..]).context("parse SEQ")?;

    let mut spu = Spu::new();
    spu.note_trace = Some(NoteTrace::new());
    // Start above the SPU's reverb work area, as the engine's own bank
    // uploads do.
    let mut alloc = SpuAllocator::new(0x1000, 512 * 1024 - 0x1000);
    let bank = VabBank::upload(&mut spu, &mut alloc, &vab, &track.bytes);

    let mut seqr = Sequencer::new(seq.clone(), bank);
    // Clock sample-by-sample so each note's stamp is exact; frames are
    // derived from the sample clock at the hardware ratio.
    //
    // `Spu::tick` is not optional here even though the rendered samples are
    // discarded. It is what advances each voice's ADSR, and a voice only
    // becomes reusable once its envelope reaches `Phase::Off`. Ticking the
    // sequencer alone leaves every voice permanently busy, so the allocator
    // never takes its "first idle voice wins" path and spreads notes evenly
    // over all 24 voices - a pure harness artifact that looks exactly like a
    // voice-allocation bug. (Same failure mode as capturing from a recomp
    // instance whose SPU is not being clocked; see
    // `scripts/recomp/audio_note_capture.py`.)
    for _ in 0..args.frames * SAMPLES_PER_FRAME {
        if let Some(t) = spu.note_trace.as_mut() {
            t.advance(1);
        }
        seqr.tick_sample(&mut spu);
        let _ = spu.tick();
    }

    let trace = spu.note_trace.take().unwrap_or_default();
    let ons = trace.note_ons().count();
    let header = format!(
        "{{\"kind\":\"header\",\"source\":\"engine\",\"track\":{},\"prot_entry\":{},\
\"ppqn\":{},\"tempo_us\":{},\"seq_events\":{},\"frames\":{},\"note_ons\":{}}}",
        args.track,
        track.entry,
        seq.header.ppqn,
        seq.header.tempo_us_per_qn,
        seq.events.len(),
        args.frames,
        ons
    );
    let jsonl = trace.to_jsonl(&header);
    if args.out == "-" {
        print!("{jsonl}");
    } else {
        std::fs::write(&args.out, jsonl).with_context(|| format!("write {}", args.out))?;
    }

    if args.summary {
        let mut per_voice = [0usize; 24];
        let mut addrs = std::collections::BTreeMap::new();
        for e in trace.note_ons() {
            per_voice[e.voice as usize] += 1;
            *addrs.entry(e.voice_state.addr).or_insert(0usize) += 1;
        }
        eprintln!(
            "note-ons {ons}  distinct VAGs {}  voices used {}",
            addrs.len(),
            per_voice.iter().filter(|n| **n > 0).count()
        );
        eprintln!("  loop_count {}", seqr.loop_count());
    }
    Ok(())
}
