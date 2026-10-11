//! Event dispatch: one sequence event onto its channel state and voices.

use super::*;
use crate::Spu;
use legaia_seq::{ChannelMessage, EventBody, MetaMessage};

impl Sequencer {
    pub(super) fn fire(&mut self, spu: &mut Spu, idx: usize) {
        let event = &self.seq.events[idx];
        match &event.body {
            EventBody::Channel { channel, message } => {
                let ch = (*channel as usize) % CHANNELS;
                self.fire_channel(spu, ch, *message);
            }
            EventBody::Meta(MetaMessage::SetTempo { us_per_qn }) => {
                // Tempo only re-prices the gap to *future* events; the
                // accumulator is in tempo-independent units (sample × ppqn ×
                // 1e6), so no rescaling of the carried remainder is needed.
                self.tempo_us_per_qn = (*us_per_qn).max(1);
                self.header_tempo = false;
                self.reprice_tempo();
            }
            EventBody::Meta(_) => {
                // Non-tempo meta events are inert for playback.
            }
        }
    }

    pub(super) fn fire_channel(&mut self, spu: &mut Spu, ch: usize, msg: ChannelMessage) {
        self.release_lost_voices(spu);
        match msg {
            ChannelMessage::ProgramChange { program } => {
                self.channels[ch].program = program;
            }
            ChannelMessage::ControlChange { control, value } => match control {
                0x07 => {
                    self.channels[ch].volume = value;
                    self.remix_channel(spu, ch);
                }
                0x0A => {
                    self.channels[ch].pan = value;
                    self.remix_channel(spu, ch);
                }
                CC_LOOP_MARKER => match value {
                    LOOP_START_VALUE => {
                        // Record the position *after* this marker. `self.next`
                        // is still this event's index here (the advance loop
                        // increments it after `fire` returns), so the event
                        // following the marker is `next + 1`. Rewinding there
                        // skips re-firing the marker and re-applying its delta.
                        self.loop_start = Some(self.next + 1);
                    }
                    LOOP_FOREVER_VALUE => {
                        self.pending_loop_forever = true;
                    }
                    _ => {}
                },
                _ => {}
            },
            ChannelMessage::NoteOn { key, velocity } => {
                if velocity == 0 {
                    self.note_off(spu, ch as u8, key);
                } else {
                    self.note_on(spu, ch as u8, key, velocity);
                }
            }
            ChannelMessage::NoteOff { key, .. } => {
                self.note_off(spu, ch as u8, key);
            }
            ChannelMessage::PitchBend { value } => {
                self.channels[ch].pitch_bend = value;
                for note in self
                    .active
                    .iter()
                    .filter(|n| n.channel as usize == ch && n.pending.is_none())
                {
                    if let Some(v) = spu.voices.get_mut(note.voice as usize) {
                        let (down, up) = note.bend_range;
                        v.pitch = bend_pitch(note.base_pitch, pitch_bend_factor(value, down, up));
                    }
                }
            }
            // Aftertouch (channel + poly) is recognized but unused by the
            // retail score (corpus sweep), so there is nothing to drive.
            ChannelMessage::PolyAftertouch { .. } | ChannelMessage::ChannelAftertouch { .. } => {}
        }
    }
}
