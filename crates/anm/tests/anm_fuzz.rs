//! Panic-hardening regression tests for the ANM parser entry points.
//!
//! The web viewer / per-entry inspector feeds ARBITRARY bytes into these
//! parsers; a junk match must return `Err` (or a bounded `Ok`), never
//! panic. Every fixture is hand-constructed synthetic data and the suite
//! passes with `LEGAIA_DISC_BIN` unset.

use legaia_anm::{
    BoneKeyframe, KeyframeReader, MAX_REASONABLE_COUNT, Preamble, RECORD_HEADER_SIZE, parse,
    peel_preamble,
};

#[test]
fn empty_and_one_byte_inputs_return_err() {
    for len in 0..4 {
        let buf = vec![0u8; len];
        assert!(parse(&buf).is_err(), "len={len} must Err (too small)");
    }
}

#[test]
fn bogus_huge_count_returns_err() {
    // count = 0xFFFFFFFF would, unguarded, drive a with_capacity that can
    // overflow / OOM. Must be rejected by the plausibility check.
    let mut buf = Vec::new();
    buf.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    buf.extend_from_slice(&[0u8; 64]);
    assert!(parse(&buf).is_err());

    // Just over MAX_REASONABLE_COUNT.
    let mut buf2 = Vec::new();
    buf2.extend_from_slice(&((MAX_REASONABLE_COUNT + 1) as u32).to_le_bytes());
    buf2.extend_from_slice(&[0u8; 64]);
    assert!(parse(&buf2).is_err());
}

#[test]
fn count_table_overruns_payload_returns_err() {
    // count says 100 records but there aren't 100 table entries' worth of
    // bytes after the count word.
    let mut buf = Vec::new();
    buf.extend_from_slice(&100u32.to_le_bytes());
    buf.extend_from_slice(&[0u8; 8]); // far too short for 100*4 table
    assert!(parse(&buf).is_err());
}

#[test]
fn offset_table_entries_past_end_return_err() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    buf.extend_from_slice(&[0u8; 32]);
    assert!(parse(&buf).is_err());
}

#[test]
fn empty_count_is_bounded_ok() {
    let buf = 0u32.to_le_bytes();
    let pack = parse(&buf).expect("count=0 is a valid empty pack");
    assert_eq!(pack.count, 0);
    assert!(pack.records.is_empty());
}

#[test]
fn preamble_on_short_buffer_returns_err() {
    for len in 0..0x10 {
        let buf = vec![0u8; len];
        assert!(Preamble::from_bytes(&buf).is_err());
        assert!(peel_preamble(&buf).is_err());
    }
}

#[test]
fn preamble_with_huge_expanded_size_returns_err() {
    let mut buf = vec![0u8; 0x10 + 16];
    // expanded_size at +0x0C = enormous value.
    buf[0x0C..0x10].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    assert!(peel_preamble(&buf).is_err());
}

#[test]
fn keyframe_reader_rejects_huge_bone_count() {
    // bone_count * (8 + 24) would overflow / overrun a tiny record.
    let record = vec![0u8; 64];
    assert!(KeyframeReader::parse(&record, usize::MAX).is_err());
    assert!(KeyframeReader::parse(&record, 100_000).is_err());
}

#[test]
fn bone_keyframe_short_buffer_returns_err() {
    for len in 0..BoneKeyframe::SIZE {
        let buf = vec![0u8; len];
        assert!(BoneKeyframe::from_bytes(&buf).is_err());
    }
}

#[test]
fn keyframe_reader_below_header_size_returns_err() {
    for len in 0..RECORD_HEADER_SIZE {
        let buf = vec![0u8; len];
        assert!(KeyframeReader::parse(&buf, 0).is_err());
    }
}

#[test]
fn random_data_parse_is_panic_free() {
    let mut buf = vec![0u8; 2048];
    let mut x: u32 = 0xCAFE_BABE;
    for b in buf.iter_mut() {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        *b = (x >> 16) as u8;
    }
    // Whatever the result, it must not panic.
    let _ = parse(&buf);
}
