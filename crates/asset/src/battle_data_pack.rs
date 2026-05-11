//! `battle_data` pack format - the multi-megabyte container used by
//! PROT entries in the `battle_data` CDNAME block (0865-0868).
//!
//! ### Layout (empirically verified against retail PROT 0865)
//!
//! ```text
//! +0x0000   u32 chunk0_header        ; (type=0x00 << 24) | first_chunk_size
//! +0x0004   ...chunk0 payload...     ; opaque streaming data
//! +0x0004 + first_chunk_size         ; chunk-stream terminator (low24=0)
//!
//! +chunk0_size + 4   u32 record_count   ; e.g. 0x57 = 87 slots
//! +chunk0_size + 8   u32 reserved       ; always 0
//! +chunk0_size + 12  Record[record_count]
//!
//! data_base = next 0x800-aligned offset after the record table
//!
//! Record (12 bytes):
//!   u32 on_disc_size      ; compressed size in bytes (incl. u32 dec_size prefix)
//!   u32 id                ; slot id (0..0x7F observed); 0 marks empty/filler
//!   u32 data_offset       ; offset from data_base
//!
//! Compressed entry (at data_base + record.data_offset):
//!   u32 decompressed_size
//!   LZS-compressed stream (size = on_disc_size - 4)
//!
//! Decompressed entry payload:
//!   +0x00  u32 magic_or_count  ; 0x14 (=20) in 0865; meaning still TBD
//!   +0x04  u32 sub_obj0_end    ; nested-section end offset (often 0)
//!   +0x08  u32 sub_obj1_end    ; nested-section end offset (often 0)
//!   +0x0C  u32 tmd_body_end    ; offset where the Legaia TMD body ends
//!   +0x10..0x20                ; per-texture info (layout TBD)
//!   +0x20  Legaia TMD          ; magic 0x80000002
//!   +tmd_body_end              ; texture/CLUT pool (layout partially TBD)
//! ```
//!
//! The container holds packed character TMDs with their textures. The TMDs
//! and post-TMD texture pool are co-located - the retail engine sources its
//! field/town NPC textures from this pack via the player-loader chain
//! (`FUN_8001E890` → LZS decode → register TMDs via `FUN_80026B4C`).
//!
//! ### Why this matters
//!
//! Town01's four NPC TMDs reference CLUT row y=479 slots x=128..240 (CBA
//! `0x77C8..0x77CF`). Those palettes live inside the post-TMD pool of one
//! or more `battle_data` records. Without descending into this pack, the
//! raw TIM scanner finds 0 TIMs in 0865 (the data is wrapped in this
//! custom format) and the targeted-upload path leaves those rows
//! unsupplied, dropping ~388 prims as MissingClut.

use anyhow::{Result, bail};
use serde::Serialize;

/// Maximum plausible record count we'll accept from the header.
const MAX_RECORDS: u32 = 256;

/// Alignment for the LZS-data section base (sector boundary).
const DATA_BASE_ALIGN: usize = 0x800;

/// One record in the trailer table.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Record {
    /// Index in the record table (0..count).
    pub index: usize,
    /// Compressed size on disc (bytes), including the u32 dec_size prefix.
    pub on_disc_size: u32,
    /// Slot id (0..0x7F observed). `0` marks an empty/filler slot.
    pub id: u32,
    /// Byte offset from `data_base` where the compressed entry lives.
    pub data_offset: u32,
}

impl Record {
    /// Absolute file offset of the compressed entry.
    pub fn file_offset(&self, data_base: usize) -> usize {
        data_base + self.data_offset as usize
    }
}

/// Parsed pack header (record table + data base).
#[derive(Debug, Clone, Serialize)]
pub struct BattleDataPack {
    /// Byte offset where the trailer record table starts.
    pub table_offset: usize,
    /// Total record count declared in the header.
    pub record_count: u32,
    /// Records that have non-zero size. Records past the first zero-size
    /// entry are treated as table padding and excluded.
    pub records: Vec<Record>,
    /// Byte offset where the compressed-data section begins.
    pub data_base: usize,
}

