//! Unit tests for the audio trace oracle.

use super::*;

fn voice(active: bool, start_addr: Option<u32>) -> VoiceTraceFrame {
    VoiceTraceFrame {
        active,
        start_addr,
        ..Default::default()
    }
}

fn voice_at(pitch: u16, tone: u32) -> VoiceTraceFrame {
    VoiceTraceFrame {
        active: true,
        pitch: Some(pitch),
        adsr_control: Some(tone),
        ..Default::default()
    }
}

fn alloc_frame(voices: Vec<VoiceTraceFrame>) -> AudioTraceFrame {
    let mut mask = 0u32;
    for (i, v) in voices.iter().enumerate() {
        if v.active {
            mask |= 1 << i;
        }
    }
    AudioTraceFrame {
        active_voice_mask: mask,
        voices,
        ..AudioTraceFrame::quiescent(0)
    }
}

/// Disjoint pitch sets read as "different notes" however close the
/// voice counts are - the reading that a same-count comparison of two
/// different tracks would otherwise pass off as convergence.
#[test]
fn voice_allocation_disjoint_pitches_read_as_different_notes() {
    let engine = vec![alloc_frame(vec![voice_at(1000, 1), voice_at(1100, 1)])];
    let retail = vec![alloc_frame(vec![voice_at(500, 1), voice_at(600, 1)])];
    let c = compare_voice_allocation(&engine, &retail);
    assert_eq!(c.shared_pitches, 0);
    assert_eq!(c.verdict, VoiceAllocationVerdict::DifferentNotes);
}

/// Overlapping pitches with the engine sounding far fewer at once is
/// the "short of voices" reading.
#[test]
fn voice_allocation_fewer_overlapping_notes_reads_as_short() {
    let engine = vec![alloc_frame(vec![voice_at(500, 1)])];
    let retail = vec![alloc_frame(vec![
        voice_at(500, 1),
        voice_at(500, 1),
        voice_at(500, 1),
        voice_at(600, 1),
    ])];
    let c = compare_voice_allocation(&engine, &retail);
    assert_eq!(c.shared_pitches, 1);
    assert_eq!(c.verdict, VoiceAllocationVerdict::ShortOfVoices);
}

/// Retail doubling a `(sample, pitch)` pair across two slots shows up
/// as a doubling factor above one; the engine playing each note once
/// sits at exactly one.
#[test]
fn voice_allocation_doubling_factor_counts_duplicate_slots() {
    let retail = vec![alloc_frame(vec![voice_at(500, 1), voice_at(500, 1)])];
    let engine = vec![alloc_frame(vec![voice_at(500, 1)])];
    let c = compare_voice_allocation(&engine, &retail);
    assert!((c.retail.doubling - 2.0).abs() < 1e-9);
    assert!((c.engine.doubling - 1.0).abs() < 1e-9);
}

/// A slot that stays sounding across frames is one key-on, not one per
/// frame - the statistic counts edges, which is what makes it a
/// property of the score rather than of how long a voice rings.
#[test]
fn onsets_count_key_on_edges_not_sounding_frames() {
    let on = vec![voice_at(500, 1)];
    let off = vec![VoiceTraceFrame::default()];
    let frames = vec![
        alloc_frame(on.clone()),
        alloc_frame(on.clone()),
        alloc_frame(on.clone()),
        alloc_frame(off.clone()),
        alloc_frame(on.clone()),
    ];
    let s = summarise_allocation(&frames);
    assert_eq!(s.onsets, 2, "two key-ons, four sounding frames");
    assert!((s.onsets_per_frame - 0.4).abs() < 1e-9);
}

/// Two sides playing the same phrase at the same rate agree on key-ons
/// per frame even when one holds each note far longer - which is the
/// shape a capture whose frames carry extra envelope time produces.
#[test]
fn onset_ratio_is_blind_to_how_long_a_note_rings() {
    // Engine: one key-on, sounding for four frames.
    let engine = vec![
        alloc_frame(vec![voice_at(500, 1)]),
        alloc_frame(vec![voice_at(500, 1)]),
        alloc_frame(vec![voice_at(500, 1)]),
        alloc_frame(vec![voice_at(500, 1)]),
    ];
    // Retail: the same one key-on, drained after a single frame.
    let retail = vec![
        alloc_frame(vec![voice_at(500, 1)]),
        alloc_frame(vec![VoiceTraceFrame::default()]),
        alloc_frame(vec![VoiceTraceFrame::default()]),
        alloc_frame(vec![VoiceTraceFrame::default()]),
    ];
    let c = compare_voice_allocation(&engine, &retail);
    assert_eq!(c.engine.onsets, 1);
    assert_eq!(c.retail.onsets, 1);
    assert!((c.onset_ratio - 1.0).abs() < 1e-9);
    // The count statistic, by contrast, reads 4x.
    assert!((c.engine.mean_active / c.retail.mean_active - 1.0).abs() < 1e-9);
}

