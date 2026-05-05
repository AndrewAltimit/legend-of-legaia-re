//! Stage-geometry record detector.
//!
//! Reverse-engineered from PROT entries in the 83968-byte cluster (formerly
//! "Cluster A" in COVERAGE.md). Many stage entries — `town01`, `dolk`,
//! `garmel`, `bylon`, `rugi`, `izumi`, … — embed a table of fixed-stride
//! records:
//!
//! ```text
//! +0   12 bytes : fixed prefix `00 F0 84 7F 01 F0 1F 00 00 F1 00 00`
//! +12   8 bytes : 4 × u16 payload (looks like 8-aligned vertex/face indices)
//! ```
//!
//! Record stride is 20 bytes. Tables run from a few dozen records up to ~760.
//! Some files place the table at the start of the file; others at the end,
//! with a separate (vertex/coordinate) section in the remaining space.
//!
//! This is empirically derived stage data — we have not yet identified the
//! consumer in the binary, so the field semantics are tentative.

use serde::Serialize;

/// The 12-byte signature that prefixes every record.
pub const RECORD_PREFIX: [u8; 12] = [
    0x00, 0xF0, 0x84, 0x7F, 0x01, 0xF0, 0x1F, 0x00, 0x00, 0xF1, 0x00, 0x00,
];

/// Total bytes per record (prefix + payload).
pub const RECORD_STRIDE: usize = 20;

/// Bytes of payload per record.
pub const RECORD_PAYLOAD: usize = 8;

/// Empirical minimum number of consecutive records to consider a buffer to
/// contain a stage-geometry table. Two consecutive 20-byte hits are enough
/// to rule out a coincidental u32 match.
pub const MIN_TABLE_RECORDS: usize = 4;

/// One detected table within a buffer.
#[derive(Debug, Clone, Serialize)]
pub struct GeomTable {
    /// Byte offset of the first record's prefix.
    pub start: usize,
    /// Number of consecutive 20-byte records.
    pub records: usize,
    /// Byte offset just past the last record (`start + records * 20`).
    pub end: usize,
}

/// Scan `buf` for runs of consecutive records sharing [`RECORD_PREFIX`] at
/// 20-byte stride. Returns one `GeomTable` per maximal run with at least
/// [`MIN_TABLE_RECORDS`] records.
pub fn scan(buf: &[u8]) -> Vec<GeomTable> {
    let mut out = Vec::new();
    if buf.len() < RECORD_STRIDE {
        return out;
    }

    let mut i = 0usize;
    while i + RECORD_STRIDE <= buf.len() {
        if buf[i..i + RECORD_PREFIX.len()] != RECORD_PREFIX {
            i += 1;
            continue;
        }

        // Found a candidate start. Walk forward 20 bytes at a time as long as
        // the prefix continues to match.
        let start = i;
        let mut records = 0usize;
        let mut j = i;
        while j + RECORD_STRIDE <= buf.len() && buf[j..j + RECORD_PREFIX.len()] == RECORD_PREFIX {
            records += 1;
            j += RECORD_STRIDE;
        }

        if records >= MIN_TABLE_RECORDS {
            out.push(GeomTable {
                start,
                records,
                end: start + records * RECORD_STRIDE,
            });
        }
        i = j.max(i + 1);
    }

    out
}

/// Iterator-friendly view of one record. The 8-byte payload is exposed both
/// raw and decoded as 4 little-endian u16s — the most plausible interpretation
/// per cross-file analysis.
#[derive(Debug, Clone, Copy)]
pub struct Record<'a> {
    pub bytes: &'a [u8; RECORD_STRIDE],
}

impl<'a> Record<'a> {
    pub fn payload(&self) -> &'a [u8] {
        &self.bytes[12..]
    }

    /// Payload as 4 × u16 little-endian.
    pub fn payload_u16s(&self) -> [u16; 4] {
        [
            u16::from_le_bytes([self.bytes[12], self.bytes[13]]),
            u16::from_le_bytes([self.bytes[14], self.bytes[15]]),
            u16::from_le_bytes([self.bytes[16], self.bytes[17]]),
            u16::from_le_bytes([self.bytes[18], self.bytes[19]]),
        ]
    }
}

/// Iterate the records in a single [`GeomTable`] within `buf`.
pub fn records<'a>(buf: &'a [u8], table: &GeomTable) -> impl Iterator<Item = Record<'a>> {
    (0..table.records).map(move |i| {
        let off = table.start + i * RECORD_STRIDE;
        let arr: &'a [u8; RECORD_STRIDE] = buf[off..off + RECORD_STRIDE]
            .try_into()
            .expect("slice always 20 bytes");
        Record { bytes: arr }
    })
}

