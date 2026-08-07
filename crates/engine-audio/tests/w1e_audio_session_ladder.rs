//! Disc-gated **audio session ladder**: run a mixer-attached frame loop over
//! real disc BGM and real disc SFX and assert at the emitted PCM.
//!
//! # Why this exists
//!
//! Every audio kernel below the sequencer runs only when something is pulling
//! frames: `VabBank::upload` needs an SPU to upload into, `Sequencer`'s voice
//! allocator only runs when a note is due, and the SFX scheduler only fires
//! when a frame tick matures a cue. The one handle that pulls frames -
//! [`legaia_engine_audio::AudioOut`] - owns a `cpal::Stream`, so a headless
//! test could not drive any of it. [`TestAudioSink`] is the device-free stand-in
//! (same [`legaia_engine_audio`] mixing core, pulled by the caller), and this
//! ladder is its session: a 60 Hz frame loop that stages banks, starts a track,
//! pauses and resumes it, and fires cues, exactly as a host's frame loop does.
//!
//! # The measurement rule this file follows
//!
//! **Assert at the output, never at the call.** A wired kernel that runs and
//! produces nothing is indistinguishable from one that is not wired - the
//! failure mode the footstep cadence hit when it was fed the wrong quantity and
//! fired zero times while its unit tests stayed green. So every stage here ends
//! in a [`SinkMeasure`] over the PCM the sink emitted, not in a count of calls
//! made.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::{Path, PathBuf};

use legaia_engine_audio::seq_calc::{
    SeqCalcState, SeqCall, SeqChannel, SlideDir, flag, seq_calc, tempo_slide_tick, tick_budget,
    volume_slide_tick,
};
use legaia_engine_audio::seq_events::{
    PumpOutcome, SeqEvent, pump_delta_time, read_delta, start_channel, stop_channel,
};
use legaia_engine_audio::seq_slots::{SeqResourceSlot, SeqResourceTable};
use legaia_engine_audio::spu::ram::{SPU_RAM_BYTES, SpuAllocator};
use legaia_engine_audio::{
    CueDispatch, PendingCue, SPU_INTERNAL_RATE, Sequencer, SfxBank, SfxScheduler, SinkMeasure, Spu,
    TestAudioSink, VabBank, classify_cue,
};
use legaia_prot::archive::Archive;
use legaia_prot::cdname;
use legaia_seq::Seq;

/// SPU RAM the BGM bank uploads into (the native boot's reserved floor).
const BGM_SPU_BASE: u32 = 0x1000;
/// SPU RAM reserved at the top for the resident SFX banks, matching the
/// native boot's `SFX_BANK_SPU_BYTES` split - the two regions must not
/// overlap or a cue's sample would be overwritten by the score's.
const SFX_SPU_BYTES: u32 = 0x2_0000;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn disc_gate() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let Some(d) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return None;
    };
    Some(d)
}

/// One `music_01` entry that carries both a VAB and a SEQ - the shape every
/// global BGM track has (`[chunk][pBAV][chunk][pQES]`).
struct BgmEntry {
    index: u32,
    bytes: Vec<u8>,
    vab_off: usize,
    seq_off: usize,
}

fn first_bgm_entry(archive: &mut Archive, extracted: &Path) -> Option<BgmEntry> {
    let map = cdname::parse(&extracted.join("CDNAME.TXT")).ok()?;
    let (start, end) = cdname::block_range_for_name_extraction(&map, "music_01")?;
    for idx in start..end {
        let entry = archive.entries.get(idx as usize)?.clone();
        let mut bytes = Vec::new();
        if archive.read_entry(&entry, &mut bytes).is_err() {
            continue;
        }
        let vab_at = bytes.windows(4).position(|w| w == b"pBAV");
        let seq_at = bytes.windows(4).position(|w| w == b"pQES");
        if let (Some(v), Some(s)) = (vab_at, seq_at) {
            return Some(BgmEntry {
                index: idx,
                bytes,
                vab_off: v,
                seq_off: s,
            });
        }
    }
    None
}