/// The retail window is matched against the engine frames that play the
/// same notes, not against the engine trace's opening bar.
#[test]
fn alignment_finds_the_engine_window_playing_the_same_notes() {
    let quiet = alloc_frame(vec![voice_at(100, 1)]);
    let phrase = [
        alloc_frame(vec![voice_at(700, 1), voice_at(800, 1)]),
        alloc_frame(vec![voice_at(900, 1)]),
    ];
    let mut engine = vec![quiet.clone(), quiet.clone(), quiet.clone()];
    engine.extend(phrase.iter().cloned());
    engine.push(quiet.clone());
    let retail = phrase.to_vec();
    let (off, score) = best_alignment_offset(&engine, &retail).expect("alignable");
    assert_eq!(off, 3);
    assert!(score > 0.9, "exact phrase match, got {score}");
    let c = compare_voice_allocation_aligned(&engine, &retail);
    assert_eq!(c.alignment_offset, Some(3));
    assert_eq!(c.verdict, VoiceAllocationVerdict::Comparable);
}

/// An engine window that simply sounds *more* voices must not outscore
/// the window whose notes actually match: the score is symmetric, so a
/// busy window carrying none of retail's pitches scores zero.
#[test]
fn alignment_score_is_not_won_by_the_busiest_engine_window() {
    let busy = alloc_frame(vec![
        voice_at(10, 1),
        voice_at(20, 1),
        voice_at(30, 1),
        voice_at(40, 1),
        voice_at(50, 1),
    ]);
    let match_frame = alloc_frame(vec![voice_at(700, 1)]);
    let engine = vec![busy.clone(), busy.clone(), match_frame.clone()];
    let retail = vec![match_frame];
    let (off, _) = best_alignment_offset(&engine, &retail).expect("alignable");
    assert_eq!(off, 2);
}

fn frame_with(mask: u32, voices: Vec<VoiceTraceFrame>) -> AudioTraceFrame {
    AudioTraceFrame {
        active_voice_mask: mask,
        voices,
        ..AudioTraceFrame::quiescent(0)
    }
}

#[test]
fn jsonl_roundtrip_engine_shape() {
    let frames = vec![
        AudioTraceFrame {
            sequencer_playhead_ticks: Some(0),
            sequencer_finished: Some(false),
            master_volume: Some((0x3FFF, 0x3FFF)),
            reverb_mode: Some(0),
            reverb_eon: Some(0x00FF_FFFF),
            reverb_depth: Some((0x3264, 0x3264)),
            reverb_work_area: Some(0x7_9020),
            active_voice_mask: 0b0000_0011,
            voices: vec![
                voice(true, Some(0x1000)),
                voice(true, Some(0x1200)),
                voice(false, None),
            ],
            ..AudioTraceFrame::quiescent(0)
        },
        AudioTraceFrame {
            sequencer_playhead_ticks: Some(480),
            sequencer_finished: Some(false),
            master_volume: Some((0x3FFF, 0x3FFF)),
            reverb_mode: Some(0),
            reverb_eon: Some(0x00FF_FFFF),
            reverb_depth: Some((0x3264, 0x3264)),
            reverb_work_area: Some(0x7_9020),
            active_voice_mask: 0b0000_0010,
            voices: vec![
                voice(false, Some(0x1000)),
                voice(true, Some(0x1200)),
                voice(false, None),
            ],
            ..AudioTraceFrame::quiescent(1)
        },
    ];
    let jsonl = audio_trace_to_jsonl(&frames);
    assert_eq!(jsonl.lines().count(), 2);
    let round = parse_audio_trace_jsonl(&jsonl).unwrap();
    assert_eq!(frames, round);
}