/// Vertex pool stride. The pool is contiguous bytes interpretable as
/// `[i16 x, i16 _, i16 z, i16 y]` per vertex (slot 1 is usually 0; treat as
/// padding for visualization purposes).
pub const VERTEX_STRIDE: usize = 8;

/// One vertex from the pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vertex {
    pub x: i16,
    pub y: i16,
    pub z: i16,
}

impl Vertex {
    /// Parse one 8-byte vertex slot. Layout (slots 0..3 as i16 LE):
    /// `[x, _, z, y]`.
    pub fn from_slot(bytes: &[u8; VERTEX_STRIDE]) -> Self {
        let x = i16::from_le_bytes([bytes[0], bytes[1]]);
        let z = i16::from_le_bytes([bytes[4], bytes[5]]);
        let y = i16::from_le_bytes([bytes[6], bytes[7]]);
        Self { x, y, z }
    }
}

/// Top-level parsed stage: the records table plus a contiguous vertex pool
/// either before or after the table. See `project_stage_geom_layout.md`
/// for the on-disc layout discoveries.
#[derive(Debug, Clone)]
pub struct Stage {
    /// All record tables found in the file. The largest is the one whose
    /// indices [`Self::vertex_pool`] is sized to satisfy.
    pub tables: Vec<GeomTable>,
    /// File-byte offset of the vertex pool's first byte.
    pub pool_offset: usize,
    /// Length of the vertex pool in bytes (always a multiple of [`VERTEX_STRIDE`]).
    pub pool_bytes: usize,
}

impl Stage {
    /// Vertex count derivable from [`Self::pool_bytes`].
    pub fn vertex_count(&self) -> usize {
        self.pool_bytes / VERTEX_STRIDE
    }

    /// Read vertex `idx` from `buf`. Returns `None` if `idx` is out of range.
    pub fn vertex(&self, buf: &[u8], idx: usize) -> Option<Vertex> {
        let off = self.pool_offset + idx * VERTEX_STRIDE;
        let end = off + VERTEX_STRIDE;
        if end > self.pool_offset + self.pool_bytes {
            return None;
        }
        let slot: &[u8; VERTEX_STRIDE] = buf[off..end].try_into().ok()?;
        Some(Vertex::from_slot(slot))
    }

    /// Resolve a record's 4 byte-offset indices to vertex pool indices.
    /// Returns `None` if any byte offset is misaligned or out of range.
    pub fn quad_vertex_indices(&self, record: &Record<'_>) -> Option<[usize; 4]> {
        let pl = record.payload_u16s();
        let mut out = [0usize; 4];
        let pool_max = self.pool_bytes;
        for (i, p) in pl.iter().enumerate() {
            let byte_off = *p as usize;
            if !byte_off.is_multiple_of(VERTEX_STRIDE) || byte_off + VERTEX_STRIDE > pool_max {
                return None;
            }
            out[i] = byte_off / VERTEX_STRIDE;
        }
        Some(out)
    }
}