/// One decompressed entry.
#[derive(Debug, Clone)]
pub struct DecodedEntry {
    /// Record this entry was decoded from.
    pub record: Record,
    /// Decompressed bytes.
    pub bytes: Vec<u8>,
    /// Byte range of the embedded Legaia TMD (if found).
    pub tmd_range: Option<std::ops::Range<usize>>,
}

/// Cheap presence check: does `buf` look like a battle_data pack?
pub fn is_battle_data_pack(buf: &[u8]) -> bool {
    detect_header(buf).is_some()
}

/// Locate the trailer record table without parsing every record. The
/// streaming preamble runs from offset 0 to `chunk0_size + 4`, terminator
/// included; the count u32 lives right after the terminator, at
/// `chunk0_size + 4` (the terminator itself sits at `chunk0_size + 0`).
fn detect_header(buf: &[u8]) -> Option<(usize, u32)> {
    if buf.len() < 32 {
        return None;
    }
    let chunk0_header = read_u32_le(buf, 0)?;
    let type_byte = (chunk0_header >> 24) & 0xFF;
    let chunk0_size = (chunk0_header & 0x00FF_FFFF) as usize;
    // First chunk must be a TIM-typed dispatcher chunk by the streaming
    // convention; battle_data's streaming preamble uses type=0.
    if type_byte != 0 {
        return None;
    }
    // The record table sits right after the chunk payload + terminator.
    // chunk_payload spans [4, 4 + chunk0_size); terminator at +chunk0_size,
    // and the count u32 starts at +chunk0_size + 4. (For 0865 this is
    // exactly 0x6C68 + 4 = 0x6C6C ... wait, the count is at 0x6C68 in 0865
    // because the streaming "size = chunk0_size = 0x6C68" includes the
    // chunk's own header byte. Empirically the count lives at chunk0_size
    // exactly, not chunk0_size + 4.)
    //
    // Read both candidate positions and prefer the one with a sane count.
    for cand in [chunk0_size, chunk0_size + 4] {
        if cand + 8 > buf.len() {
            continue;
        }
        let count = read_u32_le(buf, cand)?;
        let reserved = read_u32_le(buf, cand + 4)?;
        if reserved != 0 {
            continue;
        }
        if count == 0 || count > MAX_RECORDS {
            continue;
        }
        // Sanity check: first record's offset should be small and size plausible.
        let table = cand + 8;
        if table + 12 > buf.len() {
            continue;
        }
        let sz0 = read_u32_le(buf, table)?;
        let id0 = read_u32_le(buf, table + 4)?;
        let off0 = read_u32_le(buf, table + 8)?;
        if sz0 == 0 || sz0 > 0x40_0000 {
            continue;
        }
        if id0 > 0xFF {
            continue;
        }
        if off0 == 0 || off0 > 0x100_0000 {
            continue;
        }
        return Some((cand, count));
    }
    None
}

/// Parse the trailer record table. Returns `None` when the buffer doesn't
/// match the battle-data pack shape.
pub fn detect(buf: &[u8]) -> Option<BattleDataPack> {
    let (count_off, count) = detect_header(buf)?;
    let table_offset = count_off + 8;
    let mut records = Vec::with_capacity(count as usize);
    let mut max_extent = table_offset + (count as usize) * 12;
    for i in 0..count as usize {
        let p = table_offset + i * 12;
        if p + 12 > buf.len() {
            break;
        }
        let on_disc_size = read_u32_le(buf, p)?;
        let id = read_u32_le(buf, p + 4)?;
        let data_offset = read_u32_le(buf, p + 8)?;
        if on_disc_size == 0 {
            // Table tail is zero-padded; stop here.
            break;
        }
        records.push(Record {
            index: i,
            on_disc_size,
            id,
            data_offset,
        });
        // Track the largest table coverage so we can align data_base above it.
        max_extent = max_extent.max(p + 12);
    }
    if records.is_empty() {
        return None;
    }
    // Align data_base up to the next DATA_BASE_ALIGN boundary past the table.
    let want_base = max_extent.div_ceil(DATA_BASE_ALIGN) * DATA_BASE_ALIGN;
    // Self-correct data_base: pick the first sector boundary at or past
    // `want_base` where every record's `dec_size` u32 prefix reads as a
    // plausible decompressed size (1 .. 4 MiB). Slots with offset=0 are
    // sentinel/filler entries that point back into the table region and
    // are excluded from this check.
    let mut data_base = want_base;
    let mut chose = false;
    let probe_limit = (want_base + 0x4_0000).min(buf.len().saturating_sub(8));
    while data_base <= probe_limit {
        let mut all_ok = true;
        for r in &records {
            if r.data_offset == 0 {
                // Sentinel/filler slot - retail rec 42 has offset 0 and
                // points back into the table region. Skip the dec_size
                // sanity check; decode_record will reject it later if
                // the caller tries to decode this slot.
                continue;
            }
            let start = data_base + r.data_offset as usize;
            if start + 4 > buf.len() {
                all_ok = false;
                break;
            }
            let Some(dec_size) = read_u32_le(buf, start) else {
                all_ok = false;
                break;
            };
            if dec_size == 0 || dec_size > 0x40_0000 {
                all_ok = false;
                break;
            }
        }
        if all_ok {
            chose = true;
            break;
        }
        data_base += DATA_BASE_ALIGN;
    }
    if !chose {
        return None;
    }
    Some(BattleDataPack {
        table_offset,
        record_count: count,
        records,
        data_base,
    })
}