#[test]
fn jsonl_roundtrip_retail_shape() {
    // Retail shape: no mode number (hardware keeps none), but the
    // routing mask, depth and control word a capture does carry.
    let f = AudioTraceFrame {
        master_volume: Some((0x3F00, 0x3F00)),
        reverb_eon: Some(0x17FFFF),
        reverb_depth: Some((0x3264, 0x3264)),
        spu_control: Some(0xC081),
        active_voice_mask: 0b0000_0111,
        voices: vec![voice(true, Some(0x2000)); 3],
        ..AudioTraceFrame::quiescent(0)
    };
    let jsonl = audio_trace_to_jsonl(std::slice::from_ref(&f));
    let round = parse_audio_trace_jsonl(&jsonl).unwrap();
    assert_eq!(round, vec![f]);
}

#[test]
fn quiescent_emits_no_active_voices() {
    let q = AudioTraceFrame::quiescent(42);
    assert_eq!(q.frame, 42);
    assert_eq!(q.active_voice_mask, 0);
    assert_eq!(q.voices.len(), NUM_VOICES);
    assert!(q.voices.iter().all(|v| !v.active));
}

#[test]
fn divergence_none_when_retail_has_no_active_voices() {
    // Retail quiescent → trivially matches any engine trace.
    let engine = vec![frame_with(0b0000_0001, vec![voice(true, Some(0x1000))])];
    let retail = frame_with(0, vec![voice(false, None)]);
    assert!(first_audio_trace_divergence(&engine, &retail).is_none());
}

#[test]
fn divergence_none_when_engine_superset_matches() {
    // Retail had voice 1 active; engine had voices 0+1 active. Superset
    // → no divergence.
    let engine = vec![frame_with(
        0b0000_0011,
        vec![voice(true, Some(0x1000)), voice(true, Some(0x1200))],
    )];
    let retail = frame_with(
        0b0000_0010,
        vec![voice(false, None), voice(true, Some(0x1200))],
    );
    assert!(first_audio_trace_divergence(&engine, &retail).is_none());
}

#[test]
fn divergence_no_frame_matched_when_engine_missing_voices() {
    let engine = vec![frame_with(0b0000_0001, vec![voice(true, Some(0x1000))])];
    let retail = frame_with(
        0b0000_0010,
        vec![voice(false, None), voice(true, Some(0x1200))],
    );
    let d = first_audio_trace_divergence(&engine, &retail).unwrap();
    assert_eq!(d.kind, AudioDivergenceKind::NoFrameMatched);
}

#[test]
fn divergence_voice_start_addr_mismatch() {
    // Engine has the right voice active but at a different start_addr.
    let engine = vec![frame_with(
        0b0000_0010,
        vec![voice(false, None), voice(true, Some(0xDEAD))],
    )];
    let retail = frame_with(
        0b0000_0010,
        vec![voice(false, None), voice(true, Some(0x1200))],
    );
    let d = first_audio_trace_divergence(&engine, &retail).unwrap();
    assert_eq!(d.kind, AudioDivergenceKind::VoiceStartAddrMismatch);
}

#[test]
fn divergence_none_when_retail_start_addr_unknown() {
    // Retail's voice is active but start_addr field is None. We don't
    // penalise the engine for filling it in.
    let engine = vec![frame_with(0b0000_0001, vec![voice(true, Some(0x1000))])];
    let retail = frame_with(0b0000_0001, vec![voice(true, None)]);
    assert!(first_audio_trace_divergence(&engine, &retail).is_none());
}

#[test]
fn empty_engine_trace_returns_none() {
    let retail = frame_with(0b0000_0001, vec![voice(true, Some(0x1000))]);
    assert!(first_audio_trace_divergence(&[], &retail).is_none());
}

#[test]
fn parser_skips_blank_lines() {
    let s = "\n{\"frame\":0,\"active_voice_mask\":0,\"voices\":[]}\n\n";
    let out = parse_audio_trace_jsonl(s).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].active_voice_mask, 0);
}

/// Trace director without a bank silently drops `start` - the warning
/// goes through `log::warn` (no panic). Pre-bank-stage races at scene
/// load should not crash the trace loop.
#[test]
fn trace_director_start_without_bank_is_noop() {
    let mut d = TraceBgmDirector::new();
    d.start(0, &[]);
    assert!(d.sequencer().is_none());
    assert!(!d.is_playing());
    assert_eq!(d.last_started, None);
}