/// Detect the records table and pick the side (before or after the largest
/// table) that holds the vertex pool. Returns `None` if no table was found.
///
/// Heuristic: prefer whichever side has more bytes — both vertex-pool layouts
/// are observed in the wild (pool-first in `init_data`, pool-in-trailer in
/// `town01`). Both sides are valid u16-indexable byte regions; the empty
/// side simply has no candidate vertices.
pub fn parse(buf: &[u8]) -> Option<Stage> {
    let tables = scan(buf);
    let largest = tables.iter().max_by_key(|t| t.records)?.clone();
    let before_bytes = largest.start;
    let after_bytes = buf.len().saturating_sub(largest.end);
    let (pool_offset, pool_bytes) = if after_bytes > before_bytes {
        (largest.end, after_bytes)
    } else {
        (0, before_bytes)
    };
    // Truncate to a multiple of VERTEX_STRIDE (pools observed in practice
    // include 24 leading zero-pad bytes — those are still 3 valid vertex
    // slots, so we don't strip them).
    let pool_bytes = (pool_bytes / VERTEX_STRIDE) * VERTEX_STRIDE;
    Some(Stage {
        tables,
        pool_offset,
        pool_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthesize(n_records: usize, leading_pad: usize, trailing_pad: usize) -> Vec<u8> {
        let mut v = vec![0xAAu8; leading_pad];
        for i in 0..n_records {
            v.extend_from_slice(&RECORD_PREFIX);
            // Distinct payload per record so we can verify iteration.
            v.extend_from_slice(&(i as u16).to_le_bytes());
            v.extend_from_slice(&(i as u16 + 1).to_le_bytes());
            v.extend_from_slice(&(i as u16 + 2).to_le_bytes());
            v.extend_from_slice(&(i as u16 + 3).to_le_bytes());
        }
        v.extend(std::iter::repeat_n(0xBBu8, trailing_pad));
        v
    }

    #[test]
    fn detects_run_at_offset_zero() {
        let buf = synthesize(10, 0, 0);
        let tables = scan(&buf);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].start, 0);
        assert_eq!(tables[0].records, 10);
        assert_eq!(tables[0].end, 200);
    }

    #[test]
    fn detects_run_with_padding() {
        let buf = synthesize(50, 32, 64);
        let tables = scan(&buf);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].start, 32);
        assert_eq!(tables[0].records, 50);
    }

    #[test]
    fn ignores_runs_below_threshold() {
        // Two records — under MIN_TABLE_RECORDS (4).
        let buf = synthesize(2, 0, 0);
        assert!(scan(&buf).is_empty());
    }

    #[test]
    fn iterates_payloads_in_order() {
        let buf = synthesize(5, 16, 0);
        let tables = scan(&buf);
        let payloads: Vec<[u16; 4]> = records(&buf, &tables[0])
            .map(|r| r.payload_u16s())
            .collect();
        assert_eq!(payloads.len(), 5);
        assert_eq!(payloads[0], [0, 1, 2, 3]);
        assert_eq!(payloads[4], [4, 5, 6, 7]);
    }

    #[test]
    fn returns_empty_for_short_buffer() {
        assert!(scan(&[]).is_empty());
        assert!(scan(&[0u8; 10]).is_empty());
    }

    #[test]
    fn coincidental_single_match_does_not_register() {
        // Just one record's worth of prefix in random data.
        let mut buf = vec![0xAAu8; 1024];
        buf[100..112].copy_from_slice(&RECORD_PREFIX);
        // Following bytes happen to be junk, not another prefix.
        let tables = scan(&buf);
        assert!(tables.is_empty());
    }

    #[test]
    fn vertex_from_slot_uses_slots_0_3_2_for_xyz() {
        // [x=100 i16, _=0, z=-50 i16, y=200 i16]
        let bytes: [u8; 8] = [100, 0, 0, 0, 0xCE, 0xFF, 200, 0];
        let v = Vertex::from_slot(&bytes);
        assert_eq!(v.x, 100);
        assert_eq!(v.y, 200);
        assert_eq!(v.z, -50);
    }

    #[test]
    fn parse_picks_after_pool_when_table_is_at_start() {
        // Table at file start, vertex pool in trailer.
        let mut buf = synthesize(4, 0, 0);
        // Append 32 bytes of "vertex pool".
        buf.extend_from_slice(&[0u8; 32]);
        let stage = parse(&buf).expect("must parse");
        assert_eq!(stage.tables.len(), 1);
        assert_eq!(stage.pool_offset, 80); // 4 records * 20 bytes
        assert_eq!(stage.pool_bytes, 32);
        assert_eq!(stage.vertex_count(), 4);
    }

    #[test]
    fn parse_picks_before_pool_when_table_is_at_end() {
        // 64 bytes of "vertex pool" then table.
        let mut buf = vec![0u8; 64];
        buf.extend_from_slice(&synthesize(4, 0, 0));
        let stage = parse(&buf).expect("must parse");
        assert_eq!(stage.pool_offset, 0);
        assert_eq!(stage.pool_bytes, 64);
        assert_eq!(stage.vertex_count(), 8);
    }

    #[test]
    fn quad_vertex_indices_resolves_byte_offsets_to_indices() {
        // Build a buffer with table starting at byte 80 (4 dummy verts).
        let mut buf = vec![0u8; 32]; // 4 vertex slots
        // Table records — payload contains byte offsets 0, 16, 8, 0.
        for _ in 0..4 {
            buf.extend_from_slice(&RECORD_PREFIX);
            buf.extend_from_slice(&0u16.to_le_bytes());
            buf.extend_from_slice(&16u16.to_le_bytes());
            buf.extend_from_slice(&8u16.to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
        }
        let stage = parse(&buf).expect("parse");
        let table = &stage.tables[0];
        let rec = records(&buf, table).next().unwrap();
        let idx = stage.quad_vertex_indices(&rec).expect("resolve");
        assert_eq!(idx, [0, 2, 1, 0]);
    }
}
