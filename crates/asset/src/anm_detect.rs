//! On-disc ANM (asset type 0x06) detector.
//!
//! Wraps `legaia_anm::parse` with a strict shape check so the categorizer can
//! recognize ANM containers in raw PROT entries (where the 16-byte allocator
//! preamble that wraps RAM-extracted blobs is absent).
//!
//! Strict criteria:
//!
//! 1. Leading u32 `count` is in `1..=MAX_REASONABLE_COUNT` (4096).
//! 2. Offset table fits in the buffer (`4 + count * 4 <= len`).
//! 3. Every offset:
//!    - lands strictly past the end of the offset table,
//!    - is `<= len`,
//!    - is monotonically non-decreasing.
//! 4. Every record is at least `RECORD_HEADER_SIZE` (8) bytes.
//! 5. **Every record's `marker_1` u16 at `+4..+6` equals `0x080C`.** This is
//!    the load-bearing signature — it's been observed identical across all 93
//!    ANM records in two independent overlay captures.
//!
//! The marker check is what makes this detector zero-false-positive: random
//! `count + offset table + payload` shapes don't accidentally produce
//! `0x080C` u16s at every record's `+4..+6`.

use legaia_anm::{RECORD_HEADER_SIZE, RECORD_MARKER_1, parse};

/// Detect result. The contained `record_count` matches `legaia_anm::AnmPack::count`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnmDetect {
    /// Number of records.
    pub record_count: usize,
    /// Total payload size that parsed.
    pub size: usize,
}

/// Returns `Some(AnmDetect)` if `buf` is recognized as an ANM payload.
pub fn detect(buf: &[u8]) -> Option<AnmDetect> {
    let pack = parse(buf).ok()?;
    if pack.count == 0 {
        return None;
    }
    for rec in &pack.records {
        if rec.size < RECORD_HEADER_SIZE {
            return None;
        }
        let off = rec.offset;
        let marker = u16::from_le_bytes([buf[off + 4], buf[off + 5]]);
        if marker != RECORD_MARKER_1 {
            return None;
        }
    }
    Some(AnmDetect {
        record_count: pack.count,
        size: pack.payload_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic ANM payload from per-record bytes. Each record gets a
    /// canonical `[a=0x0A][b=0x1E][marker_1=0x080C][flag=0x0002]` header
    /// followed by `body`.
    fn synthetic(records: &[&[u8]]) -> Vec<u8> {
        let count = records.len();
        let table_end = 4 + count * 4;
        let mut full_records: Vec<Vec<u8>> = Vec::with_capacity(count);
        for body in records {
            let mut rec = vec![0u8; RECORD_HEADER_SIZE];
            rec[0] = 0x0A;
            rec[2] = 0x1E;
            rec[4..6].copy_from_slice(&RECORD_MARKER_1.to_le_bytes());
            rec[6..8].copy_from_slice(&0x0002u16.to_le_bytes());
            rec.extend_from_slice(body);
            full_records.push(rec);
        }
        let mut offs = Vec::with_capacity(count);
        let mut acc = table_end;
        for r in &full_records {
            offs.push(acc);
            acc += r.len();
        }
        let mut out = Vec::with_capacity(acc);
        out.extend_from_slice(&(count as u32).to_le_bytes());
        for o in &offs {
            out.extend_from_slice(&(*o as u32).to_le_bytes());
        }
        for r in &full_records {
            out.extend_from_slice(r);
        }
        out
    }

    #[test]
    fn fires_on_well_formed_anm() {
        let buf = synthetic(&[&[0xAA; 16], &[0xBB; 32], &[0xCC; 8]]);
        let d = detect(&buf).expect("should detect");
        assert_eq!(d.record_count, 3);
        assert_eq!(d.size, buf.len());
    }

    #[test]
    fn rejects_when_marker_mismatched() {
        let mut buf = synthetic(&[&[0xAA; 16]]);
        // Stomp the first record's marker_1.
        let off = u32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize;
        buf[off + 4] = 0xFF;
        buf[off + 5] = 0xFF;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_random_bytes() {
        let buf: Vec<u8> = (0..256u32)
            .map(|i| (i.wrapping_mul(7) & 0xFF) as u8)
            .collect();
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_empty_count() {
        let buf = 0u32.to_le_bytes().to_vec();
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_too_short_record() {
        // count=1, offset = table_end (= 8). Record body is just 4 bytes —
        // shorter than RECORD_HEADER_SIZE. Detector must reject.
        let mut buf = vec![];
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&8u32.to_le_bytes());
        buf.extend_from_slice(&[0u8; 4]);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn accepts_alternate_flag() {
        // ANM headers observed with flag = 0x0002 OR 0x0004. The detector
        // only checks marker_1, not flag — so flag = 0x0004 should still fire.
        let mut buf = synthetic(&[&[0; 16]]);
        let off = u32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize;
        buf[off + 6] = 0x04;
        buf[off + 7] = 0x00;
        assert!(detect(&buf).is_some());
    }
}
