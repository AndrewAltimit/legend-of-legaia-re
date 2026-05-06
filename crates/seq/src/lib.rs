//! Clean-room parser for PsyQ SEQ files (PS1 sequenced-music format).
//!
//! SEQ is a MIDI-derived format used by Sony's `libsnd` SsAPI sequencer
//! (`SsSeqOpen`/`SsSeqPlay`/...). The header is 15 bytes; the payload is a
//! single MIDI-style track of delta-time + event records, with running
//! status, terminated by a meta `FF 2F 00` (End-of-Track).
//!
//! No Sony bytes ship with this crate. The format description below is
//! reconstructed from the public PsyQ SDK documentation and verified
//! against synthetic test vectors (see `tests/`).
//!
//! ## Header
//!
//! ```text
//! +0x00  u8[4]   magic = "pQES" (0x70 0x51 0x45 0x53)
//! +0x04  u16 BE  version             (typically 1)
//! +0x06  u16 BE  resolution (PPQN)   ticks per quarter note
//! +0x08  u24 BE  initial tempo       microseconds per quarter note
//! +0x0B  u8      time-signature numerator
//! +0x0C  u8      time-signature denominator (as a power of 2)
//! +0x0D  ...     event stream (delta-time + MIDI byte stream)
//! ```
//!
//! ## Events
//!
//! Each event begins with a variable-length-quantity (VLQ) delta-time in
//! ticks, followed by a status byte (or running-status data byte).
//!
//! | Status range | Event           | Data bytes |
//! | ------------ | --------------- | ---------- |
//! | `0x80..=0x8F`| Note Off        | 2 (key, vel) |
//! | `0x90..=0x9F`| Note On         | 2 (key, vel; vel=0 ≡ NoteOff) |
//! | `0xA0..=0xAF`| Poly Aftertouch | 2 |
//! | `0xB0..=0xBF`| Control Change  | 2 |
//! | `0xC0..=0xCF`| Program Change  | 1 |
//! | `0xD0..=0xDF`| Channel Aftertouch | 1 |
//! | `0xE0..=0xEF`| Pitch Bend      | 2 |
//! | `0xFF NN LL` | Meta event      | LL bytes (LL is VLQ) |
//!
//! Meta events carry tempo (`0x51`, 3-byte payload), time signature
//! (`0x58`), and end-of-track (`0x2F`, zero-length).
//!
//! ## Use
//!
//! `Seq::parse(buf)` validates the header and returns a fully-decoded
//! [`Seq`] with an event vector. The vector preserves source order and
//! original delta-times — engines feed it into a tick-based player; see
//! `legaia-engine-audio::Sequencer` for the runtime side.

#![forbid(unsafe_code)]

use anyhow::{Result, bail};
use serde::Serialize;

/// SEQ file magic (`"pQES"` in source order, big-endian byte sequence).
pub const SEQ_MAGIC: [u8; 4] = *b"pQES";

/// Header length for the standard PsyQ SEQ shape (u16 BE version).
pub const HEADER_LEN: usize = 0x0D;

/// Header length for the Legaia variant (u32 BE version — 2 extra bytes
/// before the PPQN word). Real disc SEQ files use this shape; synthetic
/// test fixtures use [`HEADER_LEN`].
pub const HEADER_LEN_LEGAIA: usize = HEADER_LEN + 2;

/// Decoded SEQ header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Header {
    /// File-format version (almost always 1).
    pub version: u16,
    /// Pulses per quarter note. Drives the tick rate.
    pub ppqn: u16,
    /// Microseconds per quarter note. The tempo can be overridden mid-stream
    /// by a `0xFF 0x51` meta event.
    pub tempo_us_per_qn: u32,
    /// Time-signature numerator (e.g. 4 for 4/4).
    pub time_sig_num: u8,
    /// Time-signature denominator as a power of 2 (e.g. 2 for /4, 3 for /8).
    pub time_sig_denom_pow2: u8,
}

impl Header {
    /// Initial beats-per-minute derived from tempo. SEQ stores tempo in
    /// microseconds per quarter; `bpm = 60_000_000 / tempo`.
    pub fn bpm(self) -> f32 {
        if self.tempo_us_per_qn == 0 {
            0.0
        } else {
            60_000_000.0 / self.tempo_us_per_qn as f32
        }
    }
}

