use super::voices::{flush_key_on_lag, flush_key_on_lag_with};
use super::*;
use crate::Spu;
use crate::spu::adsr::Phase;
use psx_seq::ChannelMessage;
use psx_seq::SEQ_MAGIC;

fn synthetic_seq() -> Seq {
    // ppqn=480, tempo 500000 us/qn (120 BPM) so 1 quarter = 500_000 us.
    let mut buf = Vec::new();
    buf.extend_from_slice(&SEQ_MAGIC);
    buf.extend_from_slice(&[0x00, 0x01]);
    buf.extend_from_slice(&[0x01, 0xE0]);
    buf.extend_from_slice(&[0x07, 0xA1, 0x20]);
    buf.push(0x04);
    buf.push(0x02);
    // delta 0, ProgramChange ch0 prog 0
    buf.push(0x00);
    buf.push(0xC0);
    buf.push(0x00);
    // delta 0, NoteOn key 60 vel 100
    buf.push(0x00);
    buf.push(0x90);
    buf.push(60);
    buf.push(100);
    // delta 480 (1 quarter = 500000 us), NoteOff via NoteOn vel=0
    buf.push(0x83);
    buf.push(0x60);
    buf.push(60);
    buf.push(0);
    // delta 0, end-of-track
    buf.push(0x00);
    buf.push(0xFF);
    buf.push(0x2F);
    buf.push(0x00);
    Seq::parse(&buf).unwrap()
}

fn empty_bank() -> VabBank {
    VabBank {
        master_vol: 127,
        samples: vec![],
        programs: vec![],
    }
}

/// When every voice is busy, stealing must FREE the stolen note's slot (and
/// key-off its voice), not leave a second `ActiveNote` aliasing the reused
/// voice - otherwise the stale note's later key-off cuts the new note and the
/// voice leaks (stays "occupied" forever).
#[test]
fn alloc_voice_steal_frees_the_stolen_note_slot() {
    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    let mut spu = Spu::new();
    // One active note per voice (0..24): the occupied mask blocks every
    // voice, so allocation must steal.
    seq.voice_held = 0xFF_FFFF;
    seq.voice_env = [0x4000; 24];
    for v in 0..24u8 {
        seq.active.push(ActiveNote {
            channel: 0,
            voice: v,
            velocity: 127,
            program: 0,
            tone: 0,
            key_on_stamp: 0,
            pending: None,
        });
    }
    assert_eq!(seq.active.len(), 24);

    let stolen = seq.alloc_voice(&mut spu, 0);

    assert_eq!(stolen, Some(0), "steals the oldest note's voice");
    assert_eq!(
        seq.active.len(),
        23,
        "the stolen note's slot is freed - no aliasing entry"
    );
    assert!(
        !seq.active.iter().any(|n| n.voice == 0),
        "no lingering ActiveNote still references the reused voice"
    );
}

/// A cue re-keying a voice under a sounding note takes the voice from
/// it: the note's later note-off must not key off the cue now playing
/// there (retail's cue key-on rewrites the voice's note record, so the
/// sequencer's note-off no longer matches it).
#[test]
fn a_voice_rekeyed_under_a_note_is_not_keyed_off_by_its_note_off() {
    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    let mut spu = Spu::new();
    spu.voices[23].key_on(&spu.ram.clone());
    let stamp = spu.voices[23].key_on_count();
    seq.active.push(ActiveNote {
        channel: 0,
        voice: 23,
        velocity: 127,
        program: 0,
        tone: 0,
        key_on_stamp: stamp,
        pending: None,
    });
    // A cue takes voice 23.
    spu.voices[23].key_on(&spu.ram.clone());
    let phase = spu.voices[23].adsr.phase;
    seq.fire_channel(
        &mut spu,
        0,
        ChannelMessage::NoteOff {
            key: 60,
            velocity: 0,
        },
    );
    assert_eq!(spu.voices[23].adsr.phase, phase, "the cue keeps sounding");
    assert!(seq.active.is_empty(), "the note no longer owns the voice");
}

fn record(program: usize, key: u8, stamp: u32) -> Option<VoiceNote> {
    Some(VoiceNote {
        program,
        tone: 0,
        key,
        channel: 0,
        velocity: 100,
        pan: PAN_CENTER,
        stamp: Some(stamp),
        staged_wheel: None,
        staged_mix: None,
    })
}

/// `FUN_8006688C` tests the channel's program as it stands now and the key, not
/// the channel: a note whose channel changed program is not found, and another
/// channel's note on that program and key is.
#[test]
fn a_note_off_goes_by_program_and_key_not_by_channel() {
    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    let mut spu = Spu::new();
    let ram = spu.ram.clone();
    for v in 0..2u8 {
        spu.voices[v as usize].key_on(&ram);
        occupy(&mut seq, v);
        seq.active.last_mut().unwrap().key_on_stamp = 1;
    }
    seq.voice_note[0] = record(3, 60, 1);
    seq.voice_note[1] = record(4, 60, 1);
    seq.channels[0].program = 4;
    seq.channels[5].program = 3;
    // Channel 5 never keyed anything, and its note-off releases channel 0's note.
    seq.note_off(&mut spu, 5, 60);
    assert_eq!(spu.voices[0].adsr.phase, Phase::Release);
    assert_ne!(spu.voices[1].adsr.phase, Phase::Release);
    assert_eq!(seq.voice_held, 1 << 1);
    // Channel 1 on program 2: nothing of that program is sounding.
    seq.channels[1].program = 2;
    seq.note_off(&mut spu, 1, 60);
    assert_ne!(spu.voices[1].adsr.phase, Phase::Release);
}

/// The history holds a voice through fifteen silent flushes after its last
/// key-on, and a released voice with its envelope down is free at once.
#[test]
fn the_silent_history_lets_a_held_voice_go() {
    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    let spu = Spu::new();
    occupy(&mut seq, 2);
    seq.forget_silence(2);
    let mut flushes = 0;
    while seq.voice_held != 0 {
        seq.read_envelopes(&spu);
        flushes += 1;
        assert!(flushes <= SILENT_RING_LEN + 1, "never let go");
    }
    assert!(flushes >= SILENT_RING_LEN - 1);
}