/// Stage a real BGM track onto `sink`: upload its VAB into the sink's SPU and
/// attach a sequencer over its SEQ. Returns the uploaded bank.
fn stage_bgm(sink: &mut TestAudioSink, entry: &BgmEntry) -> VabBank {
    let report = legaia_vab::parse(&entry.bytes, entry.vab_off).expect("parse real VAB");
    let mut alloc = SpuAllocator::new(BGM_SPU_BASE, SPU_RAM_BYTES as u32 - SFX_SPU_BYTES);
    let bank = sink.with_spu(|spu: &mut Spu| {
        VabBank::upload(spu, &mut alloc, &report, &entry.bytes[entry.vab_off..])
    });
    assert!(
        !bank.programs.is_empty(),
        "real VAB must expand to at least one program-space slot"
    );
    let seq = Seq::parse(&entry.bytes[entry.seq_off..]).expect("parse real SEQ");
    let mut sequencer = Sequencer::new(seq, bank.clone());
    sequencer.set_loop_to(0);
    sink.attach_sequencer(sequencer);
    bank
}

/// Render `frames` video frames, folding the per-frame output into one
/// measure and reporting the first frame whose output was audible.
fn run_frames(sink: &mut TestAudioSink, frames: usize) -> (SinkMeasure, Option<usize>) {
    let mut total = SinkMeasure::default();
    let mut first_audible = None;
    for f in 0..frames {
        let m = sink.render_video_frame();
        if first_audible.is_none() && !m.is_silent() {
            first_audible = Some(f);
        }
        total.merge(m);
    }
    (total, first_audible)
}

/// A real global BGM track, played through the device-free sink, must reach
/// the **output** - not merely tick.
///
/// This is the ladder rung that drives `Sequencer`'s voice allocator and the
/// key-on volume/pan chain: neither runs until a note is due, and neither is
/// observable except as samples.
#[test]
fn a_real_bgm_track_sounds_through_the_device_free_sink() {
    let Some(extracted) = disc_gate() else { return };
    let mut archive = Archive::open(&extracted.join("PROT.DAT")).expect("open PROT");
    let Some(entry) = first_bgm_entry(&mut archive, &extracted) else {
        eprintln!("[skip] no music_01 entry carries both pBAV and pQES");
        return;
    };

    let mut sink = TestAudioSink::new(SPU_INTERNAL_RATE);
    stage_bgm(&mut sink, &entry);

    // Two seconds of session at 60 Hz. Retail intros are not all loud on
    // frame 0, so the window has to be long enough for the first note to be
    // due at the SEQ's own tempo rather than at the harness's convenience.
    let (total, first_audible) = run_frames(&mut sink, 120);
    eprintln!(
        "[w1e-bgm] entry={} frames={} peak={} nonzero={} mean_abs={:.1} first_audible={:?}",
        entry.index,
        total.frames,
        total.peak,
        total.nonzero,
        total.mean_abs(),
        first_audible
    );

    assert!(
        !total.is_silent(),
        "a real track attached to the sink emitted pure silence over 2 s - the \
         sequencer ticked but nothing reached the output"
    );
    assert!(
        total.peak > 64,
        "output never rose above the noise floor (peak {}), which is what a \
         key-on that resolves no sample looks like",
        total.peak
    );
    // A sounding score holds level; a single click does not. `nonzero` over a
    // 2 s window separates them.
    assert!(
        total.nonzero * 4 > total.frames,
        "only {}/{} frames carried signal - that is a click, not a track",
        total.nonzero,
        total.frames
    );

    let progress = sink
        .sequencer_progress()
        .expect("a sequencer is attached to the sink");
    assert!(
        progress.tick > 0,
        "the sequencer playhead never advanced under the sink's sample clock"
    );
}

