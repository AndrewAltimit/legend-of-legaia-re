//! The retail side of the audio trace and the trace's JSONL form: load a
//! runtime trace from a JSONL file or a save state's SPU block, and write /
//! parse the line format both sides share.

use super::*;

/// Load a multi-frame retail trace from a JSONL file emitted by
/// `scripts/pcsx-redux/extract_audio_trace_from_sstates.py` (which itself
/// decodes the binary stream produced by
/// `scripts/pcsx-redux/autorun_audio_trace.lua`). Each line of the file
/// is one [`AudioTraceFrame`] record; the `frame` field carries the
/// vsync index at which the snapshot was taken in the live emulator.
///
/// This is the multi-frame sibling of
/// [`load_runtime_audio_trace_from_save`] - the latter lifts a single
/// SPU snapshot out of a mednafen save state, while this one consumes a
/// trace of N snapshots captured per-vsync against a running emulator.
/// The multi-frame trace is what
/// [`first_audio_trace_divergence_multi`] consumes to do a
/// frame-by-frame comparison against the engine trace.
pub fn load_runtime_audio_trace_jsonl(path: &Path) -> Result<Vec<AudioTraceFrame>> {
    let s = std::fs::read_to_string(path)
        .with_context(|| format!("read retail audio-trace JSONL {}", path.display()))?;
    parse_audio_trace_jsonl(&s)
}

/// Lift a single audio-trace sample out of a mednafen `.mc{slot}` save.
/// Reads the SPU section via [`legaia_mednafen::PsxSpu`].
///
/// Returns a `frame = 0` record with voice/master/reverb populated from
/// the save state. `sequencer_playhead_ticks` / `sequencer_finished` are
/// `None` because the SPU section doesn't carry sequencer (CPU-side
/// libsnd) state.
pub fn load_runtime_audio_trace_from_save(save: &Path) -> Result<AudioTraceFrame> {
    use legaia_mednafen::{PsxSpu, SaveState};

    let state = SaveState::from_path(save)
        .with_context(|| format!("load mednafen save {}", save.display()))?;
    let spu = PsxSpu::new(&state);
    let mednafen_voices = spu.voices();
    // `Reverb_Mode` is mednafen's name for the per-voice reverb-enable mask,
    // not a libspu mode byte; the register shadow's own `EON` is the same
    // value and is the one the field is named for here.
    let eon = spu.voice_reverb_mask().or_else(|| spu.reverb_mode());
    let mut voices = Vec::with_capacity(NUM_VOICES);
    let mut mask = 0u32;
    for (i, v) in mednafen_voices.iter().enumerate() {
        let active = v.is_active();
        if active {
            mask |= 1 << i;
        }
        voices.push(VoiceTraceFrame {
            active,
            start_addr: v.start_addr,
            loop_addr: v.loop_addr,
            pitch: v.pitch,
            env_level: v.adsr_env_level,
            vol_left: v.vol_left,
            vol_right: v.vol_right,
            adsr_control: v.adsr_control,
            reverb_send: eon.map(|m| m & (1u32 << i) != 0),
            key_ons: None,
        });
    }
    Ok(AudioTraceFrame {
        frame: 0,
        sequencer_playhead_ticks: None,
        sequencer_finished: None,
        master_volume: spu.master_volume(),
        // Retail keeps no mode *number* anywhere - only the coefficient
        // registers a mode expands to - so this stays `None` on the retail
        // side and `reverb_work_area` carries the preset-identifying size.
        reverb_mode: None,
        reverb_eon: eon,
        reverb_depth: spu.reverb_output_volume(),
        reverb_work_area: spu.reverb_work_area().map(|wa| wa.wrapping_mul(2)),
        spu_control: spu.spu_control(),
        sfx_ring: Vec::new(),
        active_voice_mask: mask,
        voices,
    })
}

/// Serialise a list of frames as JSON Lines. Round-trips through
/// [`parse_audio_trace_jsonl`].
pub fn audio_trace_to_jsonl(frames: &[AudioTraceFrame]) -> String {
    let mut out = String::new();
    for f in frames {
        out.push_str(&serde_json::to_string(f).expect("AudioTraceFrame JSON serialise"));
        out.push('\n');
    }
    out
}

/// Parse JSONL emitted by [`audio_trace_to_jsonl`]. Blank lines are
/// skipped so concatenated streams parse cleanly.
pub fn parse_audio_trace_jsonl(s: &str) -> Result<Vec<AudioTraceFrame>> {
    let mut out = Vec::new();
    for (i, line) in s.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let frame: AudioTraceFrame = serde_json::from_str(trimmed)
            .with_context(|| format!("parse JSONL line {}: {trimmed}", i + 1))?;
        out.push(frame);
    }
    Ok(out)
}