/// Where the history's index stands when a voice falls silent decides the
/// count: the release test ANDs the fixed words `0..15`, so a run that crosses
/// the unread word `15` takes a flush more.
#[test]
fn the_silent_historys_phase_decides_fifteen_or_sixteen_flushes() {
    let flushes_from = |phase: usize| {
        let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
        let spu = Spu::new();
        seq.set_silent_ring_phase(phase);
        occupy(&mut seq, 2);
        seq.forget_silence(2);
        let mut flushes = 0;
        while seq.voice_held != 0 {
            seq.read_envelopes(&spu);
            flushes += 1;
        }
        flushes
    };
    // Written from word 0 (index 15 before the first flush): words 0..=14.
    assert_eq!(flushes_from(SILENT_RING_LEN - 1), SILENT_RING_LEN - 1);
    // Written from word 9: 9..=14, the unread 15, then 0..=8.
    assert_eq!(flushes_from(8), SILENT_RING_LEN);
}

/// A sequencer started over tails another one left: they count by their envelope
/// and hold no mark.
#[test]
fn a_new_sequencer_counts_inherited_tails_by_their_envelopes() {
    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    let mut spu = Spu::new();
    let ram = spu.ram.clone();
    spu.voices[0].key_on(&ram);
    spu.voices[0].adsr.level = 0x2000;
    spu.voices[0].key_off();
    seq.read_envelopes(&spu);
    assert_eq!(seq.voice_held, 0);
    assert_eq!(seq.voice_env[0], 0x2000);
    assert_eq!(seq.alloc_voice(&mut spu, 0), Some(1));
}

/// A cue keying a voice marks it held for the scan, with its age zeroed.
#[test]
fn a_cue_key_on_holds_the_voice_for_the_scan() {
    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    let mut spu = Spu::new();
    let ram = spu.ram.clone();
    seq.voice_age[23] = 9;
    seq.read_envelopes(&spu);
    spu.voices[23].key_on(&ram);
    seq.read_envelopes(&spu);
    assert_ne!(seq.voice_held & (1 << 23), 0);
    assert_eq!(seq.voice_age[23], 0);
}

/// Occupy a voice with a synthetic active note so the allocator treats it
/// as reserved (the engine stand-in for the retail in-use marker).
fn occupy(seq: &mut Sequencer, voice: u8) {
    seq.voice_held |= 1 << voice;
    seq.active.push(ActiveNote {
        channel: 0,
        voice,
        velocity: 127,
        program: 0,
        tone: 0,
        key_on_stamp: 0,
        pending: None,
    });
}

/// FUN_80066B00 case: the scan takes the FIRST idle voice in ascending
/// order and stops there - it does not prefer a later idle voice, and the
/// steal evaluation of earlier voices is discarded.
#[test]
fn retail_alloc_takes_first_idle_and_stops() {
    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    let mut spu = Spu::new();
    // Voices 0..=2 reserved, 3 and 17 idle.
    for v in 0..3 {
        occupy(&mut seq, v);
    }
    assert_eq!(seq.alloc_voice(&mut spu, 0), Some(3));
    // Age bookkeeping ran: everyone aged, winner zeroed.
    assert_eq!(seq.voice_age[3], 0);
    assert_eq!(seq.voice_age[0], 1);
}

/// FUN_80066B00 case: a released voice still ringing (envelope above
/// zero, no owning note) is NOT idle - it is skipped in favor of a later
/// fully-decayed voice.
#[test]
fn retail_alloc_skips_ringing_release_tails() {
    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    let mut spu = Spu::new();
    // Voice 0: unreserved but still ringing (release tail).
    seq.voice_env[0] = 0x1000;
    assert_eq!(seq.alloc_voice(&mut spu, 0), Some(1));
}

/// FUN_80066B00 steal tier: the candidate is the minimum-priority voice
/// among those with priority <= the request; the threshold tightens as
/// lower priorities are seen.
#[test]
fn retail_steal_picks_minimum_priority_at_or_below_request() {
    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    let mut spu = Spu::new();
    for v in 0..24 {
        occupy(&mut seq, v);
        seq.voice_env[v as usize] = 0x4000;
    }
    seq.voice_prio = [9; 24];
    seq.voice_prio[5] = 4; // strictly below request: tier-1 candidate
    seq.voice_prio[9] = 2; // even lower: tightens the threshold
    seq.voice_prio[13] = 2; // equal to tightened threshold, same env, age 0
    assert_eq!(seq.alloc_voice(&mut spu, 6), Some(9));
}

/// FUN_80066B00 equal-priority tie-breaks: lowest envelope level first,
/// then largest allocation age (least-recently allocated).
#[test]
fn retail_steal_ties_break_on_env_then_age() {
    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    let mut spu = Spu::new();
    for v in 0..24 {
        occupy(&mut seq, v);
        seq.voice_env[v as usize] = 0x4000;
    }
    // All equal priority (= request). Voice 7 is quietest.
    seq.voice_env[7] = 0x0100;
    assert_eq!(seq.alloc_voice(&mut spu, 0), Some(7));

    // Re-occupy 7 (alloc freed it) and equalize envelopes; voice 11 is
    // now the least-recently allocated (largest age).
    occupy(&mut seq, 7);
    for v in 0..24 {
        seq.voice_env[v as usize] = 0x4000;
    }
    seq.voice_age = [3; 24];
    seq.voice_age[11] = 200;
    assert_eq!(seq.alloc_voice(&mut spu, 0), Some(11));
}

/// FUN_80066B00 case 3: when every sounding voice outranks the request's
/// priority, the note is DROPPED (retail returns the out-of-range voice
/// count) - it does not steal a higher-priority voice.
#[test]
fn retail_alloc_drops_note_when_all_voices_outrank_request() {
    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    let mut spu = Spu::new();
    for v in 0..24 {
        occupy(&mut seq, v);
        seq.voice_env[v as usize] = 0x4000;
    }
    seq.voice_prio = [10; 24];
    assert_eq!(seq.alloc_voice(&mut spu, 3), None);
    assert_eq!(seq.active.len(), 24, "no voice was stolen");
}