/// Decoded MIDI-channel event (status `0x80..=0xEF`). The channel index is
/// the low nibble of the status byte (`0..=15`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ChannelMessage {
    NoteOff { key: u8, velocity: u8 },
    NoteOn { key: u8, velocity: u8 },
    PolyAftertouch { key: u8, value: u8 },
    ControlChange { control: u8, value: u8 },
    ProgramChange { program: u8 },
    ChannelAftertouch { value: u8 },
    PitchBend { value: u16 },
}

/// Meta event (status `0xFF`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum MetaMessage {
    /// `0xFF 0x2F 0x00` — End-of-Track. Always the final event.
    EndOfTrack,
    /// `0xFF 0x51 0x03` — Tempo change (microseconds per quarter note).
    SetTempo { us_per_qn: u32 },
    /// `0xFF 0x58 0x04` — Time signature.
    TimeSignature {
        numerator: u8,
        denominator_pow2: u8,
        clocks_per_metronome: u8,
        thirty_seconds_per_quarter: u8,
    },
    /// `0xFF 0x59 0x02` — Key signature.
    KeySignature { sharps: i8, minor: u8 },
    /// Any other meta event we surface as raw bytes so engines can
    /// table-dispatch when needed.
    Other { kind: u8, data: Vec<u8> },
}

/// Decoded payload of one timed event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum EventBody {
    Channel {
        channel: u8,
        message: ChannelMessage,
    },
    Meta(MetaMessage),
}

/// Timed event (delta-ticks + payload).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Event {
    /// Delta from the previous event in PPQN ticks.
    pub delta: u32,
    /// Decoded payload.
    pub body: EventBody,
}

/// Whole-file decoded SEQ.
#[derive(Debug, Clone, Serialize)]
pub struct Seq {
    pub header: Header,
    pub events: Vec<Event>,
}

impl Seq {
    /// Parse a complete SEQ file. Returns the header + every decoded event,
    /// stopping at the first `End-of-Track` meta (which is required).
    pub fn parse(buf: &[u8]) -> Result<Self> {
        let (header, header_len) = parse_header_with_len(buf)?;
        let events = parse_events(&buf[header_len..])?;
        Ok(Self { header, events })
    }

    /// Sum of every delta — total length of the sequence in PPQN ticks.
    pub fn total_ticks(&self) -> u64 {
        self.events.iter().map(|e| e.delta as u64).sum()
    }

    /// Histogram of channel/meta event types — useful for inspection.
    pub fn event_summary(&self) -> EventSummary {
        let mut s = EventSummary::default();
        for ev in &self.events {
            match &ev.body {
                EventBody::Channel { message, .. } => match message {
                    ChannelMessage::NoteOn { velocity, .. } => {
                        if *velocity == 0 {
                            s.note_off += 1;
                        } else {
                            s.note_on += 1;
                        }
                    }
                    ChannelMessage::NoteOff { .. } => s.note_off += 1,
                    ChannelMessage::ProgramChange { .. } => s.program_change += 1,
                    ChannelMessage::ControlChange { .. } => s.control_change += 1,
                    ChannelMessage::PitchBend { .. } => s.pitch_bend += 1,
                    _ => s.other_channel += 1,
                },
                EventBody::Meta(MetaMessage::SetTempo { .. }) => s.set_tempo += 1,
                EventBody::Meta(MetaMessage::TimeSignature { .. }) => s.time_sig += 1,
                EventBody::Meta(MetaMessage::EndOfTrack) => s.end_of_track += 1,
                EventBody::Meta(_) => s.other_meta += 1,
            }
        }
        s
    }
}

/// Counts of each event family. Pure inspection / debugging.
#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct EventSummary {
    pub note_on: u32,
    pub note_off: u32,
    pub program_change: u32,
    pub control_change: u32,
    pub pitch_bend: u32,
    pub other_channel: u32,
    pub set_tempo: u32,
    pub time_sig: u32,
    pub end_of_track: u32,
    pub other_meta: u32,
}

/// Parse just the header. Used by tooling that wants metadata without
/// decoding the full event stream. Accepts both the standard PsyQ SEQ
/// shape (u16 BE version) and the Legaia variant (u32 BE version, with
/// two extra reserved bytes before the PPQN word). Real disc SEQ files
/// use the Legaia shape.
pub fn parse_header(buf: &[u8]) -> Result<Header> {
    parse_header_with_len(buf).map(|(h, _)| h)
}

