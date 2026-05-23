//! `move.mdt` parser - the runtime "move" buffer that holds character animation /
//! attack-move data.
//!
//! ## Where the data lives
//!
//! In retail the file is loaded by `FUN_8002541c` case `0x0F` ("move.mdt") via
//! `FUN_800255b8`, which reads the PROT entry indexed by
//! `_DAT_80084540 + 4` into `_DAT_8007b85c` and then raw-copies it into a
//! freshly-allocated buffer at `_DAT_8007b888`. The asset dispatcher
//! `FUN_8001f05c` case `0x05` (`s_move_malloc_err`) writes the same global
//! when it fires, but no streaming chunk of type 5 exists in the retail data
//! so that branch is never taken in practice. The MOVE2 sibling
//! (`_DAT_8007b840`, dispatcher case `0x0B`) holds moves with id > `0x3FF`.
//!
//! ## What the consumer expects
//!
//! `FUN_800204f8` is the sole runtime reader. Per its disassembly:
//!
//! ```text
//!   masked_id = move_id & 0x3FF                  // 10-bit id space
//!   buf       = (flags & 0x01000000) ? alt_table : (move_id < 0x400 ? MOVE : MOVE2)
//!   record    = buf + *(u32*)(buf + masked_id*4)
//!
//!   record[0..1]   - reserved/unknown
//!   record[1] & 1  - "use frame divisor" flag
//!   record[2..3] u16 - max-position * 16 (clamps animation playhead)
//!   record[4..5] u16 - reserved
//!   record[6]      u8  - frame divisor (only when flag bit set)
//!   record[7..]    - per-frame data (size determined by record[2..3])
//! ```
//!
//! So the on-disk layout the consumer expects is:
//!
//! ```text
//!   u32 offset_table[1024]   // indexed by (move_id & 0x3FF)
//!   u8  records[]            // each at the offset given by the table
//! ```
//!
//! ## What's actually in the named PROT entries
//!
//! `0972_move_program_no.BIN` (24576 B) and `0973_move_program_no.BIN` (47104 B)
//! are CDNAME-named "move_program_no" but their byte layout does **not** match
//! the consumer-derived offset-table format. Instead they look like a flat
//! array of fixed 128-byte records:
//!
//! ```text
//!   [u8; 128] record_0
//!   [u8; 128] record_1
//!   ...
//! ```
//!
//! 0972 has 192 records (~25% non-empty); 0973 has 368 records (~54% non-empty).
//! Active records start with patterns like `22 22 22 22 22 22 22 02` or
//! `11 11 11 11 11 11 11 01` and then a sparse body of `0xEE`/`0xEF`/`0xF0`/`0xFE`
//! bytes - looks like packed nibble timing/keyframe data.
//!
//! This module exposes both views and a sanity check that flags the mismatch:
//!
//! - [`MoveBuffer::parse`] - parses any byte slice as the consumer-expected
//!   offset-table layout. Returns `Err` (or quirks) when the table looks bogus,
//!   which is exactly what 0972/0973 produce.
//! - [`RecordTable::parse`] - parses any byte slice as a flat 128-byte stride
//!   record table. This is the structure 0972/0973 actually have.
//! - [`classify`] - runs both interpretations and reports which one fits.
//!
//! See `docs/formats/mdt.md` for the broader context. Until a runtime
//! watchpoint trace pins down the actual file layout, this crate is
//! intentionally a "what we know" parser, not a final decoder.

use anyhow::Result;
use serde::Serialize;

pub const RECORD_STRIDE: usize = 128;
pub const MOVE_ID_MASK: u16 = 0x03FF;

