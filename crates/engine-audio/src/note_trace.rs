//! Note-level trace of what the sequencer asks the SPU to do.
//!
//! The engine half of the note-level BGM differential. The recomp runtime
//! records a semantic key-on ring inside its SPU (`runtime/src/spu.c`,
//! `spu_event_record`) that snapshots the voice registers at the instant of
//! each key-on; this module records the same fields at the same layer on our
//! side, so the two timelines are comparable by construction rather than by
//! interpretation.
//!
//! The recording point matters. [`crate::vab_bind::VabBank::fire`] programs a
//! voice and then keys it on directly through [`crate::spu::voice::Voice`],
//! bypassing [`crate::spu::Spu::key_on_mask`] - so a hook on the mask API
//! would silently miss every sequencer note. Recording is therefore driven by
//! explicit [`Spu::record_key_on`](crate::spu::Spu::record_key_on) /
//! [`Spu::record_key_off`](crate::spu::Spu::record_key_off) calls placed next
//! to the real key transitions.
//!
//! Time is kept in SPU samples and converted to retail frames at the hardware
//! ratio (44100 / 60 = 735 samples per frame), which is the same time base the
//! recomp capture reports - see `scripts/recomp/audio_note_capture.py`.
//!
//! Emitted JSONL is the shared canonical shape consumed by
//! `scripts/recomp/note_diff.py`. Serialisation is hand-rolled: this crate
//! deliberately carries no serde dependency.

/// SPU output samples per retail frame (44100 / 60).
pub const SAMPLES_PER_FRAME: u64 = 735;

/// What kind of voice transition a [`NoteEvent`] records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteEventKind {
    /// A voice was keyed on - carries the full programmed voice state.
    On,
    /// A voice was keyed off (envelope released; the voice keeps sounding
    /// until the release drains, exactly as on hardware).
    Off,
}

impl NoteEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NoteEventKind::On => "on",
            NoteEventKind::Off => "off",
        }
    }
}

/// The programmed state of a voice at the instant it was keyed.
///
/// Groups the fields the retail SPU keeps in a voice's register block, which
/// is what makes a key-on describable as a note at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VoiceSnapshot {
    /// SPU-RAM byte address of the ADPCM block. The handle back to the disc:
    /// it identifies which uploaded VAG, hence which tone.
    pub addr: u32,
    pub pitch: u16,
    pub voll: i16,
    pub volr: i16,
    /// Raw VAB tone ADSR words, as programmed. Comparing these against the
    /// recomp's `adsr_lo`/`adsr_hi` validates *tone selection* independently
    /// of pitch and volume.
    pub adsr1: u16,
    pub adsr2: u16,
}

/// One recorded voice transition.
#[derive(Debug, Clone, Copy)]
pub struct NoteEvent {
    /// Ordinal within the trace.
    pub i: u64,
    /// SPU sample index when recorded.
    pub sample: u64,
    /// Retail frame index (`sample / SAMPLES_PER_FRAME`).
    pub frame: u64,
    pub ev: NoteEventKind,
    pub voice: u8,
    /// Voice state as programmed at the key transition. Zeroed for key-offs,
    /// except `addr`, which identifies the voice's sample.
    pub voice_state: VoiceSnapshot,
}

impl NoteEvent {
    /// One canonical JSONL line.
    pub fn to_json(&self) -> String {
        let mut s = format!(
            "{{\"i\":{},\"frame\":{},\"sample\":{},\"ev\":\"{}\",\"v\":{},\"addr\":{}",
            self.i,
            self.frame,
            self.sample,
            self.ev.as_str(),
            self.voice,
            self.voice_state.addr
        );
        if self.ev == NoteEventKind::On {
            let v = &self.voice_state;
            s.push_str(&format!(
                ",\"pitch\":{},\"voll\":{},\"volr\":{},\"adsr1\":{},\"adsr2\":{}",
                v.pitch, v.voll, v.volr, v.adsr1, v.adsr2
            ));
        }
        s.push('}');
        s
    }
}

/// Recorder attached to an [`Spu`](crate::spu::Spu).
///
/// Off by default - a `None` trace on the SPU costs nothing, so the normal
/// audio path is unaffected.
#[derive(Debug, Clone, Default)]
pub struct NoteTrace {
    pub events: Vec<NoteEvent>,
    /// Current SPU sample clock, advanced by the driver.
    pub sample: u64,
}

impl NoteTrace {
    pub fn new() -> Self {
        Self::default()
    }

    /// Advance the trace clock by `n` SPU samples.
    pub fn advance(&mut self, n: u64) {
        self.sample = self.sample.saturating_add(n);
    }

    pub(crate) fn push(&mut self, ev: NoteEventKind, voice: u8, voice_state: VoiceSnapshot) {
        let i = self.events.len() as u64;
        self.events.push(NoteEvent {
            i,
            sample: self.sample,
            frame: self.sample / SAMPLES_PER_FRAME,
            ev,
            voice,
            voice_state,
        });
    }

    /// Note-ons only, in order.
    pub fn note_ons(&self) -> impl Iterator<Item = &NoteEvent> {
        self.events.iter().filter(|e| e.ev == NoteEventKind::On)
    }

    /// Render the whole trace as canonical JSONL, `header` line first.
    pub fn to_jsonl(&self, header: &str) -> String {
        let mut out = String::from(header);
        out.push('\n');
        for e in &self.events {
            out.push_str(&e.to_json());
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_derives_from_sample_at_the_hardware_ratio() {
        let mut t = NoteTrace::new();
        t.advance(SAMPLES_PER_FRAME * 3);
        t.push(
            NoteEventKind::On,
            5,
            VoiceSnapshot {
                addr: 0x1000,
                pitch: 0x1000,
                voll: 100,
                volr: 100,
                adsr1: 0x80FF,
                adsr2: 0x5FCF,
            },
        );
        assert_eq!(t.events[0].frame, 3);
        assert_eq!(t.events[0].sample, 2205);
    }

    #[test]
    fn note_off_lines_omit_the_voice_state_fields() {
        let mut t = NoteTrace::new();
        t.push(
            NoteEventKind::Off,
            2,
            VoiceSnapshot {
                addr: 0x40,
                ..Default::default()
            },
        );
        let line = t.events[0].to_json();
        assert!(line.contains("\"ev\":\"off\""));
        assert!(!line.contains("pitch"));
    }

    #[test]
    fn jsonl_emits_header_then_one_line_per_event() {
        let mut t = NoteTrace::new();
        t.push(
            NoteEventKind::On,
            0,
            VoiceSnapshot {
                addr: 1,
                pitch: 2,
                ..Default::default()
            },
        );
        t.push(
            NoteEventKind::Off,
            0,
            VoiceSnapshot {
                addr: 1,
                ..Default::default()
            },
        );
        let text = t.to_jsonl("{\"kind\":\"header\"}");
        assert_eq!(text.lines().count(), 3);
    }
}