/// Pausing the BGM freezes the sequencer clock without tearing anything down,
/// and resuming continues from where it stopped.
///
/// This is the pair the scene-transition plumbing drives (field-VM op `0x35`
/// sub-ops 2 and 3): a director that pauses by detaching would restart the
/// track's intro on resume.
#[test]
fn pause_freezes_the_playhead_and_resume_continues_it() {
    let Some(extracted) = disc_gate() else { return };
    let mut archive = Archive::open(&extracted.join("PROT.DAT")).expect("open PROT");
    let Some(entry) = first_bgm_entry(&mut archive, &extracted) else {
        eprintln!("[skip] no music_01 entry carries both pBAV and pQES");
        return;
    };

    let mut sink = TestAudioSink::new(SPU_INTERNAL_RATE);
    stage_bgm(&mut sink, &entry);
    run_frames(&mut sink, 60);
    let running = sink.sequencer_progress().expect("attached").tick;
    assert!(running > 0, "playhead must be moving before the pause");

    // `playhead_ticks` is *event-quantised* - it advances by the delta of each
    // event as it fires, so a real track sits on one value across any gap
    // longer than the window. That makes "unchanged over N frames" a valid
    // assertion only in the paused direction; the resumed direction has to
    // poll until the next event is due.
    sink.set_sequencer_paused(true);
    run_frames(&mut sink, 120);
    let paused = sink.sequencer_progress().expect("still attached").tick;
    assert_eq!(
        paused, running,
        "a paused sequencer must not advance its playhead"
    );
    assert!(
        sink.sequencer_paused(),
        "the pause latch must survive the frames it gates"
    );

    sink.set_sequencer_paused(false);
    let mut resumed = paused;
    let mut waited = 0usize;
    while resumed == paused && waited < 300 {
        run_frames(&mut sink, 30);
        waited += 30;
        resumed = sink.sequencer_progress().expect("still attached").tick;
    }
    let p = sink.sequencer_progress().expect("still attached");
    eprintln!(
        "[w1e-pause] running={running} paused={paused} resumed={resumed} \
         waited_frames={waited} finished={} bpm={:.1}",
        p.finished, p.bpm
    );
    assert!(
        resumed != paused,
        "the playhead never moved again in 5 s after resume ({paused}); the \
         pause latch gated the clock permanently instead of gating it"
    );
    assert!(
        !p.finished,
        "the track ran off its end during the resume window, so this says \
         nothing about resume"
    );
}

/// The master mute gate zeroes the output while everything behind it keeps
/// running - the property the engine's volume slider depends on (unmuting
/// resumes in sync rather than replaying).
#[test]
fn mute_silences_the_output_while_the_session_keeps_running() {
    let Some(extracted) = disc_gate() else { return };
    let mut archive = Archive::open(&extracted.join("PROT.DAT")).expect("open PROT");
    let Some(entry) = first_bgm_entry(&mut archive, &extracted) else {
        eprintln!("[skip] no music_01 entry carries both pBAV and pQES");
        return;
    };

    let mut sink = TestAudioSink::new(SPU_INTERNAL_RATE);
    stage_bgm(&mut sink, &entry);
    let (audible, _) = run_frames(&mut sink, 120);
    if audible.is_silent() {
        eprintln!("[skip] track produced no signal to mute in the first 2 s");
        return;
    }

    let before = sink.sequencer_progress().expect("attached").tick;
    sink.set_muted(true);
    let (muted, _) = run_frames(&mut sink, 30);
    let after = sink.sequencer_progress().expect("attached").tick;

    assert!(muted.is_silent(), "a muted sink must emit exact zeros");
    assert!(
        after > before,
        "mute must gate the output only - the sequencer clock kept running \
         ({before} -> {after})"
    );
}

