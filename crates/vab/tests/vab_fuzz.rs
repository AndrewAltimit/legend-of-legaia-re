//! Panic-hardening regression tests for the VAB parser entry points.
//!
//! The web viewer / per-entry inspector feeds ARBITRARY bytes into these
//! parsers; a junk match must return `Err`, never panic. Every fixture
//! here is hand-constructed synthetic data - zero real game bytes - and
//! the suite passes with `LEGAIA_DISC_BIN` unset.

use legaia_vab::{
    PROGRAMS_TABLE_SIZE, TONE_SIZE, TONES_PER_PROGRAM, VAB_HEADER_SIZE, VAB_MAGIC, VAG_BLOCK_BYTES,
    VAG_TABLE_ENTRIES, decode_vag, find_vabs, parse, parse_header,
};

/// Build a VAB header with caller-chosen `ps`/`vs`/`fsize`. Only the
/// header bytes are written; the rest of `len` is zero-filled. Used to
/// construct hostile blobs whose declared sizes don't match the buffer.
fn header_only(ps: u16, vs: u16, fsize: u32, len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len.max(VAB_HEADER_SIZE)];
    buf[0..4].copy_from_slice(&VAB_MAGIC.to_le_bytes());
    buf[4..8].copy_from_slice(&7u32.to_le_bytes());
    buf[12..16].copy_from_slice(&fsize.to_le_bytes());
    buf[18..20].copy_from_slice(&ps.to_le_bytes());
    buf[22..24].copy_from_slice(&vs.to_le_bytes());
    buf
}

#[test]
fn empty_and_tiny_inputs_do_not_panic() {
    for len in 0..=VAB_HEADER_SIZE {
        let buf = vec![0u8; len];
        // Junk (all-zero) headers fail the magic check; never panic.
        assert!(parse_header(&buf, 0).is_err());
        assert!(parse(&buf, 0).is_err());
        // find_vabs must tolerate any short buffer.
        let _ = find_vabs(&buf);
    }
}

#[test]
fn offset_past_end_returns_err() {
    let buf = header_only(2, 1, 64, 256);
    assert!(parse_header(&buf, buf.len()).is_err());
    assert!(parse_header(&buf, buf.len() + 9999).is_err());
    assert!(parse(&buf, buf.len() + 1).is_err());
}

#[test]
fn valid_magic_but_truncated_sections_returns_err_not_panic() {
    // Valid magic + plausible ps/vs, but the buffer is far smaller than the
    // fixed program/tone/table layout demands. Before hardening this could
    // slice past the buffer end and panic.
    let buf = header_only(128, 200, VAB_HEADER_SIZE as u32, VAB_HEADER_SIZE + 8);
    assert!(parse(&buf, 0).is_err());
}

#[test]
fn fsize_smaller_than_layout_returns_err() {
    // fsize claims the whole blob is just the header, so `offset + fsize`
    // fits the buffer, but the section slices need far more. Must Err.
    let len = 4096;
    let buf = header_only(64, 16, VAB_HEADER_SIZE as u32, len);
    assert!(parse(&buf, 0).is_err());
}

#[test]
fn vs_at_table_capacity_returns_err() {
    // vs == VAG_TABLE_ENTRIES (256) would index vag_table[256], one past
    // the 256-entry table. Must be rejected by the range check.
    let buf = header_only(1, VAG_TABLE_ENTRIES as u16, 64, 8192);
    assert!(parse_header(&buf, 0).is_err());
    // vs just under the cap parses the header (sample table may still Err
    // depending on sizes, but parse_header itself accepts it).
    let buf_ok = header_only(1, (VAG_TABLE_ENTRIES - 1) as u16, 64, 8192);
    assert!(parse_header(&buf_ok, 0).is_ok());
}

#[test]
fn bogus_huge_program_count_returns_err() {
    for ps in [0u16, 129, 256, 1000, 0xFFFF] {
        let buf = header_only(ps, 1, 64, 8192);
        assert!(parse_header(&buf, 0).is_err(), "ps={ps} should be rejected");
    }
}

#[test]
fn bogus_version_returns_err() {
    let mut buf = header_only(2, 1, 64, 8192);
    buf[4..8].copy_from_slice(&999u32.to_le_bytes());
    assert!(parse_header(&buf, 0).is_err());
}

#[test]
fn sample_size_overruns_vab_region_returns_err() {
    // Build a layout-valid VAB but make sample[0] claim a huge size via the
    // VAG offset table so it overruns the declared region.
    let ps = 1usize;
    let layout = VAB_HEADER_SIZE
        + PROGRAMS_TABLE_SIZE
        + TONE_SIZE * TONES_PER_PROGRAM * ps
        + 2 * VAG_TABLE_ENTRIES;
    let len = layout + 64;
    let mut buf = header_only(ps as u16, 1, len as u32, len);
    // VAG table entry 1 = huge size in 8-byte units.
    let table_off = VAB_HEADER_SIZE + PROGRAMS_TABLE_SIZE + TONE_SIZE * TONES_PER_PROGRAM * ps;
    buf[table_off + 2..table_off + 4].copy_from_slice(&0xFFFFu16.to_le_bytes());
    assert!(parse(&buf, 0).is_err());
}

#[test]
fn decode_vag_rejects_non_block_multiple_length() {
    for len in [1usize, 15, 17, 31] {
        let buf = vec![0u8; len];
        assert!(decode_vag(&buf).is_err(), "len={len} should Err");
    }
}

#[test]
fn decode_vag_tolerates_garbage_shift_and_filter() {
    // Hostile blocks: every byte 0xFF (filter=15, shift=15, end flag set,
    // garbage nibbles). A naive `s << (12 - shift)` would panic on the
    // negative shift; the hardened decoder must return Ok without panic.
    let buf = vec![0xFFu8; VAG_BLOCK_BYTES * 4];
    let pcm = decode_vag(&buf).expect("garbage must decode without panic");
    // First block has the end flag + garbage filter, so decoding stops
    // immediately and yields no samples.
    assert!(pcm.is_empty());
}

#[test]
fn decode_vag_high_shift_no_end_flag_does_not_panic() {
    // shift=15 (header low nibble), filter=0, NO end flag, valid filter -
    // forces the `>> shift` path with a large shift. Must not panic.
    let mut buf = vec![0u8; VAG_BLOCK_BYTES * 2];
    buf[0] = 0x0F; // filter=0, shift=15
    buf[1] = 0x00; // no flags
    // Fill the data nibbles with non-zero junk.
    for b in buf[2..16].iter_mut() {
        *b = 0xA5;
    }
    let pcm = decode_vag(&buf).expect("high shift must not panic");
    // Both blocks are valid (filter 0, no end flag): the high-shift block
    // plus the trailing all-zero block each yield 28 samples. The point of
    // the test is the absence of a shift panic, not the exact count.
    assert_eq!(pcm.len(), 28 * 2);
}

#[test]
fn find_vabs_on_random_data_is_bounded_and_panic_free() {
    // Pseudo-random junk; just assert it terminates and returns sane hits.
    let mut buf = vec![0u8; 4096];
    let mut x: u32 = 0x1234_5678;
    for b in buf.iter_mut() {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        *b = (x >> 16) as u8;
    }
    let hits = find_vabs(&buf);
    for &h in &hits {
        assert!(h + 4 <= buf.len());
    }
}
