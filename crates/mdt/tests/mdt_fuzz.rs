//! Panic-hardening regression suite for the `legaia-mdt` parser entry points.
//!
//! Bulk scanners feed arbitrary LZS-decoded blocks into `MoveBuffer::parse`,
//! `RecordTable::parse`, and `classify` to decide "is this a Move buffer?".
//!
//! `MoveBuffer::parse` is intentionally a best-effort, over-reading parser
//! (the project uses `looks_like_move_buffer()` rather than a strict fitness
//! check on retail tables; see `docs/formats/mdt.md`). This suite does NOT
//! tighten that behavior - it only asserts that malformed / truncated / junk
//! / huge-count input can never PANIC (OOB slice, capacity overflow, or
//! arithmetic over/underflow). A best-effort `Ok` or an `Err` are both fine.
//!
//! Every input here is SYNTHETIC; the suite runs with `LEGAIA_DISC_BIN` unset.

use legaia_mdt::{MoveBuffer, RecordTable, classify};

fn adversarial_inputs() -> Vec<Vec<u8>> {
    // Degenerate sizes (below the 4-byte minimum and just above it).
    let mut v: Vec<Vec<u8>> = vec![
        vec![],
        vec![0x00],
        vec![0x00, 0x00, 0x00],
        vec![0xFF; 4],
        vec![0xFF; 5],
    ];

    // Offset tables full of huge / wrapping offsets: every u32 points way past
    // EOF. A naive `Vec::with_capacity(offset)` or unguarded `buf[offset..]`
    // would crash here.
    for &fill in &[0xFFu8, 0x80, 0x7F] {
        v.push(vec![fill; 64]);
        v.push(vec![fill; 4096]);
        v.push(vec![fill; 4099]);
    }

    // A table whose offsets point *just* short of / past EOF and into the
    // record-header read window (`o + 7`), to exercise the `o + 7 > buf.len()`
    // guard in MoveBuffer::parse.
    {
        let mut b = vec![0u8; 64];
        // id 0 -> offset 60 (record header would read bytes 60..67, past 64).
        b[0..4].copy_from_slice(&60u32.to_le_bytes());
        // id 1 -> offset = len exactly.
        b[4..8].copy_from_slice(&64u32.to_le_bytes());
        // id 2 -> offset = len + 1 (past end).
        b[8..12].copy_from_slice(&65u32.to_le_bytes());
        v.push(b);
    }

    // Pseudo-random streams of several lengths.
    for &n in &[7usize, 37, 128, 1024, 4099, 65_540] {
        let mut s: u32 = 0xBADD_CAFE ^ (n as u32);
        v.push(
            (0..n)
                .map(|_| {
                    s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (s >> 24) as u8
                })
                .collect(),
        );
    }

    // All-zero buffers of various sizes (every offset is 0 -> no used slots).
    for &n in &[4usize, 128, legaia_mdt::RECORD_STRIDE * 4, 4096] {
        v.push(vec![0x00; n]);
    }

    v
}

#[test]
fn move_buffer_parse_never_panics() {
    for buf in adversarial_inputs() {
        // `parse` may return Err (too small) or a best-effort Ok; either is
        // acceptable as long as it does not panic.
        if let Ok(mb) = MoveBuffer::parse(&buf) {
            // Best-effort invariants: counts are bounded by the buffer.
            assert!(mb.table_entries <= buf.len() / 4 + 1);
            assert!(mb.used_slots.len() <= mb.table_entries);
            // looks_like_move_buffer must also be panic-free.
            let _ = mb.looks_like_move_buffer();
            let _ = mb.fitness();
        }
    }
}

#[test]
fn move_buffer_parse_with_table_size_never_panics() {
    for buf in adversarial_inputs() {
        for &ts in &[0usize, 1, 16, 1024, usize::MAX] {
            let _ = MoveBuffer::parse_with_table_size(&buf, ts);
        }
    }
}

#[test]
fn record_table_parse_never_panics() {
    for buf in adversarial_inputs() {
        let rt = RecordTable::parse(&buf);
        // record_count is derived by integer division; it must be exact.
        assert_eq!(rt.record_count, buf.len() / rt.stride);
        let _ = rt.non_empty_count();
    }
}

#[test]
fn classify_never_panics() {
    for buf in adversarial_inputs() {
        // `classify` returns Err only on the < 4-byte MoveBuffer guard.
        if let Ok(c) = classify(&buf) {
            assert_eq!(c.size, buf.len());
        }
    }
}