/// A cue enqueued with a delay fires on the frame the delay names, and the
/// **output** is silent until it does.
///
/// The scheduler is the retail SFX ring's port: the delay is in frames, and
/// `delay = d` means "d ticks later". Reading it as "fire immediately and let
/// the mixer sort it out" is silent in a unit test that only counts fires.
#[test]
fn a_delayed_sfx_cue_reaches_the_output_on_its_own_frame() {
    let Some(extracted) = disc_gate() else { return };
    let scus = match std::fs::read(extracted.join("SCUS_942.54")) {
        Ok(b) => b,
        Err(_) => {
            eprintln!("[skip] extracted/SCUS_942.54 missing");
            return;
        }
    };
    let Some(table) = legaia_asset::sfx_table::SfxTable::from_scus(&scus) else {
        eprintln!("[skip] SFX descriptor table did not decode from SCUS");
        return;
    };
    let bank = SfxBank::from_descriptors(
        table
            .active()
            .map(|(id, d)| (id, d.program, d.tone, d.note, d.voice_count())),
    );
    assert!(
        !bank.is_empty(),
        "the disc descriptor table must yield at least one active cue"
    );

    let mut archive = Archive::open(&extracted.join("PROT.DAT")).expect("open PROT");
    // The class-2 sound bank - the slot the battle / duel cues key and the
    // fallback every unrouted cue resolves to.
    let (_slot, prot) = legaia_asset::sfx_table::PINNED_SLOT_BANKS
        .iter()
        .copied()
        .find(|(s, _)| *s == legaia_asset::sfx_table::FALLBACK_VAB_SLOT)
        .expect("the fallback slot is pinned");
    let Some(entry) = archive.entries.get(prot as usize).cloned() else {
        eprintln!("[skip] PROT {prot} absent from this archive");
        return;
    };
    let mut bank_bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bank_bytes)
        .expect("read SFX bank entry");
    let Some((report, vab_off)) = [4usize, 0]
        .into_iter()
        .find_map(|o| legaia_vab::parse(&bank_bytes, o).ok().map(|r| (r, o)))
    else {
        eprintln!("[skip] no VAB header at +4 or +0 in PROT {prot}");
        return;
    };

    let mut sink = TestAudioSink::new(SPU_INTERNAL_RATE);
    let mut alloc = SpuAllocator::new(SPU_RAM_BYTES as u32 - SFX_SPU_BYTES, SFX_SPU_BYTES);
    let vab = sink.with_spu(|spu: &mut Spu| {
        VabBank::upload(spu, &mut alloc, &report, &bank_bytes[vab_off..])
    });

    // Pick a cue this bank can actually sound. Which ids are resident is a
    // property of the disc, not of the port, so the ladder discovers it rather
    // than hardcoding one and going quiet when the pairing moves.
    let mut audible_cue = None;
    for entry in bank.iter() {
        let mut probe = Spu::new();
        // The probe SPU shares the live one's RAM image so an uploaded sample
        // resolves; only the voice state is throwaway.
        probe.ram = sink.with_spu(|spu: &mut Spu| spu.ram.clone());
        if bank.play_one_shot(entry.id, &mut probe, &vab).is_none() {
            continue;
        }
        let mut peak = 0i32;
        for _ in 0..(SPU_INTERNAL_RATE / 4) {
            let (l, r) = probe.tick();
            peak = peak.max((l as i32).abs()).max((r as i32).abs());
        }
        if peak > 64 {
            audible_cue = Some((entry.id, peak));
            break;
        }
    }
    let Some((cue_id, probe_peak)) = audible_cue else {
        eprintln!("[skip] no descriptor in the class-2 bank rendered above the noise floor");
        return;
    };
    eprintln!("[w1e-sfx] audible cue id=0x{cue_id:02X} probe_peak={probe_peak}");

    // The host front-end classifies the raw id before the ring write. The
    // stored value is **not** the raw id below 0x40: `FUN_8004FCC8`'s low arm
    // computes `a1 = id - 1` (`addiu a1,s0,-0x1` at 0x8004FD9C) and stores
    // *that* (`sh a1,0x0(v0)` at 0x8004FE28), while the 0x40..0x100 arm stores
    // the id itself (`sh s0,0x0(v0)` at 0x8004FDE4). So the raw id that lands
    // descriptor `D` in the ring is `D + 1` in the low band and `D` above it -
    // feeding a descriptor id straight in fires the cue one slot below, which
    // is exactly what this ladder hit on its first real run.
    let raw_id = if (cue_id as u32) + 1 < 0x40 {
        cue_id as u32 + 1
    } else {
        cue_id as u32
    };
    let dispatch = classify_cue(raw_id);
    let CueDispatch::Ring { ring_value, .. } = dispatch else {
        panic!("raw id 0x{raw_id:02X} must classify as a ring cue, got {dispatch:?}");
    };
    assert_eq!(
        ring_value, cue_id as u16,
        "the raw id the front-end takes must land descriptor 0x{cue_id:02X} in \
         the ring"
    );

    const DELAY: u16 = 5;
    let mut sched = SfxScheduler::new();
    sched.enqueue(PendingCue::new(ring_value, DELAY));
    assert_eq!(sched.pending_count(), 1, "the cue is queued, not yet fired");

    let mut fired_on = None;
    let mut first_audible = None;
    for frame in 0..40usize {
        for cue in sched.tick_frame().fired {
            if fired_on.is_none() {
                fired_on = Some(frame);
            }
            let keyed = sink.with_spu(|spu: &mut Spu| bank.play_one_shot(cue.id as u8, spu, &vab));
            eprintln!(
                "[w1e-sfx] frame={frame} fired id=0x{:02X} keyed={keyed:?}",
                cue.id
            );
        }
        let m = sink.render_video_frame();
        if first_audible.is_none() && !m.is_silent() {
            first_audible = Some(frame);
        }
    }

    assert_eq!(
        fired_on,
        Some(DELAY as usize),
        "a cue queued with delay {DELAY} must mature on tick {DELAY}"
    );
    let audible = first_audible.expect(
        "the fired cue never reached the output - a scheduler that fires into \
         a silent SPU is indistinguishable from one that never fired",
    );
    assert!(
        audible >= DELAY as usize,
        "output went audible on frame {audible}, before the cue fired on frame {DELAY}"
    );
    assert!(
        audible <= DELAY as usize + 1,
        "the cue fired on frame {DELAY} but the output stayed silent until \
         frame {audible}"
    );
}