/// Pause + resume toggle without touching the sequencer slot. The
/// trace loop reads `is_paused()` to gate the per-frame `tick_us`.
#[test]
fn trace_director_pause_resume_toggle() {
    let mut d = TraceBgmDirector::new();
    BgmDirector::pause(&mut d);
    assert!(d.is_paused());
    BgmDirector::resume(&mut d);
    assert!(!d.is_paused());
}

/// `stop` clears the sequencer slot, paused flag, and last_started id.
/// Idempotent on an empty director.
#[test]
fn trace_director_stop_clears_state() {
    let mut d = TraceBgmDirector::new();
    d.paused = true;
    d.last_started = Some(7);
    BgmDirector::stop(&mut d);
    assert!(d.sequencer().is_none());
    assert!(!d.is_paused());
    assert_eq!(d.last_started, None);
}

/// Multi-frame divergence walk skips retail frames with no active
/// voices. Three retail frames quiet → no divergence.
#[test]
fn divergence_multi_skips_quiet_retail_frames() {
    let engine = vec![frame_with(0, vec![voice(false, None)])];
    let retail = vec![
        frame_with(0, vec![voice(false, None)]),
        frame_with(0, vec![voice(false, None)]),
        frame_with(0, vec![voice(false, None)]),
    ];
    assert!(first_audio_trace_divergence_multi(&engine, &retail).is_none());
}

/// Multi-frame divergence: if any single retail frame fails the
/// single-frame convergence rule, the multi-frame walk reports it.
#[test]
fn divergence_multi_reports_first_failing_retail_frame() {
    let engine = vec![frame_with(
        0b0000_0010,
        vec![voice(false, None), voice(true, Some(0x1200))],
    )];
    // Three retail frames: first quiet, second matched, third diverges.
    let retail = vec![
        frame_with(0, vec![voice(false, None)]),
        frame_with(
            0b0000_0010,
            vec![voice(false, None), voice(true, Some(0x1200))],
        ),
        // Retail asks for voice 0 active, but engine never had it.
        frame_with(0b0000_0001, vec![voice(true, Some(0xC0DE))]),
    ];
    let d = first_audio_trace_divergence_multi(&engine, &retail).unwrap();
    assert_eq!(d.kind, AudioDivergenceKind::NoFrameMatched);
}

/// Multi-frame divergence handles an empty engine trace the same way
/// the single-frame walk does: returns None (nothing to compare).
#[test]
fn divergence_multi_empty_engine_returns_none() {
    let retail = vec![frame_with(0b0000_0001, vec![voice(true, Some(0x1000))])];
    assert!(first_audio_trace_divergence_multi(&[], &retail).is_none());
}

/// A start without a staged bank consumes the request and attaches
/// nothing - and there is no second, deferred way in. Field-VM op `0x35`
/// sub-op 9 routes here too (it is a start behind a load barrier this
/// host never waits on), so the trace director models one entry point,
/// not two.
#[test]
fn trace_director_start_without_bank_attaches_nothing() {
    let mut d = TraceBgmDirector::new();
    BgmDirector::start(&mut d, 42, &[1, 2, 3]);
    assert!(d.sequencer().is_none());
    assert!(!d.is_playing());
}

fn keyed_frame(counts: &[u32]) -> AudioTraceFrame {
    alloc_frame(
        counts
            .iter()
            .map(|&k| VoiceTraceFrame {
                key_ons: Some(k),
                ..Default::default()
            })
            .collect(),
    )
}

/// A re-key of a voice that never went silent is invisible to the
/// sounding-edge count and visible to the exact counter.
#[test]
fn exact_key_on_counts_see_a_rekey_of_a_sounding_voice() {
    let mut a = keyed_frame(&[3, 0]);
    a.voices[0].active = true;
    let mut b = keyed_frame(&[4, 0]);
    b.voices[0].active = true;
    let c = keyed_frame(&[4, 2]);
    assert_eq!(engine_key_on_counts(&[a, b, c]), Some(vec![0, 1, 2]));
    // A frame without the counter (a retail trace) yields no counts.
    assert_eq!(
        engine_key_on_counts(&[alloc_frame(vec![voice(true, None)])]),
        None
    );
}

