//! Voice-allocation comparison: per-trace allocation statistics, the best
//! frame alignment between two traces, and the verdict on how the engine's
//! voice use differs from retail's.

use super::*;

/// Per-side summary of what a trace's *sounding* voices were doing, in the
/// two currencies that survive the two sides' independent SPU-RAM
/// allocators: pitch (a hardware register computed from note + tone, so
/// directly comparable) and the packed ADSR config word (the closest thing
/// the SPU keeps to "which tone programmed this voice").
///
/// `start_addr` is deliberately **not** a comparand here - retail's samples
/// sit wherever `SsSpuMalloc` put them and the engine's wherever
/// `SpuAllocator` put them, so the addresses cannot agree and are not
/// supposed to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VoiceAllocationStats {
    /// Frames the summary was computed over.
    pub frames: usize,
    /// Mean sounding voices per frame.
    pub mean_active: f64,
    /// Largest number of voices sounding on any one frame.
    pub max_active: usize,
    /// Mean count of distinct `(start_addr, pitch)` pairs per frame - the
    /// number of distinct *notes* sounding, as opposed to slots used.
    pub mean_distinct_notes: f64,
    /// `mean_active / mean_distinct_notes`: how many slots the side spends
    /// per distinct note. Exactly `1.0` means it never doubles a note
    /// across two voices.
    pub doubling: f64,
    /// Key-on **edges**: per voice slot, transitions from not-sounding to
    /// sounding across the window. This is the one activity statistic that
    /// is a property of the emulated CPU on both sides - the sequencer
    /// writes the key-on register from the game's own vsync handler - so it
    /// survives a retail capture whose frames carry an uncontrolled amount
    /// of SPU time (see [`crate::audio_trace_oracle`] docs and
    /// `docs/subsystems/audio.md`, "The envelope channel is not on emulated
    /// time").
    pub onsets: usize,
    /// [`Self::onsets`] over [`Self::frames`].
    pub onsets_per_frame: f64,
    /// Every pitch seen on a sounding voice anywhere in the trace.
    pub pitches: std::collections::BTreeSet<u16>,
    /// Every packed ADSR config word seen on a sounding voice.
    pub tones: std::collections::BTreeSet<u32>,
    /// Every sample start address seen on a sounding voice. Reported for
    /// counting distinct instruments, never for equality against the other
    /// side.
    pub samples: std::collections::BTreeSet<u32>,
}

/// Which of the three readings the per-voice comparison supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceAllocationVerdict {
    /// The two sides' pitch sets barely intersect: they are sounding
    /// different notes. On one track that means different moments in it;
    /// across two tracks it means the pairing itself is wrong.
    DifferentNotes,
    /// Overlapping pitches, but the engine sounds materially fewer at once.
    ShortOfVoices,
    /// Overlapping pitches and comparable slot counts.
    Comparable,
}

/// Engine-vs-retail per-voice comparison.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceAllocationComparison {
    pub engine: VoiceAllocationStats,
    pub retail: VoiceAllocationStats,
    /// Pitches both sides sounded at some point in their windows.
    pub shared_pitches: usize,
    /// Packed ADSR config words both sides used.
    pub shared_tones: usize,
    /// Engine key-on edges per frame over retail's. This is the comparand
    /// that survives a capture whose frames carry an uncontrolled amount of
    /// SPU time: both sides' key-ons are written by the score, on the
    /// emulated CPU's own clock, while how long a voice then stays above
    /// zero is the host's SPU thread's business on the retail side.
    pub onset_ratio: f64,
    /// Engine-frame offset the retail window was compared at, when the
    /// comparison was run through [`compare_voice_allocation_aligned`].
    pub alignment_offset: Option<usize>,
    pub verdict: VoiceAllocationVerdict,
}