/// The retail age-comparison signedness quirk: a challenger age >= 0x8000
/// sign-extends negative and can never displace the incumbent.
#[test]
fn retail_steal_age_signedness_quirk() {
    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    let mut spu = Spu::new();
    for v in 0..24 {
        occupy(&mut seq, v);
        seq.voice_env[v as usize] = 0x4000;
    }
    seq.voice_age[0] = 5;
    seq.voice_age[1] = 0x8001; // huge age, but sign-extends negative
    assert_eq!(seq.alloc_voice(&mut spu, 0), Some(0));
}

#[test]
fn fires_program_change_immediately() {
    let mut spu = Spu::new();
    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    // The retail pump runs once per vsync: nothing fires before the first.
    seq.tick_us(&mut spu, 0.0);
    assert_eq!(seq.next, 0);
    // First two events have delta 0 and fire at the first vsync. Channel
    // 0 program should be 0 and there should be no active note (bank is
    // empty so play_note returns false).
    seq.tick_us(&mut spu, 1_000_000.0 / 60.0);
    assert_eq!(seq.channels[0].program, 0);
    assert_eq!(seq.active_notes(), 0);
    // Third event has delta 480 - not yet fired.
    assert_eq!(seq.next, 2);
}

#[test]
fn advances_tick_count_after_quarter_note() {
    let mut spu = Spu::new();
    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    seq.tick_us(&mut spu, 500_000.0);
    // After 1 quarter-note: 3rd event (NoteOff, +480) should have fired.
    assert!(seq.next >= 3);
}

#[test]
fn finishes_at_end_of_track() {
    let mut spu = Spu::new();
    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    // Drain all events with one big tick (well past total length).
    seq.tick_us(&mut spu, 10_000_000.0);
    assert!(seq.is_finished());
}

#[test]
fn loop_rewinds_at_eot() {
    let mut spu = Spu::new();
    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    seq.set_loop_to(0);
    seq.tick_us(&mut spu, 10_000_000.0);
    // Looping should have prevented the finished flag.
    assert!(!seq.is_finished());
    // And re-fired at least one channel event.
    assert!(seq.playhead_ticks() < seq.seq.total_ticks() * 2);
}

/// The integer sample-clock fires an event at exactly the right SPU
/// sample: at 120 BPM / ppqn 480, one quarter note (delta 480) is 0.5 s =
/// 22050 samples. The event must fire on sample 22050, not 22049, with no
/// floating-point slack.
#[test]
fn sample_clock_fires_quarter_note_at_exact_sample() {
    // ppqn 480, tempo 500000 (120 BPM). Events: ProgramChange (delta 0),
    // NoteOn (delta 480), EOT (delta 0).
    let mut buf = Vec::new();
    buf.extend_from_slice(&SEQ_MAGIC);
    buf.extend_from_slice(&[0x00, 0x01]);
    buf.extend_from_slice(&[0x01, 0xE0]); // ppqn 480
    buf.extend_from_slice(&[0x07, 0xA1, 0x20]); // 500000 us/qn
    buf.push(0x04);
    buf.push(0x02);
    buf.push(0x00); // delta 0
    buf.push(0xC0);
    buf.push(0x00); // ProgramChange
    buf.push(0x83); // delta 480
    buf.push(0x60);
    buf.push(0x90);
    buf.push(60);
    buf.push(100); // NoteOn
    buf.push(0x00);
    buf.push(0xFF);
    buf.push(0x2F); // EOT
    let seq = Seq::parse(&buf).unwrap();
    let mut s = Sequencer::new(seq, empty_bank());
    // The sample clock is the exact-tempo mode's; the retail pump fires
    // on the vsync grid instead.
    s.set_exact_tempo(true);
    let mut spu = Spu::new();

    // Drain the leading zero-delta ProgramChange.
    s.tick_sample(&mut spu);
    assert_eq!(s.next, 1, "ProgramChange fires immediately");

    // 22050 samples == exactly one quarter at 120 BPM. We already consumed
    // one sample, so 22048 more leaves us one short; the 22049th tips it.
    for _ in 0..22_048 {
        s.tick_sample(&mut spu);
    }
    assert_eq!(s.next, 1, "NoteOn must not fire one sample early");
    s.tick_sample(&mut spu);
    assert!(s.next >= 2, "NoteOn fires on the exact 22050th sample");
}

/// A track with a mid-stream Loop Start marker (CC 99 value 20) must
/// rewind to that marker - not to event 0 - when Loop Forever (CC 99
/// value 30) fires, and the integer sample-clock must stay exact across
/// the rewind so the looped body re-fires on the same sample offset.
#[test]
fn loop_forever_rewinds_to_in_stream_marker_not_zero() {
    // ppqn 480, tempo 500000 (120 BPM) => one quarter (delta 480) is
    // exactly 22050 SPU samples at 44.1 kHz.
    let mut buf = Vec::new();
    buf.extend_from_slice(&SEQ_MAGIC);
    buf.extend_from_slice(&[0x00, 0x01]); // version 1 (PsyQ shape)
    buf.extend_from_slice(&[0x01, 0xE0]); // ppqn 480
    buf.extend_from_slice(&[0x07, 0xA1, 0x20]); // 500000 us/qn
    buf.push(0x04);
    buf.push(0x02);
    // idx 0: delta 0, ProgramChange ch0 prog 0
    buf.push(0x00);
    buf.push(0xC0);
    buf.push(0x00);
    // idx 1: delta 480, NoteOn ch0 key 60 vel 100
    buf.push(0x83);
    buf.push(0x60);
    buf.push(0x90);
    buf.push(60);
    buf.push(100);
    // idx 2: delta 0, Loop Start (CC 0xB0 ch0 controller 99 value 20).
    // Records the rewind target as idx 3 (the event after this marker).
    buf.push(0x00);
    buf.push(0xB0);
    buf.push(99);
    buf.push(20);
    // idx 3: delta 0, data entry: the loop's count, forever.
    buf.push(0x00);
    buf.push(0xB0);
    buf.push(6);
    buf.push(127);
    // idx 4: delta 480, NoteOn ch0 key 64 vel 100 (the loop body start)
    buf.push(0x83);
    buf.push(0x60);
    buf.push(0x90);
    buf.push(64);
    buf.push(100);
    // idx 4: delta 480, Loop Forever (CC 99 value 30) -> rewind to idx 3
    buf.push(0x83);
    buf.push(0x60);
    buf.push(0xB0);
    buf.push(99);
    buf.push(30);
    // idx 5: delta 0, end-of-track (never reached while looping)
    buf.push(0x00);
    buf.push(0xFF);
    buf.push(0x2F);
    let seq = Seq::parse(&buf).unwrap();
    let mut s = Sequencer::new(seq, empty_bank());
    let mut spu = Spu::new();

    // Loop Forever (idx 4) fires at sample 66150 (= 3 quarters). After it
    // fires the sequencer rewinds to the recorded Loop Start target.
    for _ in 0..66_150 {
        s.tick_sample(&mut spu);
    }
    assert!(!s.is_finished(), "Loop Forever must not finish the track");
    assert_eq!(
        s.loop_start,
        Some(3),
        "Loop Start records the event after the marker"
    );
    assert_eq!(
        s.next, 4,
        "Loop Forever rewinds to the in-stream marker, not to 0; its count re-reads at once"
    );

    // Exactness across the rewind: the looped NoteOn at idx 3 has delta
    // 480 = 22050 samples. It must not re-fire one sample early.
    for _ in 0..22_049 {
        s.tick_sample(&mut spu);
    }
    assert_eq!(s.next, 4, "looped NoteOn must not fire early after rewind");
    s.tick_sample(&mut spu);
    assert!(
        s.next >= 5,
        "looped NoteOn fires on the exact sample after rewind"
    );

    // And it keeps looping: drive well past another Loop Forever and the
    // playhead is still parked at the marker, never finished.
    for _ in 0..66_150 {
        s.tick_sample(&mut spu);
    }
    assert!(!s.is_finished());
    assert_eq!(s.next, 4, "repeated loops keep returning to the marker");
}

