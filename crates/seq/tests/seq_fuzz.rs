//! Panic-hardening regression tests for the SEQ parser entry points.
//!
//! The web viewer / per-entry inspector feeds ARBITRARY bytes into these
//! parsers; a junk match must return `Err`, never panic. Every fixture is
//! hand-constructed synthetic data and the suite passes with
//! `LEGAIA_DISC_BIN` unset.
//!
//! Note: `parse_header` deliberately accepts both the PsyQ shape (u16 BE
//! version) and the Legaia variant (u32 BE version, two extra bytes), and
//! meta `0x51` can be non-3-byte / running status is preserved across meta.
//! These tests only assert the *absence of panics* on malformed bytes;
//! they do not change that accepting behaviour.

use legaia_seq::{SEQ_MAGIC, Seq, parse_header, read_vlq};

/// Minimal valid PsyQ-shape header (13 bytes) with no event stream.
fn header_only() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&SEQ_MAGIC);
    out.extend_from_slice(&[0x00, 0x01]); // version 1 (u16 BE) -> PsyQ shape
    out.extend_from_slice(&[0x01, 0xE0]); // ppqn 480
    out.extend_from_slice(&[0x07, 0xA1, 0x20]); // tempo
    out.push(0x04);
    out.push(0x02);
    out
}

#[test]
fn empty_and_tiny_inputs_return_err() {
    for len in 0..13 {
        let buf = vec![0u8; len];
        assert!(parse_header(&buf).is_err(), "len={len} too small");
        assert!(Seq::parse(&buf).is_err());
    }
}

#[test]
fn bad_magic_returns_err() {
    let mut buf = header_only();
    buf[0] = b'X';
    assert!(parse_header(&buf).is_err());
    assert!(Seq::parse(&buf).is_err());
}

#[test]
fn header_only_no_event_stream_is_ok() {
    // A header with an empty event stream parses to zero events (no panic).
    let buf = header_only();
    let seq = Seq::parse(&buf).expect("header-only is structurally valid");
    assert!(seq.events.is_empty());
}

#[test]
fn truncated_delta_time_returns_err() {
    // Event stream is a lone VLQ continuation byte with no terminator/body.
    let mut buf = header_only();
    buf.push(0x80); // VLQ with continuation bit, no follow-up byte
    assert!(Seq::parse(&buf).is_err());
}

#[test]
fn running_status_with_no_prior_returns_err() {
    // delta 0 then a data byte (< 0x80) with no preceding status byte.
    let mut buf = header_only();
    buf.push(0x00); // delta 0
    buf.push(0x40); // data byte, but no running status established
    assert!(Seq::parse(&buf).is_err());
}

#[test]
fn meta_payload_overrun_returns_err() {
    // delta 0, meta 0xFF kind 0x7F, length VLQ says 200 but stream is short.
    let mut buf = header_only();
    buf.push(0x00); // delta
    buf.push(0xFF); // meta
    buf.push(0x7F); // kind
    buf.push(0x81); // VLQ length: 0x81 0x48 = 200
    buf.push(0x48);
    // no payload bytes follow
    assert!(Seq::parse(&buf).is_err());
}

#[test]
fn channel_message_truncated_data_returns_err() {
    // delta 0, NoteOn (0x90) needs 2 data bytes but stream ends.
    let mut buf = header_only();
    buf.push(0x00);
    buf.push(0x90);
    buf.push(0x3C); // only one data byte
    assert!(Seq::parse(&buf).is_err());
}

#[test]
fn vlq_short_read_and_overlong_return_err() {
    // Pure continuation bytes - never terminates within 4 bytes.
    let overlong = [0x80u8, 0x80, 0x80, 0x80, 0x80, 0x80];
    assert!(read_vlq(&overlong, 0).is_err());
    // Read starting past the end.
    let buf = [0x00u8];
    assert!(read_vlq(&buf, 5).is_err());
}

#[test]
fn vlq_extreme_pos_does_not_overflow() {
    // `read_vlq` is a public entry point with a caller-supplied `pos`.
    // A junk `pos == usize::MAX` would overflow the `pos + consumed`
    // index arithmetic and panic in a debug build. It must return Err.
    let buf = [0x00u8, 0x80, 0x40];
    assert!(read_vlq(&buf, usize::MAX).is_err());
    assert!(read_vlq(&buf, usize::MAX - 1).is_err());
    // An empty buffer with an extreme offset is also Err, not a panic.
    assert!(read_vlq(&[], usize::MAX).is_err());
}

#[test]
fn legaia_shape_truncated_returns_err() {
    // u32 BE version == 1 at +4 selects the Legaia shape (15-byte header).
    // Provide only enough for the PsyQ shape so the Legaia branch is not
    // taken on a too-short buffer (must not slice OOB).
    let mut buf = Vec::new();
    buf.extend_from_slice(&SEQ_MAGIC);
    buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]); // u32 BE version 1
    buf.extend_from_slice(&[0x01, 0xE0]); // ppqn (PsyQ-shape positions)
    buf.extend_from_slice(&[0x07, 0xA1, 0x20]);
    // This is exactly 13 bytes -> PsyQ shape path (Legaia needs >= 15).
    let _ = parse_header(&buf); // must not panic regardless of which branch
}

#[test]
fn random_data_is_panic_free() {
    let mut buf = vec![0u8; 4096];
    let mut x: u32 = 0x0BAD_F00D;
    for b in buf.iter_mut() {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        *b = (x >> 16) as u8;
    }
    // Force a valid magic so the event-stream walker actually runs on junk.
    buf[0..4].copy_from_slice(&SEQ_MAGIC);
    let _ = parse_header(&buf);
    let _ = Seq::parse(&buf);
}