/// The retail `SsSeqCalc` transport tier walks the **same real SEQ** the
/// clean-room parser decodes, and the two agree event-for-event up to the
/// first end-of-track.
///
/// This is the differential the transport rung owes: `pump_delta_time` /
/// `decode_event` / the handler tails consume the stream through the retail
/// dispatch (`seq_calc` -> `dispatch_channel`), and every event class the walk
/// yields is counted against `Seq::event_summary` over the identical bytes.
/// A phantom byte read (the defect the module docs record) or a dropped
/// operand desynchronises the stream and shows up here as an `Unknown` /
/// `Overrun` event or a count mismatch - not as a silent pass.
///
/// The stop / start arms are then driven through the same dispatch table the
/// retail frame walks: raising `STOP` / `START` on the record is exactly what
/// `SsSeqStop` / `SsSeqPlay` do, and the tail dispatch consumes the bit.
#[test]
fn the_retail_transport_tier_agrees_with_the_clean_room_parser() {
    let Some(extracted) = disc_gate() else { return };
    let mut archive = Archive::open(&extracted.join("PROT.DAT")).expect("open PROT");
    let Some(entry) = first_bgm_entry(&mut archive, &extracted) else {
        eprintln!("[skip] no music_01 entry carries both pBAV and pQES");
        return;
    };

    // Clean-room reference over the same bytes.
    let raw = &entry.bytes[entry.seq_off..];
    let seq = Seq::parse(raw).expect("clean-room parse of the real SEQ");
    let summary = seq.event_summary();

    // Retail-record seeding, the SEQ-open shape (`FUN_80062410`): resolution
    // and tempo off the header, the body's leading delta consumed before the
    // first frame, cursor published as the track start.
    let (header, header_len) = legaia_seq::parse_header_with_len(raw).expect("SEQ header");
    let body = &raw[header_len..];
    let bpm = 60_000_000u32
        .checked_div(header.tempo_us_per_qn)
        .unwrap_or(0);
    let resolution = header.ppqn as i16;
    let mut ch = SeqChannel {
        resolution,
        tempo: bpm,
        tick_budget: tick_budget(resolution, bpm, 60),
        sub_frame: -1,
        flags: flag::PLAY,
        playing: 1,
        chain_slot: 0xFF,
        vol: (0x7F, 0x7F),
        ..Default::default()
    };
    ch.pending_wait = read_delta(&mut ch, body).unwrap_or(0);
    ch.start = ch.cursor;
    ch.loop_cursor = ch.cursor;

    let mut state = SeqCalcState {
        busy: false,
        slot_mask: 1,
        slot_count: 1,
        channel_count: 1,
    };
    let mut channels = vec![vec![ch]];
    let mut events: Vec<SeqEvent> = Vec::new();
    let mut frames = 0usize;
    // Bound the walk by wall-time frames, generously: a track's first pass
    // cannot outlast its own total tick count at >= 1 tenth-tick per frame.
    let frame_cap = 200_000usize;
    while !events
        .iter()
        .any(|e| matches!(e, SeqEvent::EndOfTrack(_) | SeqEvent::LoopMarker(_)))
        && frames < frame_cap
    {
        seq_calc(&mut state, &mut channels, |call, ch| match call {
            SeqCall::Pump => {
                if let PumpOutcome::Ran(evs) | PumpOutcome::Runaway(evs) = pump_delta_time(ch, body)
                {
                    events.extend(evs);
                }
            }
            SeqCall::VolUp => {
                let vol = ch.vol;
                volume_slide_tick(ch, SlideDir::Up, vol);
            }
            SeqCall::VolDown => {
                let vol = ch.vol;
                volume_slide_tick(ch, SlideDir::Down, vol);
            }
            SeqCall::Tempo => {
                tempo_slide_tick(ch, 60);
            }
            SeqCall::Stop => stop_channel(ch),
            SeqCall::Start => start_channel(ch),
            SeqCall::Rewind => {}
        })
        .expect("the latch is never held here");
        frames += 1;
    }

    let first_pass: Vec<SeqEvent> = events
        .iter()
        .copied()
        .take_while(|e| !matches!(e, SeqEvent::EndOfTrack(_) | SeqEvent::LoopMarker(_)))
        .collect();
    let ended = first_pass.len() < events.len();
    let (mut notes, mut ccs, mut pcs, mut bends, mut metas) = (0u32, 0u32, 0u32, 0u32, 0u32);
    let mut bad: Vec<SeqEvent> = Vec::new();
    for e in &first_pass {
        match e {
            SeqEvent::Note { .. } => notes += 1,
            SeqEvent::ControlChange(_) => ccs += 1,
            SeqEvent::ProgramChange(_) => pcs += 1,
            SeqEvent::PitchBend => bends += 1,
            SeqEvent::Meta(_) => metas += 1,
            SeqEvent::Unknown(_) | SeqEvent::Overrun => bad.push(*e),
            SeqEvent::EndOfTrack(_) | SeqEvent::LoopMarker(_) => unreachable!(),
        }
    }
    eprintln!(
        "[w1e-transport] entry={} frames={} notes={} cc={} pc={} bend={} meta={} ended={}",
        entry.index, frames, notes, ccs, pcs, bends, metas, ended
    );

    assert!(ended, "no end-of-track within {frame_cap} frames");
    assert!(
        bad.is_empty(),
        "the retail walk desynchronised: {bad:?} - a status class the disc \
         stream uses that the ported decoder / handler tails mis-consume"
    );
    assert_eq!(
        notes,
        summary.note_on + summary.note_off,
        "note events (retail counts a velocity-0 release as a note event)"
    );
    assert_eq!(pcs, summary.program_change, "program changes");
    assert_eq!(ccs, summary.control_change, "control changes");
    assert_eq!(bends, summary.pitch_bend, "pitch bends");
    assert_eq!(
        metas,
        summary.set_tempo + summary.time_sig + summary.other_meta,
        "meta events (end-of-track excluded on both sides)"
    );

    // The transport arms, driven through the same tail dispatch retail walks:
    // `SsSeqStop` raises the bit, the next `SsSeqCalc` frame consumes it.
    channels[0][0].flags |= flag::STOP;
    let trace = seq_calc(&mut state, &mut channels, |call, ch| match call {
        SeqCall::Pump => {
            let _ = pump_delta_time(ch, body);
        }
        SeqCall::Stop => stop_channel(ch),
        SeqCall::Start => start_channel(ch),
        _ => {}
    })
    .expect("not latched");
    assert!(
        trace.iter().any(|(_, _, c)| *c == SeqCall::Stop),
        "the STOP bit must reach the stop arm"
    );
    assert_eq!(channels[0][0].playing, 0, "stop clears the playing byte");
    assert_eq!(
        channels[0][0].flags & flag::STOP,
        0,
        "stop consumes its own bit"
    );

    channels[0][0].flags |= flag::START;
    let trace = seq_calc(&mut state, &mut channels, |call, ch| match call {
        SeqCall::Pump => {
            let _ = pump_delta_time(ch, body);
        }
        SeqCall::Stop => stop_channel(ch),
        SeqCall::Start => start_channel(ch),
        _ => {}
    })
    .expect("not latched");
    assert!(
        trace.iter().any(|(_, _, c)| *c == SeqCall::Start),
        "the START bit must reach the start arm"
    );
    assert_eq!(channels[0][0].playing, 1, "start sets the playing byte");
    assert_eq!(
        channels[0][0].flags & flag::START,
        0,
        "start consumes its own bit"
    );
}