/// Share of the *smaller* pitch vocabulary the two sides hold in common,
/// below which the windows are judged to be sounding different notes. The
/// denominator is the smaller of the two sets rather than the engine's, so a
/// longer engine window is not penalised for hearing more of the track than
/// the retail freeze did.
pub(super) const PITCH_OVERLAP_FLOOR: f64 = 0.25;
/// Fraction of retail's concurrent-voice count below which the engine is
/// judged short of voices rather than comparable.
pub(super) const VOICE_COUNT_FLOOR: f64 = 0.75;

pub(super) fn summarise_allocation(frames: &[AudioTraceFrame]) -> VoiceAllocationStats {
    use std::collections::BTreeSet;
    let mut out = VoiceAllocationStats {
        frames: frames.len(),
        ..Default::default()
    };
    let (mut act_sum, mut note_sum, mut counted) = (0usize, 0usize, 0usize);
    // Key-on edges per slot. `prev` is indexed by voice slot and carries the
    // previous frame's sounding flag, so a slot that stays sounding across a
    // frame boundary counts once, not once per frame.
    let mut prev = vec![false; NUM_VOICES];
    for f in frames {
        for (i, v) in f.voices.iter().enumerate() {
            if i >= prev.len() {
                prev.resize(i + 1, false);
            }
            if v.active && !prev[i] {
                out.onsets += 1;
            }
            prev[i] = v.active;
        }
        let sounding: Vec<&VoiceTraceFrame> = f.voices.iter().filter(|v| v.active).collect();
        if sounding.is_empty() {
            // A silent frame is not evidence about allocation; averaging it
            // in measures how much of the window the trace spent before the
            // track started, which is a different question.
            continue;
        }
        counted += 1;
        act_sum += sounding.len();
        out.max_active = out.max_active.max(sounding.len());
        let notes: BTreeSet<(Option<u32>, Option<u16>)> =
            sounding.iter().map(|v| (v.start_addr, v.pitch)).collect();
        note_sum += notes.len();
        for v in sounding {
            if let Some(p) = v.pitch {
                out.pitches.insert(p);
            }
            if let Some(t) = v.adsr_control {
                out.tones.insert(t);
            }
            if let Some(a) = v.start_addr {
                out.samples.insert(a);
            }
        }
    }
    if counted > 0 {
        out.mean_active = act_sum as f64 / counted as f64;
        out.mean_distinct_notes = note_sum as f64 / counted as f64;
        out.doubling = if note_sum > 0 {
            act_sum as f64 / note_sum as f64
        } else {
            0.0
        };
    }
    if !frames.is_empty() {
        out.onsets_per_frame = out.onsets as f64 / frames.len() as f64;
    }
    out
}

/// Score how well engine frame `off + i` lines up with retail frame `i`,
/// as the mean per-frame Jaccard of the two frames' sounding-pitch
/// multisets. Symmetric, so a window where the engine simply sounds more
/// voices does not outscore one where the notes actually agree - an
/// intersection-only score ranks the busiest engine window first whatever
/// it is playing.
pub(super) fn alignment_score(
    engine: &[AudioTraceFrame],
    retail: &[AudioTraceFrame],
    off: usize,
) -> f64 {
    use std::collections::BTreeMap;
    fn pitches(f: &AudioTraceFrame) -> BTreeMap<u16, usize> {
        let mut m = BTreeMap::new();
        for v in f.voices.iter().filter(|v| v.active) {
            if let Some(p) = v.pitch {
                *m.entry(p).or_insert(0) += 1;
            }
        }
        m
    }
    let mut sum = 0.0;
    for (i, r) in retail.iter().enumerate() {
        let Some(e) = engine.get(off + i) else {
            return 0.0;
        };
        let (rm, em) = (pitches(r), pitches(e));
        let mut inter = 0usize;
        let mut union = 0usize;
        for k in rm
            .keys()
            .chain(em.keys())
            .collect::<std::collections::BTreeSet<_>>()
        {
            let (a, b) = (
                rm.get(k).copied().unwrap_or(0),
                em.get(k).copied().unwrap_or(0),
            );
            inter += a.min(b);
            union += a.max(b);
        }
        if union > 0 {
            sum += inter as f64 / union as f64;
        }
    }
    sum / retail.len().max(1) as f64
}