/// Parse - errors instead of returning `None`.
pub fn parse(buf: &[u8]) -> Result<BattleDataPack> {
    detect(buf).ok_or_else(|| anyhow::anyhow!("not a battle_data pack"))
}

/// Decompress one record. Returns the decoded bytes plus the Legaia-TMD
/// byte range inside them (when locatable).
pub fn decode_record(buf: &[u8], pack: &BattleDataPack, idx: usize) -> Result<DecodedEntry> {
    let record = *pack
        .records
        .get(idx)
        .ok_or_else(|| anyhow::anyhow!("record index {} out of bounds", idx))?;
    let file_off = record.file_offset(pack.data_base);
    if file_off + 4 > buf.len() {
        bail!("record {} dec_size prefix past buffer end", idx);
    }
    let dec_size =
        read_u32_le(buf, file_off).ok_or_else(|| anyhow::anyhow!("dec_size read failed"))? as usize;
    if dec_size == 0 || dec_size > 0x40_0000 {
        bail!("record {} has implausible dec_size 0x{:x}", idx, dec_size);
    }
    // `on_disc_size` declares the slot's *allocation* footprint, not the
    // LZS stream length. The retail decoder reads tokens until enough
    // output bytes (`dec_size`) are produced; records often run their
    // LZS source past `on_disc_size` into the next slot's region.
    // Hand the decompressor everything from the dec_size prefix to the
    // end of file - it stops based on output count, not input count.
    let lzs_input = &buf[file_off + 4..];
    let bytes = legaia_lzs::decompress(lzs_input, dec_size)?;
    let tmd_range = locate_embedded_tmd(&bytes);
    Ok(DecodedEntry {
        record,
        bytes,
        tmd_range,
    })
}

/// Decompress every record. Records whose decode fails are skipped with
/// the error attached to the result via `Result<...>` so a caller can
/// surface them in a CLI listing.
pub fn decode_all(buf: &[u8], pack: &BattleDataPack) -> Vec<Result<DecodedEntry>> {
    (0..pack.records.len())
        .map(|i| decode_record(buf, pack, i))
        .collect()
}

/// Find the Legaia TMD inside a decoded entry.
///
/// Empirically two record-shape variants appear in retail 0865:
///   - Simple: TMD at offset 0x20 (after a 32-byte header whose u32[3]
///     holds the TMD body end).
///   - Nested: u32[1] / u32[2] hold non-zero sub-object end offsets and
///     the TMD shifts later in the buffer.
///
/// Try the canonical 0x20 position first; if that doesn't validate,
/// fall back to scanning every word-aligned offset for the magic and
/// keep the first one that passes structural checks.
fn locate_embedded_tmd(decoded: &[u8]) -> Option<std::ops::Range<usize>> {
    if decoded.len() < 0x20 + 12 {
        return None;
    }
    // Fast path: canonical 0x20 offset with u32[3] = body end.
    if let Some(rng) = try_tmd_at(decoded, 0x20, Some(12)) {
        return Some(rng);
    }
    // Fallback: word-aligned magic scan. Body end is unknown - rely on
    // legaia_tmd::parse to validate and report extent later.
    let mut off = 4;
    while off + 12 <= decoded.len() {
        if let Some(rng) = try_tmd_at(decoded, off, None) {
            return Some(rng);
        }
        off += 4;
    }
    None
}