/// Like [`parse_header`] but also returns the header byte length, which
/// callers need to seek to the start of the event stream.
pub fn parse_header_with_len(buf: &[u8]) -> Result<(Header, usize)> {
    if buf.len() < HEADER_LEN {
        bail!(
            "SEQ buffer too small: {} bytes (need >= {})",
            buf.len(),
            HEADER_LEN
        );
    }
    if buf[0..4] != SEQ_MAGIC {
        bail!(
            "bad SEQ magic: {:02X?} (expected {:02X?})",
            &buf[0..4],
            SEQ_MAGIC
        );
    }
    // Detect the variant: if `u32 BE at +4..+8 == 1`, this is the Legaia
    // shape; otherwise the standard PsyQ shape.
    let v32 = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    if v32 == 1 && buf.len() >= HEADER_LEN_LEGAIA {
        let ppqn = u16::from_be_bytes([buf[8], buf[9]]);
        let tempo_us_per_qn = u24_be(&buf[10..13]);
        let time_sig_num = buf[13];
        let time_sig_denom_pow2 = buf[14];
        return Ok((
            Header {
                version: 1,
                ppqn,
                tempo_us_per_qn,
                time_sig_num,
                time_sig_denom_pow2,
            },
            HEADER_LEN_LEGAIA,
        ));
    }
    // PsyQ-doc shape — u16 version at +4..+6.
    let version = u16::from_be_bytes([buf[4], buf[5]]);
    let ppqn = u16::from_be_bytes([buf[6], buf[7]]);
    let tempo_us_per_qn = u24_be(&buf[8..11]);
    let time_sig_num = buf[0x0B];
    let time_sig_denom_pow2 = buf[0x0C];
    Ok((
        Header {
            version,
            ppqn,
            tempo_us_per_qn,
            time_sig_num,
            time_sig_denom_pow2,
        },
        HEADER_LEN,
    ))
}

fn u24_be(b: &[u8]) -> u32 {
    ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32)
}

/// Decode one variable-length-quantity (VLQ) integer from `buf` at `pos`.
/// Returns `(value, bytes_consumed)`. Used for delta-times and meta lengths.
pub fn read_vlq(buf: &[u8], pos: usize) -> Result<(u32, usize)> {
    let mut value: u32 = 0;
    let mut consumed: usize = 0;
    loop {
        let b = *buf
            .get(pos + consumed)
            .ok_or_else(|| anyhow::anyhow!("VLQ: short read at +{}", pos + consumed))?;
        consumed += 1;
        value = (value << 7) | (b & 0x7F) as u32;
        if b & 0x80 == 0 {
            break;
        }
        if consumed > 4 {
            bail!("VLQ longer than 4 bytes at +{}", pos);
        }
    }
    Ok((value, consumed))
}

fn parse_events(stream: &[u8]) -> Result<Vec<Event>> {
    let mut events = Vec::new();
    let mut pos = 0;
    let mut running_status: Option<u8> = None;
    while pos < stream.len() {
        let (delta, n) = read_vlq(stream, pos)?;
        pos += n;
        if pos >= stream.len() {
            bail!("event stream ended after delta-time at +{}", pos);
        }
        let mut status_byte = stream[pos];
        if status_byte < 0x80 {
            // Running status: reuse previous status byte; status_byte is data.
            status_byte = running_status
                .ok_or_else(|| anyhow::anyhow!("running status with no prior at +{}", pos))?;
        } else {
            pos += 1;
        }

        let body = match status_byte {
            0xFF => {
                // Meta event. PsyQ libsnd preserves running status across
                // meta events (real Legaia SEQ data relies on this — the
                // strict-MIDI behaviour of clearing running status here
                // would cause a "running status with no prior" error on
                // the byte stream immediately after a meta).
                if pos >= stream.len() {
                    bail!("meta event truncated at +{}", pos);
                }
                let kind = stream[pos];
                pos += 1;
                let (length, lvlq) = read_vlq(stream, pos)?;
                pos += lvlq;
                let length = length as usize;
                if pos + length > stream.len() {
                    bail!("meta event payload overruns at +{}", pos);
                }
                let payload = &stream[pos..pos + length];
                pos += length;
                let meta = decode_meta(kind, payload)?;
                EventBody::Meta(meta)
            }
            0xF0 | 0xF7 => {
                // SysEx — not used by libsnd; parse-and-skip so we don't
                // explode on unknown payloads but warn loudly via Other meta.
                running_status = None;
                let (length, lvlq) = read_vlq(stream, pos)?;
                pos += lvlq;
                let length = length as usize;
                if pos + length > stream.len() {
                    bail!("sysex event payload overruns at +{}", pos);
                }
                let payload = stream[pos..pos + length].to_vec();
                pos += length;
                EventBody::Meta(MetaMessage::Other {
                    kind: status_byte,
                    data: payload,
                })
            }
            s if s & 0x80 != 0 => {
                // Channel voice / mode message. Consume the message-specific
                // number of data bytes.
                running_status = Some(s);
                let channel = s & 0x0F;
                let high = s & 0xF0;
                let needs = match high {
                    0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => 2,
                    0xC0 | 0xD0 => 1,
                    _ => bail!("unsupported status nibble {:#x} at +{}", high, pos),
                };
                if pos + needs > stream.len() {
                    bail!("channel message data truncated at +{}", pos);
                }
                let data = &stream[pos..pos + needs];
                pos += needs;
                let message = decode_channel(high, data);
                EventBody::Channel { channel, message }
            }
            _ => bail!(
                "unhandled status byte {:#x} at +{} (running={:?})",
                status_byte,
                pos,
                running_status
            ),
        };

        let is_eot = matches!(&body, EventBody::Meta(MetaMessage::EndOfTrack));
        events.push(Event { delta, body });
        if is_eot {
            break;
        }
    }
    Ok(events)
}

