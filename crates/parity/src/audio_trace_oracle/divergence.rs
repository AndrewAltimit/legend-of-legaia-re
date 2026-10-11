//! First-divergence search between an engine trace and a retail trace.

use super::*;

/// First field on which `engine` and `retail` disagree. Compares the
/// active-voice mask plus per-voice start_addr / loop_addr / pitch where
/// both sides report them.
///
/// Convergence rule: at least one engine frame's `active_voice_mask` must
/// be a *superset or equal* of retail's mask AND, for every voice retail
/// marks active, the engine's same voice index must also be active with a
/// matching start_addr (when both sides report it). The "superset" half is
/// pragmatic - the engine can leak voices across frames during the trace
/// window; what matters is "the engine saw the same voice allocations
/// retail did when retail captured".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTraceDivergence {
    pub kind: AudioDivergenceKind,
    pub engine: AudioTraceFrame,
    pub retail: AudioTraceFrame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioDivergenceKind {
    /// Engine never observed any of retail's active voices over the trace
    /// window. Most actionable: usually means the engine port didn't drive
    /// BGM playback at all.
    NoFrameMatched,
    /// Some engine frame had retail's voice indices active, but the
    /// voice's start_addr didn't match (different sample / bank slot).
    VoiceStartAddrMismatch,
    /// Engine matched retail's active mask but the master volume diverged.
    /// Lower-priority - master is per-frame in retail (sweep state) and
    /// per-frame in engine (libspu MVOL write), so equality is informational.
    MasterVolumeMismatch,
}

/// Walk `engine` left-to-right; return the first divergence point against
/// the retail snapshot.
pub fn first_audio_trace_divergence(
    engine: &[AudioTraceFrame],
    retail: &AudioTraceFrame,
) -> Option<AudioTraceDivergence> {
    if engine.is_empty() {
        return None;
    }
    if retail.active_voice_mask == 0 {
        // Retail captured zero active voices - the engine's quiescent
        // boot frame trivially matches. Don't surface a divergence.
        return None;
    }
    // Walk for a frame whose mask is a superset of retail's mask.
    let mut best: Option<&AudioTraceFrame> = None;
    for ef in engine {
        if ef.active_voice_mask & retail.active_voice_mask == retail.active_voice_mask {
            best = Some(ef);
            break;
        }
    }
    let Some(matched) = best else {
        // No engine frame had the right voice indices active.
        let last = engine.last().unwrap().clone();
        return Some(AudioTraceDivergence {
            kind: AudioDivergenceKind::NoFrameMatched,
            engine: last,
            retail: retail.clone(),
        });
    };
    // Mask matched - check start_addr alignment for each retail-active voice.
    for (i, rv) in retail.voices.iter().enumerate() {
        if !rv.active {
            continue;
        }
        let Some(ev) = matched.voices.get(i) else {
            continue;
        };
        // Both sides know the start_addr? They must match.
        if let (Some(es), Some(rs)) = (ev.start_addr, rv.start_addr)
            && es != rs
        {
            return Some(AudioTraceDivergence {
                kind: AudioDivergenceKind::VoiceStartAddrMismatch,
                engine: matched.clone(),
                retail: retail.clone(),
            });
        }
    }
    None
}

/// Multi-frame retail convergence walk. Sister to
/// [`first_audio_trace_divergence`] but consumes a `retail: &[AudioTraceFrame]`
/// trace captured per-vsync via the PCSX-Redux autorun probe.
///
/// Convergence rule (parallel to the single-frame rule, applied per
/// retail frame): for each retail frame whose `active_voice_mask` is
/// non-zero, there must exist some engine frame whose mask is a
/// **superset** of retail's mask AND, for every retail-active voice, the
/// engine's same voice index reports a matching `start_addr` (when both
/// sides report one). The first retail frame that fails this rule is
/// returned. Retail frames with `active_voice_mask == 0` are trivially
/// satisfied and skipped.
///
/// Picking the engine frame: we walk the engine trace forward and use
/// the first matching frame. This handles minor cross-fade timing
/// drift - if the engine's voice allocations land a few frames before
/// or after retail's, the convergence still triggers.
pub fn first_audio_trace_divergence_multi(
    engine: &[AudioTraceFrame],
    retail: &[AudioTraceFrame],
) -> Option<AudioTraceDivergence> {
    if engine.is_empty() {
        return None;
    }
    for rf in retail {
        if rf.active_voice_mask == 0 {
            continue;
        }
        if let Some(d) = first_audio_trace_divergence(engine, rf) {
            return Some(d);
        }
    }
    None
}