/// Session teardown through the SEQ resource-slot table: the release path
/// (`FUN_8001FF58`) fires the VAB close exactly once with the slot's handle,
/// and the close is what actually silences the session.
///
/// The installer role (stamp the handle, raise the loaded flag - the
/// `chunk_install` walker's job) is played by the test; the release under
/// measurement is the untouched production arm, and the assertion is at the
/// **output**: after the close detaches the sequencer, the sink must decay to
/// exact silence, and a second release must be a no-op.
#[test]
fn releasing_the_seq_resource_slot_tears_the_session_down() {
    let Some(extracted) = disc_gate() else { return };
    let mut archive = Archive::open(&extracted.join("PROT.DAT")).expect("open PROT");
    let Some(entry) = first_bgm_entry(&mut archive, &extracted) else {
        eprintln!("[skip] no music_01 entry carries both pBAV and pQES");
        return;
    };

    let mut sink = TestAudioSink::new(SPU_INTERNAL_RATE);
    stage_bgm(&mut sink, &entry);
    let (before, _) = run_frames(&mut sink, 120);
    if before.is_silent() {
        eprintln!("[skip] track produced no signal in the first 2 s");
        return;
    }

    // The retail table is 12-byte records at 0x80091508; the installer stamps
    // the handle byte and raises the loaded flag when the VAB/SEQ upload
    // lands. Slot 2 is arbitrary - the release indexes, it does not search.
    const SLOT: usize = 2;
    const HANDLE: i8 = 5;
    let mut table = SeqResourceTable::new(16);
    *table.slot_mut(SLOT).unwrap() = SeqResourceSlot {
        handle: HANDLE,
        loaded: true,
    };

    let mut closed: Vec<i8> = Vec::new();
    let fired = table.release(SLOT, |h| {
        closed.push(h);
        // The engine's VAB-close analog: key off and drop the sequencer
        // (`FUN_80068C80`'s SpuFree half is the allocator's business).
        sink.detach_sequencer();
    });
    assert!(fired, "a loaded slot must close");
    assert_eq!(
        closed,
        vec![HANDLE],
        "the close gets the slot's handle byte"
    );
    assert!(
        sink.sequencer_progress().is_none(),
        "the sequencer is gone after the close"
    );
    assert!(
        !table.slot(SLOT).unwrap().loaded,
        "the loaded flag is cleared"
    );

    // Output law: the keyed-off voices decay through their ADSR release until
    // only the reverb floor remains. **Exact digital silence never returns**,
    // and that is a measured property of the session, not a defect in the
    // close: the boot enables retail's Studio C reverb, and the reverb
    // network's multiply is `(a * coef) >> 15` - an arithmetic shift that
    // truncates toward -infinity, under which a buffer cell holding `-1`
    // recirculates as `-1` forever (the same sticky low bit the hardware's
    // own reverb arithmetic has). First execution of this rung measured
    // exactly that: every voice off, every output sample pinned at |1| LSB.
    // So the assertion is "decays to the 1-LSB reverb floor", and anything
    // in a host that gates on `is_silent()` after reverb has run will never
    // fire - that is what this rung pins.
    let mut floor_at = None;
    let mut last = SinkMeasure::default();
    for f in 0..600usize {
        last = sink.render_video_frame();
        if last.peak <= 1 {
            floor_at = Some(f);
            break;
        }
    }
    let live: Vec<String> = sink.with_spu(|spu: &mut Spu| {
        spu.voices
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_off())
            .map(|(i, v)| format!("v{i} adsr={:?}", v.adsr))
            .collect()
    });
    eprintln!(
        "[w1e-release] floor_at={floor_at:?} last_peak={} live_voices={live:?}",
        last.peak
    );
    assert!(
        live.is_empty(),
        "voices still keyed after the close: {live:?}"
    );
    assert!(
        floor_at.is_some(),
        "10 s after the release the sink still carries signal above the 1-LSB \
         reverb floor (peak {}) - the close did not key the session's voices \
         off",
        last.peak
    );

    // Idempotence: retail's release re-tests the loaded flag, so a second
    // call must not fire the close again.
    let fired_again = table.release(SLOT, |_| panic!("double close"));
    assert!(!fired_again, "a released slot is a no-op");
}