#[test]
fn keyon_census_csv_counts_kon_mask_bits_per_vsync() {
    let csv = "vsync,mode,mask,ra,records\n\
               -1,alloc,01,80066590,win=1 vmax=24 prior=0\n\
               1,0,000001,80065F5C,\n\
               1,1,000006,80065F7C,1:9/0/1/0002 2:7/1/1/0002\n\
               1,note,0001,80061BC0,vab=1 prog=3 key=60 vel=100\n\
               3,1,000C00,80065F7C,10:8/0/1/0002 11:5/0/1/0002\n\
               4,1,C00000,80065F7C,22:1/3/3/0021 23:1/2/3/0021\n\
               4,note,0021,80065100,vab=3 prog=1 key=60 vel=100\n\
               5,1,000003,80065F7C,\n\
               60,clock,0,0,ms=1.0\n";
    // Vsync 4 keys two SFX-cue voices (owner 0x21), which the music-only
    // engine trace never plays; vsync 5 carries no records and counts
    // its mask. Only the score note on vsync 1 is a note.
    let c = parse_keyon_census_csv(csv).unwrap();
    assert_eq!(c.key_ons, vec![0, 2, 0, 2, 0, 2]);
    assert_eq!(c.notes, vec![0, 1, 0, 0, 0, 0]);
}

/// The census aligns on key-on timing, tolerates a one-frame skew, and
/// compares exact totals over the aligned window.
#[test]
fn keyon_census_aligns_on_timing_and_compares_totals() {
    // Engine: cumulative per-voice counters; key-ons land at frames
    // 5 (x3) and 8 (x1) and 12 (x2).
    let per_frame = [0u32, 0, 0, 0, 0, 3, 0, 0, 1, 0, 0, 0, 2, 0];
    let mut cum = 0u32;
    let engine: Vec<_> = per_frame
        .iter()
        .map(|k| {
            cum += k;
            keyed_frame(&[cum])
        })
        .collect();
    // Retail census: the same rhythm from its vsync 0, the second beat
    // quantised one vsync late.
    let census = KeyOnCensus {
        key_ons: vec![3, 0, 0, 0, 1, 0, 0, 2],
        notes: Vec::new(),
    };
    let c = compare_key_on_census(&engine, &census).expect("alignable");
    assert_eq!(c.offset, 5);
    assert_eq!(c.matched, 6);
    assert_eq!(c.retail_total, 6);
    assert_eq!(c.engine_total, 6);
    assert!((c.ratio - 1.0).abs() < 1e-9);
}

/// A retail KON that trails its note by up to three vsyncs (a flush
/// deferred behind a busy SPU transfer) still matches the engine key-on
/// written at the note's own vsync.
#[test]
fn keyon_census_matches_a_deferred_retail_kon() {
    let per_frame = [
        0u32, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0,
    ];
    let mut cum = 0u32;
    let engine: Vec<_> = per_frame
        .iter()
        .map(|k| {
            cum += k;
            keyed_frame(&[cum])
        })
        .collect();
    // Notes at vsyncs 0, 6, 12; KONs written at 3, 7, 15.
    let mut notes = vec![0u32; 16];
    let mut key_ons = vec![0u32; 16];
    for v in [0, 6, 12] {
        notes[v] = 1;
    }
    for v in [3, 7, 15] {
        key_ons[v] = 1;
    }
    let c = compare_key_on_census(&engine, &KeyOnCensus { key_ons, notes }).expect("alignable");
    assert_eq!(c.matched, 3);
    assert_eq!((c.engine_total, c.retail_total), (3, 3));
}

/// A census captured before its track starts is compared from its first
/// score activity, not from vsync 0.
#[test]
fn keyon_census_crops_leading_silence() {
    let per_frame = [0u32, 2, 0, 0, 1, 0, 0, 0];
    let mut cum = 0u32;
    let engine: Vec<_> = per_frame
        .iter()
        .map(|k| {
            cum += k;
            keyed_frame(&[cum])
        })
        .collect();
    let mut key_ons = vec![0u32; 12];
    key_ons[8] = 2;
    key_ons[11] = 1;
    let c = compare_key_on_census(
        &engine,
        &KeyOnCensus {
            key_ons,
            notes: Vec::new(),
        },
    )
    .expect("alignable");
    assert_eq!(c.window, 8..12);
    assert_eq!(c.offset, 1);
    assert_eq!((c.engine_total, c.retail_total), (3, 3));
}