/// A score built `Loop Start` then events then end-of-track, or one whose Loop
/// Forever has no data entry to count from.
fn uncounted_loop_seq(forever: bool) -> Seq {
    let mut buf = Vec::new();
    buf.extend_from_slice(&SEQ_MAGIC);
    buf.extend_from_slice(&[0x00, 0x01]);
    buf.extend_from_slice(&[0x01, 0xE0]);
    buf.extend_from_slice(&[0x07, 0xA1, 0x20]);
    buf.push(0x04);
    buf.push(0x02);
    // idx 0: delta 0, ProgramChange: the event only a head restart replays.
    buf.extend_from_slice(&[0x00, 0xC0, 0x00]);
    // idx 1: delta 0, Loop Start
    buf.extend_from_slice(&[0x00, 0xB0, 99, 20]);
    // idx 2: delta 480, NoteOn
    buf.extend_from_slice(&[0x83, 0x60, 0x90, 60, 100]);
    if forever {
        buf.extend_from_slice(&[0x00, 0xB0, 99, 30]);
    }
    buf.extend_from_slice(&[0x00, 0xFF, 0x2F]);
    Seq::parse(&buf).unwrap()
}

/// `FUN_8006139C` with a zero count clears the busy flag and reads on, and
/// `FUN_80063AA8` then stops a sequence whose play count `+0x20` is spent: the
/// three retail scores written Loop Start, Loop Forever and no data entry play
/// once.
#[test]
fn an_uncounted_loop_runs_on_to_the_end_and_stops() {
    let mut s = Sequencer::new(uncounted_loop_seq(true), empty_bank());
    let mut spu = Spu::new();
    for _ in 0..40_000 {
        s.tick_sample(&mut spu);
    }
    assert!(s.is_finished());
    assert_eq!(s.loop_count(), 0, "no rewind at the Loop Forever");
}

/// End-of-track never looks at a Loop Start: `FUN_80063AA8` reloads the cursor
/// from the score head.
#[test]
fn end_of_track_goes_to_the_head_not_a_marker() {
    let mut s = Sequencer::new(uncounted_loop_seq(false), empty_bank());
    s.set_loop_to(0);
    let mut spu = Spu::new();
    let mut heads = 0;
    for _ in 0..40_000 {
        s.tick_sample(&mut spu);
        if s.next_event() <= 1 {
            heads += 1;
        }
    }
    assert!(s.loop_count() >= 1);
    assert!(heads > 0, "the rewind lands on the program change at idx 0");
}

/// `FUN_8006139C`: the Loop Start saves the cursor past the delta it read, and
/// the loop-for-good arm zeroes the wait, so the event behind the marker waits
/// on the first pass only. Across `music_01` that delta is usually one tick,
/// and waiting it every pass moved the tick stream's phase against the vsync.
#[test]
fn a_loop_jump_does_not_wait_the_delta_behind_the_loop_start_again() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&SEQ_MAGIC);
    buf.extend_from_slice(&[0x00, 0x01]);
    buf.extend_from_slice(&[0x01, 0xE0]);
    buf.extend_from_slice(&[0x07, 0xA1, 0x20]);
    buf.push(0x04);
    buf.push(0x02);
    buf.extend_from_slice(&[0x00, 0xB0, 99, 20]);
    // The delta behind the marker: 96 ticks, six vsyncs at 120 BPM.
    buf.extend_from_slice(&[0x60, 0xB0, 6, 127]);
    buf.extend_from_slice(&[0x00, 0x90, 60, 100]);
    buf.extend_from_slice(&[0x60, 0xB0, 99, 30]);
    buf.extend_from_slice(&[0x00, 0xFF, 0x2F]);
    let mut s = Sequencer::new(Seq::parse(&buf).unwrap(), empty_bank());
    let mut spu = Spu::new();
    let mut jumps = Vec::new();
    let mut count = s.loop_count();
    for vsync in 0..60 {
        s.step_vsync(&mut spu);
        if s.loop_count() != count {
            count = s.loop_count();
            jumps.push(vsync);
        }
    }
    let gaps: Vec<i32> = jumps.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(gaps.len() >= 3, "{jumps:?}");
    // One pass is the 96 ticks before the loop end alone, not 192.
    assert!(gaps.iter().all(|&g| g == 6), "{jumps:?}");
}

/// A flush writes the KON mask after a `FUN_8006C048` call for every voice with
/// a staged register, so a bigger chord keys later, and a voice re-volumed in
/// that vsync costs what a keyed one does.
#[test]
fn the_kon_write_trails_the_flush_by_its_staged_voices() {
    assert_eq!(flush_key_on_lag(0), 0);
    assert_eq!(flush_key_on_lag(1), 0);
    assert_eq!(flush_key_on_lag(2), 1);
    assert_eq!(flush_key_on_lag(6), 6);
    assert_eq!(flush_key_on_lag(11), 12);
}

