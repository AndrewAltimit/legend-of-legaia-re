//! "TMD-prefixed scene-stream" detector - a streaming-format variant
//! that opens with a bare Legaia TMD instead of a typed chunk header.
//!
//! ### Layout (empirically verified across 148 PROT entries, 2026-05)
//!
//! ```text
//! +0x00          u32 chunk0_header   ; (type=0x00 << 24) | size
//! +0x04          Legaia TMD          ; magic 0x80000002, fills `size` bytes
//! +0x04 + size   streaming chunks    ; standard FUN_8002541c-style chunks
//!                                    ; until terminator OR end-of-file
//! ```
//!
//! The chunk0 header looks like a standard streaming `(type << 24) | size`
//! with `type = 0x00` (= TIM dispatcher), but the payload is a Legaia TMD
//! (magic `0x80000002`). The runtime presumably uses a specialized loader
//! that knows the leading chunk is a TMD; the standard streaming walker
//! accepts the file structurally but flags `magic_ok = false`.
//!
//! After the leading TMD the layout matches standard DATA_FIELD streaming,
//! so each subsequent chunk goes through the asset-type dispatcher
//! (`FUN_8001f05c`) normally.
//!
//! This shape is dominant in scene-asset PROT entries (most `town*`, `dolk*`,
//! `rugi*`, and similar named blocks). Pre-TOC-fix the bare-TMD prefix made
//! many of these look like "low-entropy unknowns" because the inner streaming
//! header was 8824+ bytes deep - the standard streaming detector starts at
//! offset 0 and saw a non-streaming first chunk (`type_byte = 0x00` with
//! TMD-magic content).

use serde::Serialize;

use crate::AssetType;

/// Minimum sane object count in a Legaia TMD header. Defensive bound - real
/// scene TMDs have 1-8 objects (terrain mesh + a few props).
const MAX_TMD_OBJECTS: u32 = 64;

/// Maximum total streaming chunk count we'll walk before giving up. Real hits
/// have 1-6 chunks; anything past 16 is almost certainly a mis-detection.
const MAX_CHUNKS: usize = 64;

/// Maximum bytes consumed by the optional streaming-tail walk. We don't need
/// to walk forever - a few hundred KB is enough to confirm shape.
const MAX_TAIL_WALK: usize = 4 * 1024 * 1024;

/// Per-chunk record in the streaming tail.
#[derive(Debug, Clone, Serialize)]
pub struct TailChunk {
    /// Byte offset of the chunk header within the file.
    pub offset: usize,
    /// Asset type from the chunk's high-byte.
    pub asset_type: AssetType,
    /// Low-24-bit size from the chunk header.
    pub size: u32,
}

/// Detection result.
#[derive(Debug, Clone, Serialize)]
pub struct SceneTmdStream {
    /// Byte size of the leading TMD body (= `(chunk0_header & 0xFFFFFF)`).
    /// The TMD occupies `[4 .. 4 + tmd_size]`.
    pub tmd_size: usize,
    /// Object count from the leading TMD's header.
    pub tmd_nobj: u32,
    /// Streaming chunks walked from `4 + tmd_size` until terminator or break.
    pub tail_chunks: Vec<TailChunk>,
    /// Whether the streaming tail terminated cleanly (header low-24 == 0).
    pub tail_terminated: bool,
    /// Byte offset where the streaming tail walk stopped.
    pub tail_end: usize,
}

impl SceneTmdStream {
    /// Byte range of the leading TMD body inside the on-disc buffer.
    /// Hand `&buf[range]` to `legaia_tmd::parse`.
    pub fn tmd_range(&self) -> std::ops::Range<usize> {
        4..4 + self.tmd_size
    }
}

