//! Panic-hardening regression tests for the MES parser entry points.
//!
//! The web viewer / per-entry inspector feeds ARBITRARY bytes into these
//! parsers; a junk match must return `Err` (or a bounded `Ok`), never
//! panic. Every fixture is hand-constructed synthetic data and the suite
//! passes with `LEGAIA_DISC_BIN` unset.

use legaia_mes::{
    COMPACT_MAGIC, RECORD_MARKER, detect_format, extract_all_messages, extract_message,
    iter_tokens, parse,
};

#[test]
fn empty_and_tiny_inputs_do_not_panic() {
    for len in 0..8 {
        let buf = vec![0u8; len];
        // Detection on tiny junk: never panic; parse Errs (no format match).
        let _ = detect_format(&buf);
        assert!(parse(&buf).is_err());
        // Token iteration over any buffer must terminate without panic.
        let _ = iter_tokens(&buf, 0).count();
    }
}

#[test]
fn compact_magic_but_truncated_body_returns_err() {
    // Valid magic, but the buffer is smaller than the fixed compact layout
    // (offset table ends at 0xC8). Must Err, not slice OOB.
    for len in 4..0xC8 {
        let mut buf = vec![0u8; len];
        buf[0..4].copy_from_slice(&COMPACT_MAGIC.to_le_bytes());
        assert!(parse(&buf).is_err(), "len={len} should be too small");
    }
}

#[test]
fn records_format_with_garbage_body_parses_without_panic() {
    // >=4 markers triggers Records detection; the body is otherwise junk.
    let mut buf = vec![0x99u8; 512];
    for i in (10..210).step_by(40) {
        buf[i] = RECORD_MARKER[0];
        buf[i + 1] = RECORD_MARKER[1];
    }
    let blob = parse(&buf).expect("records format should parse structurally");
    assert!(blob.records.is_some());
}

#[test]
fn extract_message_out_of_range_index_returns_err() {
    // Build a minimal compact blob (offset table all-zero), then ask for a
    // wildly out-of-range message index.
    let mut buf = vec![0u8; 0x200];
    buf[0..4].copy_from_slice(&COMPACT_MAGIC.to_le_bytes());
    assert!(extract_message(&buf, 9999).is_err());
    // extract_all_messages must not panic even on a degenerate table.
    let _ = extract_all_messages(&buf);
}

#[test]
fn extract_message_offset_past_end_returns_err() {
    // Put a huge u24 offset in the first offset-table slot so the computed
    // pc lands past the buffer end. Must Err, not index OOB.
    let mut buf = vec![0u8; 0x100];
    buf[0..4].copy_from_slice(&COMPACT_MAGIC.to_le_bytes());
    // OFFSET_TABLE_OFFSET = 0x62; write 0xFFFFFF (u24).
    buf[0x62] = 0xFF;
    buf[0x63] = 0xFF;
    buf[0x64] = 0xFF;
    assert!(extract_message(&buf, 0).is_err());
}

#[test]
fn iter_tokens_start_past_end_yields_nothing() {
    let buf = [0x21u8, 0x22, 0x00];
    assert_eq!(iter_tokens(&buf, 999).count(), 0);
    assert_eq!(iter_tokens(&buf, buf.len()).count(), 0);
}

#[test]
fn iter_tokens_truncated_two_byte_opcode_at_end() {
    // Lone 2-byte opcode with no arg byte must surface a Truncated token,
    // not read past the buffer.
    for op in [
        0xC1u8, 0xC2, 0xC3, 0xC4, 0xC5, 0xC7, 0xCE, 0xCF, 0xC0, 0x5E, 0xFF,
    ] {
        let buf = [op];
        let toks: Vec<_> = iter_tokens(&buf, 0).collect();
        assert_eq!(toks.len(), 1, "op={op:#x} should yield exactly one token");
    }
}

#[test]
fn random_data_is_panic_free() {
    let mut buf = vec![0u8; 4096];
    let mut x: u32 = 0xDEAD_BEEF;
    for b in buf.iter_mut() {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        *b = (x >> 16) as u8;
    }
    let _ = detect_format(&buf);
    let _ = parse(&buf);
    let _ = iter_tokens(&buf, 0).count();
}