fn try_tmd_at(
    decoded: &[u8],
    tmd_offset: usize,
    header_body_end_offset: Option<usize>,
) -> Option<std::ops::Range<usize>> {
    const TMD_MAGIC: u32 = 0x8000_0002;
    let magic = read_u32_le(decoded, tmd_offset)?;
    if magic != TMD_MAGIC {
        return None;
    }
    let flags = read_u32_le(decoded, tmd_offset + 4)?;
    if flags != 0 {
        return None;
    }
    let nobj = read_u32_le(decoded, tmd_offset + 8)?;
    if nobj == 0 || nobj > 64 {
        return None;
    }
    if let Some(off) = header_body_end_offset {
        let tmd_end = read_u32_le(decoded, off)? as usize;
        if tmd_end > tmd_offset && tmd_end <= decoded.len() {
            return Some(tmd_offset..tmd_end);
        }
    }
    // Without a header-supplied end, conservatively report the TMD as
    // running from `tmd_offset` to end-of-buffer. Callers needing the
    // exact extent should hand the range to `legaia_tmd::parse`.
    Some(tmd_offset..decoded.len())
}

fn read_u32_le(buf: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(buf.get(off..off + 4)?.try_into().ok()?))
}

/// Probe the post-TMD region of a decoded entry for a CLUT-shaped run of
/// halfwords (16 valid RGB1555 colors with the high-transparency bit
/// clear in at least 12 of them). Returns the byte range of the first
/// such run, or `None` if no run matches.
///
/// The retail post-TMD layout interleaves CLUTs and 4bpp texture pixel
/// data without standard TIM image-block headers; this probe is a cheap
/// "is there a palette here" heuristic that engine integrations can use
/// to surface raw bytes for upload to a VRAM row. Not a replacement for
/// a full layout reverse - the post-TMD descriptor table at u32[3..0x20]
/// of the entry header points at specific palette positions, but that
/// table's semantics aren't yet pinned.
pub fn probe_first_clut_run(decoded: &[u8]) -> Option<std::ops::Range<usize>> {
    let tmd_range = locate_embedded_tmd(decoded)?;
    let mut off = tmd_range.end;
    // Align to 2 (halfword) - retail post-TMD regions are u16-aligned.
    if !off.is_multiple_of(2) {
        off += 1;
    }
    while off + 32 <= decoded.len() {
        if looks_clut_like(&decoded[off..off + 32]) {
            return Some(off..off + 32);
        }
        off += 2;
    }
    None
}