/// A positional emitter's three voices, re-volumed on the frame's vsync, put
/// `0.85` samples each on that flush's key-ons, and nothing on a flush with
/// no key-on.
#[test]
fn an_emitters_volume_writes_delay_the_scores_key_ons() {
    assert_eq!(flush_key_on_lag_with(1, 3), 3);
    assert_eq!(flush_key_on_lag_with(2, 3), 4);
    assert_eq!(flush_key_on_lag_with(4, 0), flush_key_on_lag(4));

    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    let mut spu = Spu::new();
    seq.stage_external_voices(0x7_0000);
    seq.flush_key_ons(&mut spu);
    assert_eq!(seq.last_flush_staged(), 0, "nothing keyed: the mask clears");
}

/// A finite count jumps back until it runs out: `3` plays the body three times.
#[test]
fn a_counted_loop_plays_its_count() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&SEQ_MAGIC);
    buf.extend_from_slice(&[0x00, 0x01]);
    buf.extend_from_slice(&[0x01, 0xE0]);
    buf.extend_from_slice(&[0x07, 0xA1, 0x20]);
    buf.push(0x04);
    buf.push(0x02);
    buf.extend_from_slice(&[0x00, 0xB0, 99, 20]);
    buf.extend_from_slice(&[0x00, 0xB0, 6, 3]);
    buf.extend_from_slice(&[0x83, 0x60, 0x90, 60, 100]);
    buf.extend_from_slice(&[0x00, 0xB0, 99, 30]);
    buf.extend_from_slice(&[0x00, 0xFF, 0x2F]);
    let mut s = Sequencer::new(Seq::parse(&buf).unwrap(), empty_bank());
    let mut spu = Spu::new();
    for _ in 0..200_000 {
        s.tick_sample(&mut spu);
    }
    assert_eq!(s.loop_count(), 2);
    assert!(s.is_finished());
}

#[test]
fn tempo_event_changes_us_per_tick() {
    // Build a SEQ that starts at 120 BPM then jumps to 60 BPM after 0
    // ticks: the second event should be twice as long in real time.
    let mut buf = Vec::new();
    buf.extend_from_slice(&SEQ_MAGIC);
    buf.extend_from_slice(&[0x00, 0x01]);
    buf.extend_from_slice(&[0x01, 0xE0]);
    // initial tempo 500_000 (120 BPM)
    buf.extend_from_slice(&[0x07, 0xA1, 0x20]);
    buf.push(0x04);
    buf.push(0x02);
    // delta 0, set-tempo to 1_000_000 us/qn (60 BPM). PSX SEQ meta has no
    // MIDI length byte: the 3 tempo bytes follow 0x51 directly.
    buf.push(0x00);
    buf.push(0xFF);
    buf.push(0x51);
    buf.extend_from_slice(&[0x0F, 0x42, 0x40]); // 1_000_000 BE
    // delta 480, end-of-track
    buf.push(0x83);
    buf.push(0x60);
    buf.push(0xFF);
    buf.push(0x2F);
    let seq = Seq::parse(&buf).unwrap();
    let mut s = Sequencer::new(seq, empty_bank());
    let mut spu = Spu::new();
    // 1 quarter at the original tempo wouldn't reach EOT, but the
    // post-tempo-change tick rate should mean 1 quarter requires 1s.
    s.tick_us(&mut spu, 500_000.0);
    assert!(!s.is_finished()); // halfway through
    s.tick_us(&mut spu, 500_000.0);
    assert!(s.is_finished());
}

#[test]
fn apply_channel_pan_attenuates_opposite_side() {
    // Center pan leaves both sides untouched.
    assert_eq!(
        apply_channel_pan(0x3000, 0x3000, PAN_CENTER),
        (0x3000, 0x3000)
    );
    // Hard left silences the right, leaves the left.
    assert_eq!(apply_channel_pan(0x3000, 0x3000, 0), (0x3000, 0));
    // Hard right silences the left, leaves the right.
    assert_eq!(apply_channel_pan(0x3000, 0x3000, 0x7f), (0, 0x3000));
    // Partial left attenuates the right but not below zero / not the left.
    let (l, r) = apply_channel_pan(0x3000, 0x3000, 0x20);
    assert_eq!(l, 0x3000);
    assert!(r > 0 && r < 0x3000);
}

#[test]
fn cc7_volume_rederives_the_sounding_voice_through_the_velocity() {
    let mut spu = Spu::new();
    let mut seq = Sequencer::new(synthetic_seq(), one_tone_bank());
    seq.set_master_vol(RETAIL_BGM_SEQ_VOL);
    seq.fire_channel(
        &mut spu,
        0,
        ChannelMessage::NoteOn {
            key: 60,
            velocity: 100,
        },
    );
    seq.flush_key_ons(&mut spu);
    let v = seq.active[0].voice as usize;
    let full = (spu.voices[v].vol_left, spu.voices[v].vol_right);
    // Retail's chain by hand: CC7 127 leaves velocity 100; head
    // 100 * 127 * 0x3FFF / 0x3F01 = 12_700, * 127 * 127 / 0x3F01 =
    // 12_700, * 107 / 127 = 10_700 per side; centre pans (64) cost the
    // left `(0x7F - 64) / 63` three times over (tone, program,
    // channel): 10_700 -> 10_700 -> 10_700; taper 10_700^2 / 0x3FFF.
    let expect = |vel: i32| {
        let head = vel * (127 * 0x3FFF) / 0x3F01;
        let head = head * 127 * 127 / 0x3F01;
        let side = head * 107 / 127;
        let l = side * 63 / 63 * 63 / 63 * 63 / 63;
        ((l * l / 0x3FFF) as i16, (side * side / 0x3FFF) as i16)
    };
    assert_eq!(full, expect(100));

    // CC7 63: the velocity becomes 100 * 63 / 127 = 49 (truncated), and
    // the whole chain re-runs from it.
    seq.fire_channel(
        &mut spu,
        0,
        ChannelMessage::ControlChange {
            control: 0x07,
            value: 63,
        },
    );
    assert_eq!(seq.channels[0].volume, 63);
    assert_eq!(
        (spu.voices[v].vol_left, spu.voices[v].vol_right),
        expect(49)
    );
    // Back to full re-derives the original, no compounding.
    seq.fire_channel(
        &mut spu,
        0,
        ChannelMessage::ControlChange {
            control: 0x07,
            value: 127,
        },
    );
    assert_eq!((spu.voices[v].vol_left, spu.voices[v].vol_right), full);
}

