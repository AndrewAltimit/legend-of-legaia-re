use super::*;
use crate::Spu;
use legaia_seq::ChannelMessage;
use legaia_seq::SEQ_MAGIC;

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
    for v in 0..24u8 {
        seq.active.push(ActiveNote {
            channel: 0,
            key: 60 + v,
            voice: v,
            base_pitch: 0x1000,
            bend_range: (2, 2),
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
        key: 60,
        voice: 23,
        base_pitch: 0x1000,
        bend_range: (0, 0),
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

/// Occupy a voice with a synthetic active note so the allocator treats it
/// as reserved (the engine stand-in for the retail in-use marker).
fn occupy(seq: &mut Sequencer, voice: u8) {
    seq.active.push(ActiveNote {
        channel: 0,
        key: 60 + voice,
        voice,
        base_pitch: 0x1000,
        bend_range: (2, 2),
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
    spu.voices[0].adsr.level = 0x1000;
    spu.voices[0].adsr.phase = crate::spu::adsr::Phase::Release;
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
        spu.voices[v as usize].adsr.level = 0x4000;
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
        spu.voices[v as usize].adsr.level = 0x4000;
    }
    // All equal priority (= request). Voice 7 is quietest.
    spu.voices[7].adsr.level = 0x0100;
    assert_eq!(seq.alloc_voice(&mut spu, 0), Some(7));

    // Re-occupy 7 (alloc freed it) and equalize envelopes; voice 11 is
    // now the least-recently allocated (largest age).
    occupy(&mut seq, 7);
    for v in 0..24 {
        spu.voices[v as usize].adsr.level = 0x4000;
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
        spu.voices[v as usize].adsr.level = 0x4000;
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
        spu.voices[v as usize].adsr.level = 0x4000;
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
    // idx 3: delta 480, NoteOn ch0 key 64 vel 100 (the loop body start)
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
        s.next, 3,
        "Loop Forever rewinds to the in-stream marker (idx 3), not to 0"
    );

    // Exactness across the rewind: the looped NoteOn at idx 3 has delta
    // 480 = 22050 samples. It must not re-fire one sample early.
    for _ in 0..22_049 {
        s.tick_sample(&mut spu);
    }
    assert_eq!(s.next, 3, "looped NoteOn must not fire early after rewind");
    s.tick_sample(&mut spu);
    assert!(
        s.next >= 4,
        "looped NoteOn fires on the exact sample after rewind"
    );

    // And it keeps looping: drive well past another Loop Forever and the
    // playhead is still parked at the marker, never finished.
    for _ in 0..66_150 {
        s.tick_sample(&mut spu);
    }
    assert!(!s.is_finished());
    assert_eq!(s.next, 3, "repeated loops keep returning to the marker");
}

/// When the stream carries a Loop Start marker, end-of-track rewinds to it
/// even if no external [`Sequencer::set_loop_to`] fallback was set.
#[test]
fn eot_rewinds_to_marker_without_external_loop() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&SEQ_MAGIC);
    buf.extend_from_slice(&[0x00, 0x01]);
    buf.extend_from_slice(&[0x01, 0xE0]);
    buf.extend_from_slice(&[0x07, 0xA1, 0x20]);
    buf.push(0x04);
    buf.push(0x02);
    // idx 0: delta 0, Loop Start
    buf.push(0x00);
    buf.push(0xB0);
    buf.push(99);
    buf.push(20);
    // idx 1: delta 480, NoteOn
    buf.push(0x83);
    buf.push(0x60);
    buf.push(0x90);
    buf.push(60);
    buf.push(100);
    // idx 2: delta 0, end-of-track
    buf.push(0x00);
    buf.push(0xFF);
    buf.push(0x2F);
    let seq = Seq::parse(&buf).unwrap();
    let mut s = Sequencer::new(seq, empty_bank());
    // Note: set_loop_to is NOT called - looping is driven purely by the
    // in-stream marker.
    let mut spu = Spu::new();
    for _ in 0..40_000 {
        s.tick_sample(&mut spu);
    }
    assert!(
        !s.is_finished(),
        "in-stream marker loops without set_loop_to"
    );
    // Loop Start recorded idx 1; EOT rewound there rather than finishing.
    assert_eq!(s.loop_start, Some(1));
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
fn pitch_bend_factor_center_is_unity() {
    // Center wheel = no bend, whatever the tone range.
    assert!((pitch_bend_factor(PITCH_BEND_CENTER, 2, 2) - 1.0).abs() < 1e-9);
    // With a ±2-semitone tone range, full up bends sharp, full down flat.
    assert!(pitch_bend_factor(0x3FFF, 2, 2) > 1.0);
    assert!(pitch_bend_factor(0x0000, 2, 2) < 1.0);
    // A tone with a (0, 0) range never responds to the wheel.
    assert!((pitch_bend_factor(0x3FFF, 0, 0) - 1.0).abs() < 1e-9);
    assert!((pitch_bend_factor(0x0000, 0, 0) - 1.0).abs() < 1e-9);
    // Asymmetric range: full up uses `up`, full down uses `down`.
    let up = pitch_bend_factor(0x3FFF, 1, 12);
    let down = pitch_bend_factor(0x0000, 1, 12);
    assert!(up > 1.5); // ~+1 octave
    assert!(down > 0.94 && down < 1.0); // ~-1 semitone
}

#[test]
fn bend_pitch_clamps_to_register_range() {
    // A large base × a sharp bend saturates at 0x3FFF, never overflows.
    assert_eq!(
        bend_pitch(0x3FFF, pitch_bend_factor(0x3FFF, 12, 12)),
        0x3FFF
    );
    // A bend never drives the register below 1.
    assert!(bend_pitch(1, pitch_bend_factor(0x0000, 12, 12)) >= 1);
    // Center leaves a mid-range pitch untouched.
    assert_eq!(
        bend_pitch(0x1000, pitch_bend_factor(PITCH_BEND_CENTER, 2, 2)),
        0x1000
    );
}

#[test]
fn pitch_bend_event_repitches_sounding_voice() {
    let mut spu = Spu::new();
    let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
    // Seed a sounding note on channel 0, voice 0 at a known base pitch.
    let base = 0x1000u16;
    spu.voices[0].pitch = base;
    seq.active.push(ActiveNote {
        channel: 0,
        key: 60,
        voice: 0,
        base_pitch: base,
        bend_range: (2, 2),
        velocity: 127,
        program: 0,
        tone: 0,
        key_on_stamp: 0,
        pending: None,
    });

    // Bend sharp: the voice's live pitch register rises, the channel
    // state records the wheel, and the stored base is untouched.
    seq.fire_channel(&mut spu, 0, ChannelMessage::PitchBend { value: 0x3FFF });
    assert_eq!(seq.channels[0].pitch_bend, 0x3FFF);
    assert!(spu.voices[0].pitch > base);
    assert_eq!(seq.active[0].base_pitch, base);

    // Return to center: the voice snaps back to exactly the base pitch
    // (re-bending the base, not the already-bent value).
    seq.fire_channel(
        &mut spu,
        0,
        ChannelMessage::PitchBend {
            value: PITCH_BEND_CENTER,
        },
    );
    assert_eq!(spu.voices[0].pitch, base);
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
    // Pan hard left: the right side is silenced, the left keeps its level.
    seq.fire_channel(
        &mut spu,
        0,
        ChannelMessage::ControlChange {
            control: 0x0A,
            value: 0,
        },
    );
    assert_eq!(seq.channels[0].pan, 0);
    assert_eq!(spu.voices[v].vol_right, 0);
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
fn layer_tone(vag: i16, min: u8, max: u8, center: u8) -> legaia_vab::VagAtr {
    legaia_vab::VagAtr {
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