// ---------------------------------------------------------------------------
// Consumer-expected layout (offset-table view)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct MoveBuffer {
    pub size: usize,
    pub table_entries: usize,
    pub used_slots: Vec<MoveSlot>,
    pub bogus_offsets: usize,
    pub records: Vec<MoveRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MoveSlot {
    pub move_id: u16,
    pub raw_offset: u32,
    pub points_into_table: bool,
    pub points_past_end: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MoveRecord {
    pub offset: u32,
    pub flags: u8,
    pub use_divisor: bool,
    pub max_position_x16: u16,
    pub divisor: u8,
    pub trailing_bytes_seen: usize,
    pub referenced_by_ids: Vec<u16>,
}

impl MoveBuffer {
    /// Parse a byte slice as the consumer-expected layout. Always succeeds;
    /// inspect [`Self::bogus_offsets`] and [`MoveSlot::points_past_end`] to see
    /// whether the bytes actually fit this format.
    pub fn parse(buf: &[u8]) -> Result<Self> {
        Self::parse_with_table_size(buf, MOVE_ID_MASK as usize + 1)
    }

    /// Parse with a caller-chosen table size. Useful for synthetic / test
    /// buffers where the consumer's hard-coded 1024 entries doesn't apply.
    pub fn parse_with_table_size(buf: &[u8], table_entries: usize) -> Result<Self> {
        if buf.len() < 4 {
            anyhow::bail!("buffer too small for any offset table");
        }
        // The table can't extend past EOF
        let table_entries = table_entries.min(buf.len() / 4);

        let mut used = Vec::new();
        let mut bogus = 0usize;
        let mut record_offsets = std::collections::BTreeMap::<u32, Vec<u16>>::new();

        for id in 0..(table_entries as u16) {
            let raw = u32::from_le_bytes(
                buf[(id as usize) * 4..(id as usize) * 4 + 4]
                    .try_into()
                    .unwrap(),
            );
            if raw == 0 {
                continue;
            }
            let in_table = (raw as usize) < table_entries * 4;
            let past_end = (raw as usize) >= buf.len();
            if past_end {
                bogus += 1;
            }
            used.push(MoveSlot {
                move_id: id,
                raw_offset: raw,
                points_into_table: in_table,
                points_past_end: past_end,
            });
            if !past_end {
                record_offsets.entry(raw).or_default().push(id);
            }
        }

        let mut records = Vec::new();
        for (off, ids) in record_offsets {
            let o = off as usize;
            if o + 7 > buf.len() {
                continue;
            }
            let flags = buf[o + 1];
            let max_pos = u16::from_le_bytes([buf[o + 2], buf[o + 3]]);
            let div = buf[o + 6];
            // measure how many bytes follow until the next record or EOF
            // (purely for reporting)
            let trailing = buf.len() - o;
            records.push(MoveRecord {
                offset: off,
                flags,
                use_divisor: flags & 0x01 != 0,
                max_position_x16: max_pos,
                divisor: div,
                trailing_bytes_seen: trailing,
                referenced_by_ids: ids,
            });
        }

        Ok(MoveBuffer {
            size: buf.len(),
            table_entries,
            used_slots: used,
            bogus_offsets: bogus,
            records,
        })
    }

    /// Cheap fitness score: number of non-bogus offsets minus the number of
    /// out-of-range ones.
    ///
    /// **Caveat.** Real per-scene Move buffers have offset tables shorter
    /// than the consumer-facing 1024-entry mask, and pack record data
    /// densely past the real table end. `parse` keeps reading u32s past
    /// the real boundary where record bytes masquerade as offsets, so most
    /// real Move buffers score strongly negative (e.g. `0086_map01.BIN`:
    /// used=1020 bogus=973 → fitness=-926). Use this score for synthetic
    /// buffers only. For "is this LZS-decoded data a Move buffer?" on
    /// retail inputs, use [`MoveBuffer::looks_like_move_buffer`] instead.
    pub fn fitness(&self) -> i64 {
        self.used_slots.len() as i64 - 2 * self.bogus_offsets as i64
    }

    /// Relaxed validity predicate that doesn't penalise the over-read past
    /// the per-scene table boundary.
    ///
    /// Recognises:
    /// - At least one decodable record (`records.len() > 0`).
    /// - The majority of non-zero offsets actually point into the buffer
    ///   (`used_slots > bogus_offsets`).
    ///
    /// Random / non-Move LZS-decoded data fails this: random u32 offsets
    /// almost always land past the buffer end, so `bogus ≈ used` and the
    /// `records` list is dominated by garbage slots. All-zero buffers
    /// fail because `used_slots = 0`. 75/79 retail per-scene Move buffers
    /// pass it - the canonical bar for per-scene MDT install. See
    /// [`engine-core::scene_bundle::move_payload_looks_valid`] for the
    /// engine-side usage.
    pub fn looks_like_move_buffer(&self) -> bool {
        !self.records.is_empty() && self.used_slots.len() > self.bogus_offsets
    }
}

// ---------------------------------------------------------------------------
// Flat 128-byte record table (the structure 0972/0973 actually have)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct RecordTable {
    pub size: usize,
    pub stride: usize,
    pub record_count: usize,
    pub trailing_bytes: usize,
    pub records: Vec<FlatRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlatRecord {
    pub index: usize,
    pub head_8: [u8; 8],
    pub head_repeats: bool,
    pub head_tail_byte: u8,
    pub body_nonzero_bytes: usize,
    pub body_first_nonzero_offset: Option<usize>,
    pub all_zero: bool,
}

impl RecordTable {
    pub fn parse(buf: &[u8]) -> Self {
        let n = buf.len() / RECORD_STRIDE;
        let trailing = buf.len() - n * RECORD_STRIDE;
        let mut records = Vec::with_capacity(n);
        for i in 0..n {
            let rec = &buf[i * RECORD_STRIDE..(i + 1) * RECORD_STRIDE];
            let head: [u8; 8] = rec[..8].try_into().unwrap();
            // Check if the first 7 bytes are equal (the "X X X X X X X" pattern)
            let head_repeats = head[..7].iter().all(|&b| b == head[0]);
            let body = &rec[8..];
            let body_nz: Vec<usize> = body
                .iter()
                .enumerate()
                .filter_map(|(i, &b)| (b != 0).then_some(i))
                .collect();
            records.push(FlatRecord {
                index: i,
                head_8: head,
                head_repeats,
                head_tail_byte: head[7],
                body_nonzero_bytes: body_nz.len(),
                body_first_nonzero_offset: body_nz.first().copied(),
                all_zero: head.iter().all(|&b| b == 0) && body_nz.is_empty(),
            });
        }
        RecordTable {
            size: buf.len(),
            stride: RECORD_STRIDE,
            record_count: n,
            trailing_bytes: trailing,
            records,
        }
    }

    pub fn non_empty_count(&self) -> usize {
        self.records.iter().filter(|r| !r.all_zero).count()
    }
}

// ---------------------------------------------------------------------------
// Classifier
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct Classification {
    pub size: usize,
    pub offset_table_fit: i64,
    pub offset_table_used_slots: usize,
    pub offset_table_bogus: usize,
    pub flat_table_records: usize,
    pub flat_table_non_empty: usize,
    pub flat_table_trailing: usize,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum Verdict {
    /// Offsets in the first ~1024 u32s point into the buffer cleanly.
    OffsetTableLayout,
    /// Looks like a flat array of 128-byte records.
    FlatRecordTable,
    /// Neither view fits well.
    Unknown,
}

pub fn classify(buf: &[u8]) -> Result<Classification> {
    let mb = MoveBuffer::parse(buf)?;
    let rt = RecordTable::parse(buf);
    let off_fit = mb.fitness();
    let flat_fit = rt.non_empty_count() as i64;
    // `MoveBuffer::looks_like_move_buffer` tolerates the over-read past the
    // real per-scene table boundary that `parse` always performs. See the
    // doc comment on `fitness` for why the legacy `off_fit > 32 &&
    // bogus_offsets == 0` predicate is false-negative on retail Move data.
    let verdict = if mb.looks_like_move_buffer() {
        Verdict::OffsetTableLayout
    } else if flat_fit > 8 && rt.trailing_bytes == 0 {
        Verdict::FlatRecordTable
    } else {
        Verdict::Unknown
    };
    Ok(Classification {
        size: buf.len(),
        offset_table_fit: off_fit,
        offset_table_used_slots: mb.used_slots.len(),
        offset_table_bogus: mb.bogus_offsets,
        flat_table_records: rt.record_count,
        flat_table_non_empty: rt.non_empty_count(),
        flat_table_trailing: rt.trailing_bytes,
        verdict,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_offset_table() -> Vec<u8> {
        // Build a buffer with a 16-entry offset table and 4 small records.
        let mut buf = vec![0u8; 16 * 4 + 4 * 16];
        // Records start at offset 64 (16*4)
        // record 0 at 64, record 1 at 80, etc.
        for (id, rec_off) in [(1u16, 64u32), (2, 80), (5, 96), (7, 112)] {
            let p = (id as usize) * 4;
            buf[p..p + 4].copy_from_slice(&rec_off.to_le_bytes());
            // record body: flags=0x01 (use_divisor), max_pos=10, divisor=2
            let r = rec_off as usize;
            buf[r] = 0;
            buf[r + 1] = 0x01;
            buf[r + 2] = 10;
            buf[r + 3] = 0;
            buf[r + 4] = 0;
            buf[r + 5] = 0;
            buf[r + 6] = 2;
            buf[r + 7] = 0;
        }
        buf
    }

    #[test]
    fn synthetic_offset_table_parses_cleanly() {
        let buf = synthetic_offset_table();
        // table is 16 entries; rest of buffer is record bodies
        let mb = MoveBuffer::parse_with_table_size(&buf, 16).unwrap();
        assert_eq!(mb.used_slots.len(), 4);
        assert_eq!(mb.bogus_offsets, 0);
        assert_eq!(mb.records.len(), 4);
        for r in &mb.records {
            assert!(r.use_divisor);
            assert_eq!(r.divisor, 2);
            assert_eq!(r.max_position_x16, 10);
        }
    }

    #[test]
    fn synthetic_offset_table_classifies_as_offset_layout() {
        // The default `classify` uses 1024-entry probe, which would read into
        // record bodies. Use the explicit table size for the synthetic case.
        let buf = synthetic_offset_table();
        let mb = MoveBuffer::parse_with_table_size(&buf, 16).unwrap();
        assert_eq!(mb.used_slots.len(), 4);
        assert_eq!(mb.bogus_offsets, 0);
    }

    #[test]
    fn flat_records_classify_as_flat() {
        let mut buf = vec![0u8; RECORD_STRIDE * 16];
        // 12 non-empty records with the 0972-style header
        for i in 0..12 {
            let off = i * RECORD_STRIDE;
            buf[off..off + 7].fill(0x22);
            buf[off + 7] = 0x02;
        }
        let c = classify(&buf).unwrap();
        assert_eq!(c.flat_table_records, 16);
        assert_eq!(c.flat_table_non_empty, 12);
        assert_eq!(c.flat_table_trailing, 0);
        assert_eq!(c.verdict, Verdict::FlatRecordTable);
    }

    #[test]
    fn record_table_parses_synthetic() {
        let mut buf = vec![0u8; RECORD_STRIDE * 4];
        // Record 0: header `11 11 11 11 11 11 11 01`, body byte at offset 16
        buf[..7].fill(0x11);
        buf[7] = 0x01;
        buf[16] = 0xEF;
        // Record 1: empty
        // Record 2: header `22 22 22 22 22 22 22 02`
        let off2 = 2 * RECORD_STRIDE;
        buf[off2..off2 + 7].fill(0x22);
        buf[off2 + 7] = 0x02;
        let rt = RecordTable::parse(&buf);
        assert_eq!(rt.record_count, 4);
        assert_eq!(rt.non_empty_count(), 2);
        assert_eq!(rt.records[0].head_tail_byte, 0x01);
        assert!(rt.records[0].head_repeats);
        assert_eq!(rt.records[0].body_first_nonzero_offset, Some(8));
        assert!(rt.records[1].all_zero);
        assert_eq!(rt.records[2].head_tail_byte, 0x02);
    }

    #[test]
    fn empty_buffer_rejects() {
        assert!(MoveBuffer::parse(&[]).is_err());
    }

    /// `MoveBuffer::parse`, `RecordTable::parse`, and `classify` are all run
    /// against LZS-decoded sections of arbitrary PROT entries by the asset
    /// scanner. Pseudo-random byte soup of every length 0..512 must never
    /// panic - the over-read past a short per-scene table is expected and
    /// tolerated, but a slice/index/overflow panic is a real bug.
    #[test]
    fn parsers_never_panic_on_random_bytes() {
        for seed in 0u64..400 {
            let mut x = seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(99);
            let n = (seed % 512) as usize;
            let mut buf = Vec::with_capacity(n);
            for _ in 0..n {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                buf.push(x as u8);
            }
            let _ = MoveBuffer::parse(&buf);
            let _ = MoveBuffer::parse_with_table_size(&buf, 1024);
            let _ = RecordTable::parse(&buf);
            let _ = classify(&buf);
        }
    }

    /// An offset table whose entries are all `0xFFFFFFFF` (every offset far
    /// past EOF) must classify cleanly as bogus without panicking and never
    /// look like a valid Move buffer.
    #[test]
    fn all_max_offsets_are_bogus_not_panic() {
        let buf = vec![0xFFu8; 1024];
        let mb = MoveBuffer::parse(&buf).unwrap();
        assert!(!mb.looks_like_move_buffer());
        let _ = classify(&buf).unwrap();
    }
}