/// `FUN_80067E9C`, which `SsSeqSetVol` and each step of its slide run,
/// re-volumes every voice the sequence owns: a note held across a duck
/// comes down with it at the next flush, and comes back up with it.
#[test]
fn sequence_volume_change_revolumes_the_sounding_voice_at_the_flush() {
    let mut spu = Spu::new();
    let mut seq = Sequencer::new(synthetic_seq(), one_tone_bank());
    seq.set_master_vol(RETAIL_BGM_SEQ_VOL);
    seq.fire_channel(
        &mut spu,
        0,
        ChannelMessage::NoteOn {
            key: 60,
            velocity: 100,
        },
    );
    seq.flush_key_ons(&mut spu);
    let v = seq.active[0].voice as usize;
    let full = (spu.voices[v].vol_left, spu.voices[v].vol_right);
    // The chain of the CC7 test above, at sequence volume `sv`.
    let expect = |sv: i32| {
        let head = 100 * (127 * 0x3FFF) / 0x3F01;
        let head = head * 127 * 127 / 0x3F01;
        let side = head * sv / 127;
        ((side * side / 0x3FFF) as i16, (side * side / 0x3FFF) as i16)
    };
    assert_eq!(full, expect(107));

    // The 75% summon duck's floor: 107 * 161 / 215 = 80.
    seq.set_master_vol(80);
    // Nothing moves until the flush.
    assert_eq!((spu.voices[v].vol_left, spu.voices[v].vol_right), full);
    let mut spu_ticks = 0;
    while spu_ticks < FLUSH_SAMPLES {
        seq.tick_sample(&mut spu);
        spu_ticks += 1;
    }
    assert_eq!(
        (spu.voices[v].vol_left, spu.voices[v].vol_right),
        expect(80)
    );
    // Restored, no compounding.
    seq.set_master_vol(RETAIL_BGM_SEQ_VOL);
    for _ in 0..FLUSH_SAMPLES {
        seq.tick_sample(&mut spu);
    }
    assert_eq!((spu.voices[v].vol_left, spu.voices[v].vol_right), full);
}

#[test]
fn cc10_pan_repans_the_sounding_voice() {
    let mut spu = Spu::new();
    let mut seq = Sequencer::new(synthetic_seq(), one_tone_bank());
    seq.fire_channel(
        &mut spu,
        0,
        ChannelMessage::NoteOn {
            key: 60,
            velocity: 127,
        },
    );
    seq.flush_key_ons(&mut spu);
    let v = seq.active[0].voice as usize;
    let centre = (spu.voices[v].vol_left, spu.voices[v].vol_right);
    // Pan hard left: `FUN_8006861C` floors the pan at one, so the right side is
    // all but silenced, the left keeps its level, and the record holds the zero.
    seq.fire_channel(
        &mut spu,
        0,
        ChannelMessage::ControlChange {
            control: 0x0A,
            value: 0,
        },
    );
    assert_eq!(seq.channels[0].pan, 0);
    assert!(spu.voices[v].vol_right < centre.1 / 100);
    assert_eq!(spu.voices[v].vol_left, centre.0);
    // Back to centre re-pans from scratch.
    seq.fire_channel(
        &mut spu,
        0,
        ChannelMessage::ControlChange {
            control: 0x0A,
            value: PAN_CENTER,
        },
    );
    assert_eq!((spu.voices[v].vol_left, spu.voices[v].vol_right), centre);
}
fn layer_tone(vag: i16, min: u8, max: u8, center: u8) -> psx_vab::VagAtr {
    psx_vab::VagAtr {
        prior: 0,
        mode: 0,
        vol: 127,
        pan: 64,
        center,
        shift: 0,
        min,
        max,
        vibw: 0,
        vibt: 0,
        porw: 0,
        port: 0,
        pbmin: 0,
        pbmax: 0,
        reserved1: 0,
        reserved2: 0,
        adsr1: 0x80FF,
        adsr2: 0x5FC0,
        prog: 0,
        vag,
        reserved3: [0; 4],
    }
}

/// Program 0 stacks two tones over key 60 (and a third tone that does
/// not cover it): one note-on keys both layers on two voices, and its
/// note-off releases both - FUN_80068568 + the per-tone loop of
/// FUN_80066308.
#[test]
fn a_note_on_keys_every_tone_layered_over_its_key() {
    let upload = Some(crate::vab_bind::UploadedVag {
        addr: 0x1010,
        size: 0x20,
    });
    let bank = VabBank {
        master_vol: 127,
        samples: vec![upload, upload, upload],
        programs: vec![crate::vab_bind::VabProgram {
            mvol: 127,
            mpan: 64,
            tones: vec![
                layer_tone(1, 0, 127, 60),
                layer_tone(2, 72, 127, 60),
                layer_tone(3, 48, 72, 72),
            ],
            key_tones: 3,
        }],
    };
    assert_eq!(bank.layer_tones(0, 60), vec![0, 2]);
    let mut seq = Sequencer::new(synthetic_seq(), bank);
    let mut spu = Spu::new();
    seq.fire_channel(
        &mut spu,
        0,
        ChannelMessage::NoteOn {
            key: 60,
            velocity: 100,
        },
    );
    seq.flush_key_ons(&mut spu);
    let keyed: Vec<u32> = spu.voices.iter().map(|v| v.key_on_count()).collect();
    assert_eq!(keyed.iter().sum::<u32>(), 2, "one key-on per layer");
    assert_eq!(seq.active.len(), 2);
    let pitches: Vec<u16> = seq
        .active
        .iter()
        .map(|n| spu.voices[n.voice as usize].pitch)
        .collect();
    // Tone 0 is centred on the key, tone 2 an octave above it.
    assert_eq!(pitches, vec![0x1000, 0x800]);
    seq.fire_channel(
        &mut spu,
        0,
        ChannelMessage::NoteOff {
            key: 60,
            velocity: 0,
        },
    );
    assert!(seq.active.is_empty(), "the note-off releases both layers");
}