fn decode_channel(high: u8, data: &[u8]) -> ChannelMessage {
    match high {
        0x80 => ChannelMessage::NoteOff {
            key: data[0],
            velocity: data[1],
        },
        0x90 => ChannelMessage::NoteOn {
            key: data[0],
            velocity: data[1],
        },
        0xA0 => ChannelMessage::PolyAftertouch {
            key: data[0],
            value: data[1],
        },
        0xB0 => ChannelMessage::ControlChange {
            control: data[0],
            value: data[1],
        },
        0xC0 => ChannelMessage::ProgramChange { program: data[0] },
        0xD0 => ChannelMessage::ChannelAftertouch { value: data[0] },
        0xE0 => {
            let lsb = data[0] as u16 & 0x7F;
            let msb = data[1] as u16 & 0x7F;
            ChannelMessage::PitchBend {
                value: (msb << 7) | lsb,
            }
        }
        _ => unreachable!(),
    }
}

fn decode_meta(kind: u8, payload: &[u8]) -> Result<MetaMessage> {
    Ok(match kind {
        0x2F => MetaMessage::EndOfTrack,
        0x51 => {
            // Standard MIDI SetTempo is exactly 3 bytes (u24 BE us/qn).
            // Real Legaia SEQ data sometimes carries longer 0x51 payloads
            // — likely PsyQ-specific extensions (loop markers / mark
            // events). Surface those as `Other` rather than failing the
            // whole-track decode.
            if payload.len() == 3 {
                MetaMessage::SetTempo {
                    us_per_qn: u24_be(payload),
                }
            } else {
                MetaMessage::Other {
                    kind,
                    data: payload.to_vec(),
                }
            }
        }
        0x58 => {
            // Same tolerance as 0x51 — accept the canonical 4-byte form
            // and surface anything else as `Other`.
            if payload.len() != 4 {
                return Ok(MetaMessage::Other {
                    kind,
                    data: payload.to_vec(),
                });
            }
            MetaMessage::TimeSignature {
                numerator: payload[0],
                denominator_pow2: payload[1],
                clocks_per_metronome: payload[2],
                thirty_seconds_per_quarter: payload[3],
            }
        }
        0x59 => {
            if payload.len() != 2 {
                return Ok(MetaMessage::Other {
                    kind,
                    data: payload.to_vec(),
                });
            }
            MetaMessage::KeySignature {
                sharps: payload[0] as i8,
                minor: payload[1],
            }
        }
        _ => MetaMessage::Other {
            kind,
            data: payload.to_vec(),
        },
    })
}

