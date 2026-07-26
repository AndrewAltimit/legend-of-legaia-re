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
use legaia_engine_audio::seq_calc::flag;
use legaia_engine_audio::sequencer::Sequencer;
use legaia_engine_audio::spu::ram::SpuAllocator;
use legaia_engine_audio::{
    PumpOutcome, SeqCalcState, SeqCall, SeqChannel, SeqEvent, SlideDir, Spu, VabBank,
    pump_delta_time, read_delta, seq_calc, start_channel, stop_channel, tempo_slide_tick,
    tick_budget, volume_slide_tick,
};
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
    /// Trace the **retail** `SsSeqCalc` tier over the same track's SEQ body
    /// instead of the engine sequencer: one line per frame of dispatch, then
    /// the decoded events. This is the differential reference for
    /// `sequencer.rs` - see `legaia_engine_audio::seq_calc` /
    /// `legaia_engine_audio::seq_events`.
    #[arg(long)]
    seq_calc: bool,
    /// The runtime word at `0x801CD2BC` that `SsSeqCalc`'s tick-budget divide
    /// uses. `60` is the frame-rate reading of `(res * tempo * 10) / (d * 60)`
    /// and reproduces `ticks_per_second / 60`; it is an inference from the
    /// arithmetic, not a captured value, which is why it is a knob.
    #[arg(long, default_value_t = 60)]
    divisor: u32,
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

/// Drive the retail `SsSeqCalc` tier over one track's SEQ body.
///
/// This is the host for [`legaia_engine_audio::seq_calc`] +
/// [`legaia_engine_audio::seq_events`]: it seeds one `(slot, channel)` record
/// from the SEQ header, then runs `seq_calc` once per retail frame with the
/// dispatch table's arms bound to the ported kernels. The engine's own
/// `Sequencer` is not involved - the point is to see what retail's transport
/// makes of the same bytes.
fn seq_calc_trace(track: &Track, frames: u64, divisor: u32) -> Result<()> {
    let raw = &track.bytes[track.seq_off..];
    let (header, header_len) =
        legaia_seq::parse_header_with_len(raw).context("parse SEQ header")?;
    let body = &raw[header_len..];

    // The meta handler's own divide (`FUN_80061954` at `800619d4`) traps on a
    // zero divisor; here it just yields a zero tempo.
    let tempo_bpm = 60_000_000u32
        .checked_div(header.tempo_us_per_qn)
        .unwrap_or(0);
    let resolution = header.ppqn as i16;
    let mut channel = SeqChannel {
        resolution,
        tempo: tempo_bpm,
        tick_budget: tick_budget(resolution, tempo_bpm, divisor),
        // Negative is the pump's ordinary "spend the whole budget" mode.
        sub_frame: -1,
        flags: flag::PLAY,
        playing: 1,
        chain_slot: 0xFF,
        vol: (0x7F, 0x7F),
        ..Default::default()
    };
    // The SEQ open (`FUN_80062410` at `80062620`) reads the body's leading
    // delta-time before the first `SsSeqCalc` frame. Without that the pump
    // decodes it as a status byte.
    channel.pending_wait = read_delta(&mut channel, body).unwrap_or(0);
    channel.start = channel.cursor;
    channel.loop_cursor = channel.cursor;
    println!(
        "seq-calc  prot_entry {}  body {} bytes  ppqn {}  tempo {} bpm  \
tick_budget {} (tenths/frame, divisor {})",
        track.entry,
        body.len(),
        header.ppqn,
        tempo_bpm,
        channel.tick_budget,
        divisor
    );

    let mut state = SeqCalcState {
        busy: false,
        slot_mask: 1,
        slot_count: 1,
        channel_count: 1,
    };
    let mut channels = vec![vec![channel]];
    let (mut notes, mut ccs, mut pcs, mut bends, mut metas, mut loops, mut unknown) =
        (0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let mut printed = 0u64;

    for frame in 0..frames {
        let mut line: Vec<String> = Vec::new();
        seq_calc(&mut state, &mut channels, |call, ch| match call {
            SeqCall::Pump => match pump_delta_time(ch, body) {
                PumpOutcome::Ran(events) | PumpOutcome::Runaway(events) => {
                    for ev in events {
                        match ev {
                            SeqEvent::Note {
                                note,
                                velocity,
                                delta,
                            } => {
                                notes += 1;
                                line.push(format!("note {note:#04x} v{velocity:#04x} d{delta}"));
                            }
                            SeqEvent::ControlChange(v) => {
                                ccs += 1;
                                line.push(format!("cc {v:#04x}"));
                            }
                            SeqEvent::ProgramChange(v) => {
                                pcs += 1;
                                line.push(format!("pc {v:#04x}"));
                            }
                            SeqEvent::PitchBend => {
                                bends += 1;
                                line.push("bend".into());
                            }
                            SeqEvent::Meta(k) => {
                                metas += 1;
                                line.push(format!("meta {k:#04x}"));
                            }
                            SeqEvent::EndOfTrack(end) => {
                                loops += 1;
                                line.push(format!("eot {end:?}"));
                            }
                            SeqEvent::LoopMarker(end) => {
                                loops += 1;
                                line.push(format!("loop-marker {end:?}"));
                            }
                            SeqEvent::Unknown(s) => {
                                unknown += 1;
                                line.push(format!("unknown {s:#04x}"));
                            }
                            SeqEvent::Overrun => line.push("overrun".into()),
                        }
                    }
                }
                PumpOutcome::Divided | PumpOutcome::Waited | PumpOutcome::Idle => {}
            },
            SeqCall::VolUp | SeqCall::VolDown => {
                let dir = if call == SeqCall::VolUp {
                    SlideDir::Up
                } else {
                    SlideDir::Down
                };
                let vol = ch.vol;
                let tick = volume_slide_tick(ch, dir, vol);
                if let Some(v) = tick.commit {
                    ch.vol = v;
                    line.push(format!("vol {dir:?} -> {v:?}"));
                }
            }
            SeqCall::Tempo => {
                let t = tempo_slide_tick(ch, divisor);
                line.push(format!("tempo -> {} ({t:?})", ch.tempo));
            }
            SeqCall::Stop => {
                stop_channel(ch);
                line.push("stop".into());
            }
            SeqCall::Start => {
                start_channel(ch);
                line.push("start".into());
            }
            SeqCall::Rewind => line.push("rewind".into()),
        });
        if !line.is_empty() {
            println!("f{frame:<6} {}", line.join("  "));
            printed += 1;
        }
    }

    channel = channels[0][0];
    println!(
        "-- {printed} active frames of {frames}; notes {notes}  cc {ccs}  pc {pcs}  \
bend {bends}  meta {metas}  track-end {loops}  unknown {unknown}"
    );
    println!(
        "-- final cursor {} of {}  tick_accum {}  pending_wait {}  flags {:#x}",
        channel.cursor,
        body.len(),
        channel.tick_accum,
        channel.pending_wait,
        channel.flags
    );
    Ok(())
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

    if args.seq_calc {
        return seq_calc_trace(track, args.frames, args.divisor);
    }

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