fn one_tone_bank() -> VabBank {
    let upload = Some(crate::vab_bind::UploadedVag {
        addr: 0x1010,
        size: 0x20,
    });
    VabBank {
        master_vol: 127,
        samples: vec![upload],
        programs: vec![crate::vab_bind::VabProgram {
            mvol: 127,
            mpan: 64,
            tones: vec![layer_tone(1, 0, 127, 60)],
            key_tones: 1,
        }],
    }
}

fn keys_written(spu: &Spu) -> u32 {
    spu.voices.iter().map(|v| v.key_on_count()).sum()
}

/// A note-on is staged and written at the next flush; a note-off before
/// that flush cancels it, so the note never sounds - FUN_80067480 clears
/// the staged key-on bit as it sets the key-off bit.
#[test]
fn a_note_released_before_the_flush_never_keys() {
    let mut seq = Sequencer::new(synthetic_seq(), one_tone_bank());
    let mut spu = Spu::new();
    let on = ChannelMessage::NoteOn {
        key: 60,
        velocity: 100,
    };
    let off = ChannelMessage::NoteOff {
        key: 60,
        velocity: 0,
    };
    seq.fire_channel(&mut spu, 0, on);
    assert_eq!(keys_written(&spu), 0, "staged, not yet written");
    seq.fire_channel(&mut spu, 0, off);
    seq.flush_key_ons(&mut spu);
    assert_eq!(
        keys_written(&spu),
        0,
        "cancelled by the same-period key-off"
    );
    assert!(seq.active.is_empty());

    seq.fire_channel(&mut spu, 0, on);
    seq.flush_key_ons(&mut spu);
    assert_eq!(keys_written(&spu), 1, "a flushed note keys");
    seq.fire_channel(&mut spu, 0, off);
    assert_eq!(keys_written(&spu), 1);
}

/// A dropped layer zeroes voice 0's staged ADSR2: `FUN_80066B00`'s failure
/// path stores to record `24`, which is voice 0's shadow ADSR2 word. Only a
/// voice-0 key-on staged ahead of the drop in the same flush carries it.
#[test]
fn a_dropped_note_zeroes_voice_0s_staged_adsr2() {
    let on = ChannelMessage::NoteOn {
        key: 60,
        velocity: 100,
    };
    let crowd = |seq: &mut Sequencer| {
        seq.voice_held = 0xFF_FFFF;
        seq.voice_env = [0x4000; 24];
        seq.voice_prio = [10; 24];
    };
    // The drop after voice 0's key-on: ADSR2 goes out as 0.
    let mut seq = Sequencer::new(synthetic_seq(), one_tone_bank());
    let mut spu = Spu::new();
    seq.fire_channel(&mut spu, 0, on);
    assert_eq!(seq.active[0].voice, 0);
    crowd(&mut seq);
    seq.fire_channel(&mut spu, 1, on);
    assert_eq!(seq.active.len(), 1, "the second note found no voice");
    seq.flush_key_ons(&mut spu);
    assert_eq!(spu.voices[0].adsr_cfg.raw, (0x80FF, 0));

    // The drop ahead of it: the key-on stages the tone's ADSR afresh.
    let mut seq = Sequencer::new(synthetic_seq(), one_tone_bank());
    let mut spu = Spu::new();
    crowd(&mut seq);
    seq.fire_channel(&mut spu, 1, on);
    seq.voice_held = 0;
    seq.voice_env = [0; 24];
    seq.fire_channel(&mut spu, 0, on);
    assert_eq!(seq.active[0].voice, 0);
    seq.flush_key_ons(&mut spu);
    assert_eq!(spu.voices[0].adsr_cfg.raw, (0x80FF, 0x5FC0));
}

/// The sample clock flushes once per [`FLUSH_SAMPLES`]: a note due at
/// sample 0 keys at the first boundary, not before.
#[test]
fn the_sample_clock_writes_key_ons_at_flush_boundaries() {
    let mut seq = Sequencer::new(synthetic_seq(), one_tone_bank());
    let mut spu = Spu::new();
    for _ in 0..FLUSH_SAMPLES - 1 {
        seq.tick_sample(&mut spu);
    }
    assert_eq!(keys_written(&spu), 0);
    seq.tick_sample(&mut spu);
    assert_eq!(keys_written(&spu), 1);
}

/// Two note-ons of one (channel, key) in one flush period both key -
/// FUN_80061B24 does not release the first - and the key's note-off
/// releases both.
#[test]
fn a_repeated_key_keys_again_without_releasing_the_first() {
    let mut seq = Sequencer::new(synthetic_seq(), one_tone_bank());
    let mut spu = Spu::new();
    let on = ChannelMessage::NoteOn {
        key: 60,
        velocity: 100,
    };
    seq.fire_channel(&mut spu, 0, on);
    seq.fire_channel(&mut spu, 0, on);
    seq.flush_key_ons(&mut spu);
    assert_eq!(keys_written(&spu), 2);
    assert_eq!(seq.active.len(), 2);
    seq.fire_channel(
        &mut spu,
        0,
        ChannelMessage::NoteOff {
            key: 60,
            velocity: 0,
        },
    );
    assert!(seq.active.is_empty());
}

/// The retail pump fires events on the vsync grid and retunes the vsync
/// a tempo meta lands in: at 120 BPM / ppqn 480 a quarter (480 ticks) is
/// 30 vsyncs at the 160-tenths budget, so a note 480 ticks in fires at
/// the end of vsync 30, not on its exact sample.
#[test]
fn the_retail_pump_fires_on_the_vsync_grid() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&SEQ_MAGIC);
    buf.extend_from_slice(&[0x00, 0x01]);
    buf.extend_from_slice(&[0x01, 0xE0]); // ppqn 480
    buf.extend_from_slice(&[0x07, 0xA1, 0x20]); // 500000 us/qn
    buf.push(0x04);
    buf.push(0x02);
    buf.extend_from_slice(&[0x00, 0xC0, 0x00]); // delta 0 ProgramChange
    buf.extend_from_slice(&[0x83, 0x60, 0x90, 60, 100]); // delta 480 NoteOn
    buf.extend_from_slice(&[0x00, 0xFF, 0x2F]); // EOT
    let seq = Seq::parse(&buf).unwrap();
    let mut s = Sequencer::new(seq, empty_bank());
    let mut spu = Spu::new();
    for _ in 0..FLUSH_SAMPLES * 29 {
        s.tick_sample(&mut spu);
    }
    assert_eq!(s.next, 1, "29 vsyncs spend 464 of the 480 ticks");
    for _ in 0..FLUSH_SAMPLES {
        s.tick_sample(&mut spu);
    }
    assert_eq!(s.next, 3, "the 30th vsync reaches the note (and the EOT)");
}