/// Compute microseconds per PPQN tick from a tempo + ppqn pair.
///
/// `tempo` is microseconds per quarter note; `ppqn` is ticks per quarter
/// note. The product `tempo / ppqn` is the per-tick duration the SsAPI
/// sequencer accumulates against to decide when to fire the next event.
pub fn us_per_tick(tempo_us_per_qn: u32, ppqn: u16) -> f64 {
    if ppqn == 0 {
        0.0
    } else {
        tempo_us_per_qn as f64 / ppqn as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-build a minimal SEQ: 1 program-change, 1 note-on, 1 note-off,
    /// end-of-track.
    fn synthetic_seq() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&SEQ_MAGIC); // magic
        out.extend_from_slice(&[0x00, 0x01]); // version 1 BE
        out.extend_from_slice(&[0x01, 0xE0]); // ppqn = 480 BE
        out.extend_from_slice(&[0x07, 0xA1, 0x20]); // tempo 500_000 us/qn (120 BPM)
        out.push(0x04); // 4
        out.push(0x02); // /4
        // Events:
        // delta 0, ProgramChange ch0 program 5
        out.push(0x00); // VLQ delta 0
        out.push(0xC0);
        out.push(0x05);
        // delta 0, NoteOn ch0 key 60 vel 100
        out.push(0x00);
        out.push(0x90);
        out.push(60);
        out.push(100);
        // delta 480, NoteOff ch0 key 60 vel 0 (running status NoteOn vel=0)
        out.push(0x83); // VLQ 480 (0x83, 0x60)
        out.push(0x60);
        out.push(60);
        out.push(0);
        // delta 0, end-of-track
        out.push(0x00);
        out.push(0xFF);
        out.push(0x2F);
        out.push(0x00);
        out
    }

    #[test]
    fn header_parses() {
        let buf = synthetic_seq();
        let seq = Seq::parse(&buf).unwrap();
        assert_eq!(seq.header.version, 1);
        assert_eq!(seq.header.ppqn, 480);
        assert_eq!(seq.header.tempo_us_per_qn, 500_000);
        assert_eq!(seq.header.time_sig_num, 4);
        assert_eq!(seq.header.time_sig_denom_pow2, 2);
        assert!((seq.header.bpm() - 120.0).abs() < 0.5);
    }

    #[test]
    fn events_decode() {
        let buf = synthetic_seq();
        let seq = Seq::parse(&buf).unwrap();
        assert_eq!(seq.events.len(), 4);
        match &seq.events[0].body {
            EventBody::Channel {
                channel,
                message: ChannelMessage::ProgramChange { program },
            } => {
                assert_eq!(*channel, 0);
                assert_eq!(*program, 5);
            }
            other => panic!("expected ProgramChange, got {:?}", other),
        }
        match &seq.events[1].body {
            EventBody::Channel {
                message: ChannelMessage::NoteOn { key, velocity },
                ..
            } => {
                assert_eq!(*key, 60);
                assert_eq!(*velocity, 100);
            }
            _ => panic!(),
        }
        // Running-status NoteOn with vel=0 — semantic NoteOff but emitted as
        // NoteOn so engines can table-dispatch identically.
        match &seq.events[2].body {
            EventBody::Channel {
                message: ChannelMessage::NoteOn { key, velocity },
                ..
            } => {
                assert_eq!(*key, 60);
                assert_eq!(*velocity, 0);
            }
            other => panic!("expected NoteOn(vel=0), got {:?}", other),
        }
        assert_eq!(seq.events[2].delta, 480);
        match &seq.events[3].body {
            EventBody::Meta(MetaMessage::EndOfTrack) => {}
            _ => panic!(),
        }
    }

    #[test]
    fn vlq_round_trip() {
        for &v in &[0u32, 1, 0x7F, 0x80, 0x3FFF, 0x4000, 0x1F_FFFF, 0x0FFF_FFFF] {
            let mut buf = Vec::new();
            write_vlq(&mut buf, v);
            let (decoded, n) = read_vlq(&buf, 0).unwrap();
            assert_eq!(decoded, v, "round-trip failed for {}", v);
            assert_eq!(n, buf.len());
        }
    }

    #[test]
    fn rejects_bad_magic() {
        let mut buf = synthetic_seq();
        buf[0] = 0;
        assert!(Seq::parse(&buf).is_err());
    }

    #[test]
    fn summary_counts() {
        let buf = synthetic_seq();
        let seq = Seq::parse(&buf).unwrap();
        let s = seq.event_summary();
        assert_eq!(s.program_change, 1);
        assert_eq!(s.note_on, 1);
        assert_eq!(s.note_off, 1); // vel=0 NoteOn counts as NoteOff in the summary
        assert_eq!(s.end_of_track, 1);
    }

    /// Helper: encode a u32 as VLQ. Tests-only.
    fn write_vlq(out: &mut Vec<u8>, mut v: u32) {
        let mut bytes = [0u8; 5];
        let mut n = 0;
        loop {
            bytes[n] = (v & 0x7F) as u8;
            n += 1;
            v >>= 7;
            if v == 0 {
                break;
            }
        }
        // Output high-order first with continuation bits set.
        for i in (0..n).rev() {
            let mut b = bytes[i];
            if i != 0 {
                b |= 0x80;
            }
            out.push(b);
        }
    }
}