/// Try to detect a TMD-prefixed scene stream. Returns `None` when the buffer
/// doesn't match the schema; structural errors fail soft.
pub fn detect(buf: &[u8]) -> Option<SceneTmdStream> {
    if buf.len() < 32 {
        return None;
    }

    // (1) Bare TMD magic at offset 4.
    let tmd_magic = read_u32_le(buf, 4)?;
    if tmd_magic != 0x80000002 {
        return None;
    }

    // (2) TMD on-disc flags must be zero (post-fixup is 1, on-disc is 0).
    let tmd_flags = read_u32_le(buf, 8)?;
    if tmd_flags != 0 {
        return None;
    }

    // (3) Object count must be a small positive number.
    let nobj = read_u32_le(buf, 12)?;
    if nobj == 0 || nobj > MAX_TMD_OBJECTS {
        return None;
    }

    // (4) The chunk0 header packs `(type<<24) | size`, where the high byte
    //     is 0 (TIM dispatcher) and the low 24 bits give the TMD body size.
    //     Reject if the type byte isn't 0 - that would mean a different
    //     dispatcher fires on the leading chunk and this isn't the variant
    //     we're trying to detect.
    let chunk0_header = read_u32_le(buf, 0)?;
    if (chunk0_header >> 24) & 0xFF != 0 {
        return None;
    }
    let tmd_size = (chunk0_header & 0x00FF_FFFF) as usize;
    let min_tmd_size = 12 + (nobj as usize) * 28;
    if tmd_size < min_tmd_size {
        return None;
    }
    let tmd_end = 4usize.checked_add(tmd_size)?;
    if tmd_end > buf.len() {
        return None;
    }
    // Streaming chunks are 4-byte aligned; the TMD body must land on one.
    if !tmd_size.is_multiple_of(4) {
        return None;
    }

    // (5) Walk the streaming tail starting at `4 + tmd_size`. We accept the
    //     file even if the tail doesn't terminate cleanly - many entries
    //     are stored padded out to the next 0x800 sector boundary, which
    //     our walker may detect as garbage rather than a clean terminator.
    let mut tail_chunks = Vec::new();
    let mut cur = tmd_end;
    let mut terminated = false;
    let walk_cap = (cur + MAX_TAIL_WALK).min(buf.len());
    while cur + 4 <= walk_cap && tail_chunks.len() < MAX_CHUNKS {
        let header = match read_u32_le(buf, cur) {
            Some(v) => v,
            None => break,
        };
        if header & 0x00FF_FFFF == 0 {
            terminated = true;
            cur += 4;
            break;
        }
        let type_byte = ((header >> 24) & 0xFF) as u8;
        let asset_type = AssetType::from_byte(type_byte);
        if matches!(asset_type, AssetType::Unknown(_)) {
            // Tail is malformed (or truncated). Stop without recording the
            // bogus header - caller can still see how many good chunks parsed.
            break;
        }
        let size = header & 0x00FF_FFFF;
        tail_chunks.push(TailChunk {
            offset: cur,
            asset_type,
            size,
        });
        // Streaming chunks are 4-byte aligned by spec.
        cur = cur
            .checked_add(4 + ((size as usize + 3) & !3))
            .unwrap_or(buf.len());
    }

    // (6) Require at least one good streaming-tail chunk OR a clean terminator
    //     immediately at `tmd_end`. Otherwise we're matching arbitrary
    //     [u32 size][TMD] data with random bytes following it.
    if tail_chunks.is_empty() && !terminated {
        return None;
    }

    Some(SceneTmdStream {
        tmd_size,
        tmd_nobj: nobj,
        tail_chunks,
        tail_terminated: terminated,
        tail_end: cur,
    })
}

/// Cheap presence check - used by [`crate::categorize`] before doing the
/// full streaming-tail walk in callers that just need a yes/no.
pub fn is_scene_tmd_stream(buf: &[u8]) -> bool {
    detect(buf).is_some()
}