/// Find the engine-frame offset at which the retail window best lines up.
///
/// The per-voice comparison is only meaningful when the two windows cover
/// the *same stretch* of the same piece. An engine trace starts its track at
/// tick 0; a retail capture is parked wherever the playthrough left it, so
/// the naive "first N frames of each" pairing compares two different bars
/// and reports the difference between them as an engine difference. Returns
/// `None` when the engine trace is shorter than the retail window.
pub fn best_alignment_offset(
    engine: &[AudioTraceFrame],
    retail: &[AudioTraceFrame],
) -> Option<(usize, f64)> {
    if retail.is_empty() || engine.len() < retail.len() {
        return None;
    }
    (0..=engine.len() - retail.len())
        .map(|off| (off, alignment_score(engine, retail, off)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// Compare what the engine's score allocated against retail's, per voice.
///
/// This is the axis [`first_audio_trace_divergence_multi`] cannot decide:
/// that walk asks whether some engine frame's mask *covers* a retail frame's,
/// which a pair of traces taken at different moments of a 2-minute track can
/// fail while playing identically. The comparison here is of what the voices
/// were *doing* - pitches, tones, and how many slots each side spends per
/// distinct note - and it is meaningful only when both sides are on the same
/// track. Check that first: a scene's engine trace starts the track its own
/// prescript selects (op `0x35`), while a retail save carries whatever track
/// the playthrough left loaded, and the two are routinely different.
pub fn compare_voice_allocation(
    engine: &[AudioTraceFrame],
    retail: &[AudioTraceFrame],
) -> VoiceAllocationComparison {
    let e = summarise_allocation(engine);
    let r = summarise_allocation(retail);
    let shared_pitches = e.pitches.intersection(&r.pitches).count();
    let shared_tones = e.tones.intersection(&r.tones).count();
    let smaller = e.pitches.len().min(r.pitches.len());
    let overlap = if smaller == 0 {
        0.0
    } else {
        shared_pitches as f64 / smaller as f64
    };
    let verdict = if overlap < PITCH_OVERLAP_FLOOR {
        VoiceAllocationVerdict::DifferentNotes
    } else if e.mean_active < r.mean_active * VOICE_COUNT_FLOOR {
        VoiceAllocationVerdict::ShortOfVoices
    } else {
        VoiceAllocationVerdict::Comparable
    };
    let onset_ratio = if r.onsets_per_frame > 0.0 {
        e.onsets_per_frame / r.onsets_per_frame
    } else {
        0.0
    };
    VoiceAllocationComparison {
        engine: e,
        retail: r,
        shared_pitches,
        shared_tones,
        onset_ratio,
        alignment_offset: None,
        verdict,
    }
}

/// [`compare_voice_allocation`] over the engine window that best lines up
/// with the retail one ([`best_alignment_offset`]), rather than over the
/// engine trace's first frames.
///
/// Comparing frame 0 of each is what turned "the engine holds more voices
/// at once" into a standing residual: a cold engine trace opens at the
/// track's first bar while the capture sits wherever the playthrough parked
/// it, and the two bars have different note densities. Aligned, the two
/// sides' key-on counts land on top of each other.
pub fn compare_voice_allocation_aligned(
    engine: &[AudioTraceFrame],
    retail: &[AudioTraceFrame],
) -> VoiceAllocationComparison {
    match best_alignment_offset(engine, retail) {
        Some((off, _)) => {
            let mut c = compare_voice_allocation(&engine[off..off + retail.len()], retail);
            c.alignment_offset = Some(off);
            c
        }
        None => compare_voice_allocation(engine, retail),
    }
}