/// A one-tone bank whose tone bends `pbmin` semitones down and `pbmax` up.
fn bending_bank(pbmin: u8, pbmax: u8) -> VabBank {
    let mut bank = one_tone_bank();
    bank.programs[0].tones[0].pbmin = pbmin;
    bank.programs[0].tones[0].pbmax = pbmax;
    bank
}

fn note(key: u8, velocity: u8) -> ChannelMessage {
    ChannelMessage::NoteOn { key, velocity }
}

/// The wheel's value in the stream's 14-bit form (operand bytes `lsb`, `msb`).
fn wheel(msb: u16) -> ChannelMessage {
    ChannelMessage::PitchBend { value: msb << 7 }
}

/// `FUN_8006166C` stores nothing: track 2026 glides channel 0 down to `0x0F` and
/// never recentres, and every later note on the channel still plays at its own
/// pitch (`party_battle_gobu_gobu`: `0x80E`, where a held wheel gave `0x4C2`).
#[test]
fn a_bend_does_not_reach_the_next_note() {
    let mut seq = Sequencer::new(synthetic_seq(), bending_bank(12, 12));
    let mut spu = Spu::new();
    seq.fire_channel(&mut spu, 0, note(60, 100));
    seq.flush_key_ons(&mut spu);
    let v = seq.active[0].voice as usize;
    let own = spu.voices[v].pitch;
    seq.fire_channel(&mut spu, 0, wheel(0x0F));
    assert!(spu.voices[v].pitch < own, "the sounding note bends");

    seq.fire_channel(&mut spu, 0, note(60, 0));
    seq.fire_channel(&mut spu, 0, note(60, 100));
    seq.flush_key_ons(&mut spu);
    let v = seq.active[0].voice as usize;
    assert_eq!(spu.voices[v].pitch, own);
}

/// `FUN_80067A1C` tests the voice's driver record only: the sequence, the VAB
/// and the program. Not the channel, and not whether the note is still held.
#[test]
fn a_bend_reaches_every_voice_on_the_program_held_or_released() {
    let mut seq = Sequencer::new(synthetic_seq(), bending_bank(2, 2));
    let mut spu = Spu::new();
    // Channels default to program 0, so these two share one.
    seq.fire_channel(&mut spu, 0, note(60, 100));
    seq.fire_channel(&mut spu, 1, note(60, 100));
    seq.flush_key_ons(&mut spu);
    let (a, b) = (seq.active[0].voice as usize, seq.active[1].voice as usize);
    let own = spu.voices[a].pitch;
    seq.fire_channel(&mut spu, 1, note(60, 0));

    seq.fire_channel(&mut spu, 0, wheel(0x7F));
    assert!(spu.voices[a].pitch > own);
    assert_eq!(
        spu.voices[b].pitch, spu.voices[a].pitch,
        "released, other channel"
    );

    seq.fire_channel(&mut spu, 0, wheel(0x40));
    assert_eq!(
        spu.voices[a].pitch, own,
        "the wheel's centre is the note itself"
    );
}

/// A cue re-keyed the voice: its record names sequence `0x21`, so no bend finds it.
#[test]
fn a_bend_leaves_a_voice_a_cue_took() {
    let mut seq = Sequencer::new(synthetic_seq(), bending_bank(2, 2));
    let mut spu = Spu::new();
    seq.fire_channel(&mut spu, 0, note(60, 100));
    seq.flush_key_ons(&mut spu);
    let v = seq.active[0].voice as usize;
    spu.key_on_mask(1 << v);
    spu.voices[v].pitch = 0x123;
    seq.fire_channel(&mut spu, 0, wheel(0x7F));
    assert_eq!(spu.voices[v].pitch, 0x123);
}

/// The retail handler writes the shadow pitch register the flush hands over, so
/// a bend between a note's staging and its flush keys the note bent.
#[test]
fn a_bend_before_the_flush_keys_the_note_bent() {
    let mut seq = Sequencer::new(synthetic_seq(), bending_bank(2, 2));
    let mut spu = Spu::new();
    seq.fire_channel(&mut spu, 0, note(60, 100));
    seq.fire_channel(&mut spu, 0, wheel(0x7F));
    seq.flush_key_ons(&mut spu);
    let v = seq.active[0].voice as usize;
    assert!(spu.voices[v].pitch > 0x1000);
}

/// `FUN_80026740` sets the sequence's volume to `0` before the key-off: the
/// notes cut, and their release runs unheard. The sequence's own volume stands
/// for the next attach.
#[test]
fn a_pause_cuts_the_sounding_notes() {
    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    let mut spu = Spu::new();
    seq.set_master_vol(107);
    occupy(&mut seq, 2);
    spu.voices[2].vol_left = 0x2000;
    spu.voices[2].vol_right = 0x2000;
    spu.voices[2].adsr.level = 0x4000;
    spu.voices[2].adsr.phase = crate::spu::adsr::Phase::Sustain;
    seq.voice_note[2] = record(0, 60, 0);
    // A tail already releasing is in the walk too.
    spu.voices[3].vol_left = 0x2000;
    spu.voices[3].adsr.level = 0x4000;
    spu.voices[3].adsr.phase = crate::spu::adsr::Phase::Release;
    seq.voice_note[3] = record(0, 61, 0);
    seq.pause(&mut spu);
    assert_eq!((spu.voices[2].vol_left, spu.voices[2].vol_right), (0, 0));
    assert_eq!(spu.voices[3].vol_left, 0);
    assert_eq!(spu.voices[2].adsr.phase, crate::spu::adsr::Phase::Release);
    assert_eq!(seq.master_vol(), 107);
}