fn read_u32_le(buf: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(buf.get(off..off + 4)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthesize a TMD-prefixed scene stream:
    /// [u32 chunk0_header = (0<<24)|tmd_body_size]
    /// [TMD magic / flags / nobj]
    /// [object table]
    /// [body padding zeros]
    /// [streaming chunks]
    /// [terminator]
    fn synth(nobj: u32, body_bytes: usize, chunks: &[(u8, &[u8])]) -> Vec<u8> {
        // body_bytes must be 4-aligned for the chunk0 header to land cleanly.
        assert!(
            body_bytes.is_multiple_of(4),
            "test-only synth requires 4-aligned body"
        );
        let tmd_body_size = 12 + 28 * nobj as usize + body_bytes;
        let mut buf = Vec::with_capacity(4 + tmd_body_size + 1024);
        // chunk0 header: type=0, size = TMD body bytes
        let chunk0_header = tmd_body_size as u32 & 0x00FFFFFF;
        buf.extend_from_slice(&chunk0_header.to_le_bytes());
        buf.extend_from_slice(&0x80000002u32.to_le_bytes()); // magic
        buf.extend_from_slice(&0u32.to_le_bytes()); // flags
        buf.extend_from_slice(&nobj.to_le_bytes());
        for _ in 0..nobj {
            buf.extend_from_slice(&[0u8; 28]);
        }
        buf.extend(std::iter::repeat_n(0u8, body_bytes));
        // Streaming chunks
        for (type_byte, payload) in chunks {
            let header = ((*type_byte as u32) << 24) | (payload.len() as u32 & 0x00FFFFFF);
            buf.extend_from_slice(&header.to_le_bytes());
            buf.extend_from_slice(payload);
            // Pad to 4-byte boundary.
            while !buf.len().is_multiple_of(4) {
                buf.push(0);
            }
        }
        // Terminator (low 24 bits zero).
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf
    }

    #[test]
    fn detects_minimal_synthetic() {
        let buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
        let r = detect(&buf).expect("should detect");
        assert_eq!(r.tmd_nobj, 2);
        // TMD body = 12 (header) + 2*28 (object table) + 64 (padding)
        assert_eq!(r.tmd_size, 12 + 28 * 2 + 64);
        assert_eq!(r.tmd_range(), 4..4 + r.tmd_size);
        assert_eq!(r.tail_chunks.len(), 1);
        assert!(matches!(r.tail_chunks[0].asset_type, AssetType::Tim));
        assert!(r.tail_terminated);
    }

    #[test]
    fn rejects_wrong_magic() {
        let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
        buf[4..8].copy_from_slice(&0x80000041u32.to_le_bytes()); // PSX-standard TMD
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_zero_nobj() {
        let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
        buf[12..16].copy_from_slice(&0u32.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_silly_nobj() {
        let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
        buf[12..16].copy_from_slice(&0x10000u32.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_first_u32_oob() {
        let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
        // Set a TMD body size that exceeds the file.
        let oob = (buf.len() as u32) + 0x1000;
        buf[0..4].copy_from_slice(&oob.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_nonzero_chunk0_type() {
        // chunk0 header type byte must be 0 (TIM dispatcher) - anything else
        // is a different streaming variant we're not detecting here.
        let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
        let mut hdr = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        hdr |= 0x02_000000; // type = 2 (TMD)
        buf[0..4].copy_from_slice(&hdr.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_first_u32_too_small() {
        let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
        buf[0..4].copy_from_slice(&8u32.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_unaligned_first_u32() {
        let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
        // Pick an unaligned TMD body size (low 24 bits).
        let unaligned: u32 = 129;
        buf[0..4].copy_from_slice(&unaligned.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_no_streaming_tail() {
        // [u32 size][bare TMD] then random bytes that don't form a streaming chunk.
        let buf = synth(2, 64, &[]);
        // Replace terminator with garbage so neither chunk-walk nor terminator catches.
        let len = buf.len();
        let mut buf = buf;
        buf[len - 4..].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
        // Type byte 0xDE is unknown and chunk size huge → no good chunks, no terminator.
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn accepts_terminator_only_tail() {
        // Streaming tail consisting solely of a terminator.
        let buf = synth(1, 0, &[]);
        let r = detect(&buf).expect("should detect terminator-only tail");
        assert!(r.tail_terminated);
        assert!(r.tail_chunks.is_empty());
    }

    #[test]
    fn accepts_truncated_tail() {
        // Build a stream then truncate before the terminator.
        let buf = synth(2, 16, &[(0x01, &[0u8; 0x40]), (0x02, &[0u8; 0x40])]);
        // Drop the trailing terminator bytes.
        let truncated = &buf[..buf.len() - 4];
        let r = detect(truncated).expect("truncated tail should still parse");
        assert_eq!(r.tail_chunks.len(), 2);
        assert!(!r.tail_terminated);
    }
}