fn looks_clut_like(bytes: &[u8]) -> bool {
    debug_assert!(bytes.len() == 32);
    let mut nonzero = 0;
    let mut high_clear = 0;
    let mut distinct = std::collections::HashSet::new();
    for i in 0..16 {
        let h = u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]);
        if h != 0 {
            nonzero += 1;
        }
        if h & 0x8000 == 0 {
            high_clear += 1;
        }
        distinct.insert(h);
    }
    nonzero >= 12 && high_clear >= 12 && distinct.len() >= 8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic pack with one record carrying a recognizable
    /// payload through the LZS round-trip. Mirrors the retail layout:
    /// the count u32 lives at the last 4 bytes of the chunk0 payload
    /// (i.e. at offset `chunk0_size`), with the streaming terminator
    /// immediately after.
    fn synth(payload: &[u8]) -> Vec<u8> {
        // Encode the payload as a trivial LZS stream of literals.
        let mut lzs = Vec::new();
        let mut i = 0;
        while i < payload.len() {
            let group = (payload.len() - i).min(8);
            let mut control: u8 = 0;
            for b in 0..group {
                control |= 1 << b;
            }
            lzs.push(control);
            lzs.extend_from_slice(&payload[i..i + group]);
            i += group;
        }
        // Pad with all-literal groups so the decoder always has source.
        for _ in 0..16 {
            lzs.push(0xFF);
            lzs.extend_from_slice(&[0u8; 8]);
        }

        let dec_size = payload.len() as u32;
        let on_disc_size = 4 + lzs.len() as u32;

        let mut buf = Vec::new();
        // Streaming preamble: one type-0 chunk whose payload ends with
        // the (count, reserved) header pair. The detector reads the
        // count at `chunk0_size` and the records at `chunk0_size + 8`.
        // Pick chunk0_payload_size such that the last u32 of payload is
        // the count u32.
        let count_u32_position: usize = 0x40; // arbitrary - aligned multiple of 4
        let chunk0_size: u32 = count_u32_position as u32;
        let chunk_header = chunk0_size; // type=0, size=chunk0_size
        buf.extend_from_slice(&chunk_header.to_le_bytes());
        // Chunk payload: zero-fill up to count_u32_position, then count.
        while buf.len() < count_u32_position {
            buf.push(0);
        }
        // count u32 lives at offset `chunk0_size` = count_u32_position.
        buf.extend_from_slice(&1u32.to_le_bytes()); // count = 1
        // reserved u32 at chunk0_size + 4
        buf.extend_from_slice(&0u32.to_le_bytes());
        // Record table starts at chunk0_size + 8
        // Empirically retail records always start at a non-zero offset
        // (the retail pack leaves the first 0x1800 bytes after data_base
        // unused). Mirror that here so the detector's sanity bounds pass.
        let record_data_offset: u32 = 0x100;
        buf.extend_from_slice(&on_disc_size.to_le_bytes()); // record.on_disc_size
        buf.extend_from_slice(&0x42u32.to_le_bytes()); // record.id
        buf.extend_from_slice(&record_data_offset.to_le_bytes());
        // Pad to next 0x800 boundary - data_base.
        while !buf.len().is_multiple_of(0x800) {
            buf.push(0);
        }
        let data_base = buf.len();
        // Pad to record's data_offset within the data section.
        buf.resize(data_base + record_data_offset as usize, 0);
        // Record entry: u32 dec_size + LZS bytes
        buf.extend_from_slice(&dec_size.to_le_bytes());
        buf.extend_from_slice(&lzs);
        let want_end = data_base + record_data_offset as usize + on_disc_size as usize;
        buf.resize(want_end, 0);
        buf
    }

    #[test]
    fn detects_minimal_synthetic() {
        // Payload: 32-byte header (last u32[3] = 0x40 tmd_body_end) + TMD-shaped magic
        // + small TMD-like body so the locator validates.
        let mut payload = vec![0u8; 0x80];
        // header.u32[3] = tmd body end = 0x60
        payload[12..16].copy_from_slice(&0x60u32.to_le_bytes());
        // TMD at +0x20
        payload[0x20..0x24].copy_from_slice(&0x80000002u32.to_le_bytes());
        payload[0x24..0x28].copy_from_slice(&0u32.to_le_bytes()); // flags
        payload[0x28..0x2C].copy_from_slice(&1u32.to_le_bytes()); // nobj=1
        let buf = synth(&payload);
        let pack = detect(&buf).expect("should detect");
        assert_eq!(pack.records.len(), 1);
        assert_eq!(pack.records[0].id, 0x42);
        let entry = decode_record(&buf, &pack, 0).expect("decode");
        assert_eq!(entry.bytes.len(), payload.len());
        assert_eq!(entry.tmd_range, Some(0x20..0x60));
    }

    #[test]
    fn rejects_random_bytes() {
        let buf = vec![0xAAu8; 0x4000];
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_short_buffers() {
        for n in [0usize, 1, 16, 31, 100] {
            let buf = vec![0u8; n];
            assert!(detect(&buf).is_none());
        }
    }

    #[test]
    fn rejects_nonzero_chunk0_type() {
        let mut buf = synth(&[0u8; 0x80]);
        // Bump the chunk0 type byte to non-zero.
        let mut h = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        h |= 0x05_00_00_00;
        buf[0..4].copy_from_slice(&h.to_le_bytes());
        assert!(detect(&buf).is_none());
    }
}
