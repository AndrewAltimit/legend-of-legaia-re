//! "VAB-prefixed scene-stream" detector - a streaming-format variant
//! whose leading chunk0 carries a Sony VAB sound bank rather than a TMD.
//!
//! ### Layout (empirically verified across 216 PROT entries, 2026-05-04)
//!
//! ```text
//! +0x00          u32 chunk0_header   ; (type=0x00 << 24) | size  (LE)
//! +0x04          VABp magic          ; 0x56414270 ('VABp')
//! +0x08          u32 version         ; 7 in retail
//! +0x0C..        VAB header tail + programs + tones + VAG offset table
//! +0x04 + size   streaming chunks    ; standard FUN_8002541c-style chunks
//!                                    ; (often empty / file ends here)
//! ```
//!
//! Same outer wrapper as [`crate::scene_tmd_stream`]: a 4-byte streaming chunk
//! header with type = 0x00 (the TIM dispatcher slot - repurposed by a
//! specialized loader). The bytes from offset 4 onward are a Sony VAB sound
//! bank (PsyQ format, `VABp` magic).
//!
//! Empirically the on-disc head pattern is `20 XX 00 00 70 42 41 56 ...`:
//! - `20 XX 00 00` = LE u32 `0x0000_XX20` - chunk0 header with type=0x00 and
//!   `size` low byte = 0x20 (sector-aligned-ish, varies in the 0x0C20..0x2E20
//!   band).
//! - `70 42 41 56` = LE u32 `0x56414270` = the four ASCII bytes `'p' 'B' 'A'
//!   'V'` - the VAB header magic.
//!
//! Coverage impact: this shape covers the bulk of the `vab_01` cluster
//! (CDNAME indices 1072-1194) plus VAB-bearing entries scattered through
//! `music_01`, `chitei2`, `ropeway`, `town*`, and others. Roughly **216
//! entries** in the post-TOC-fix corpus are this format - by far the largest
//! single bucket reduction since `scene_tmd_stream`.
//!
//! See `docs/formats/scene-bundles.md` for the full byte-level spec.

use serde::Serialize;

/// VAB header magic - 'p' 'B' 'A' 'V' read as a little-endian u32.
const VAB_MAGIC: u32 = 0x5641_4270;

/// VAB header is 32 bytes (Sony PsyQ docs). The detector requires this much
/// post-magic to validate the version + program count cleanly.
const VAB_HEADER_SIZE: usize = 0x20;

/// Highest plausible VAB version we'll accept. Retail Legaia uses 7; we cap
/// at 10 to leave room for future hand-validated finds.
const MAX_VAB_VERSION: u32 = 10;

/// Maximum total streaming chunk count we'll walk in the trailing tail before
/// giving up. Real hits typically have 0-4 trailing chunks.
const MAX_TAIL_CHUNKS: usize = 64;

/// Maximum bytes consumed by the optional streaming-tail walk. A few MB is
/// enough to confirm structural shape without runaway scans.
const MAX_TAIL_WALK: usize = 4 * 1024 * 1024;

/// Detection result.
#[derive(Debug, Clone, Serialize)]
pub struct SceneVabStream {
    /// Size declared by the chunk0 header (low 24 bits of the LE u32 at
    /// offset 0). The VAB body occupies `[4 .. 4 + chunk0_size]`.
    pub chunk0_size: usize,
    /// VAB version word at offset 8.
    pub vab_version: u32,
    /// Number of programs in use (`ps` field of the VAB header at offset 0x12).
    pub vab_ps: u16,
    /// Number of tones in use (`ts` field at offset 0x14).
    pub vab_ts: u16,
    /// Number of trailing streaming chunks walked from `4 + chunk0_size`.
    pub tail_chunks: usize,
    /// Whether the streaming tail terminated cleanly (header low-24 == 0).
    /// `true` also when the tail is empty (the file ends exactly at the
    /// VAB body, which is the common case).
    pub tail_terminated: bool,
}

impl SceneVabStream {
    /// Byte range of the leading VAB body inside the on-disc buffer. Hand
    /// `&buf[range]` to `legaia_vab::parse_header` etc.
    pub fn vab_range(&self) -> std::ops::Range<usize> {
        4..4 + self.chunk0_size
    }
}

/// Try to detect a VAB-prefixed scene stream. Returns `None` when the buffer
/// doesn't match the schema.
pub fn detect(buf: &[u8]) -> Option<SceneVabStream> {
    // Header + magic + at least one full VAB header word.
    if buf.len() < 4 + VAB_HEADER_SIZE {
        return None;
    }

    // (1) chunk0 header: type byte must be 0 (high byte of LE u32 == 0),
    //     size must be reasonable.
    let chunk0_header = legaia_bytes::u32_le(buf, 0)?;
    let type_byte = (chunk0_header >> 24) & 0xFF;
    let size = (chunk0_header & 0x00FF_FFFF) as usize;
    if type_byte != 0 {
        return None;
    }
    // Size must cover at least the VAB header and fit in the buffer (with
    // tolerance - some on-disc copies have padding past the declared size).
    if !(VAB_HEADER_SIZE..=buf.len() - 4).contains(&size) {
        return None;
    }

    // (2) VAB magic at offset 4.
    let magic = legaia_bytes::u32_le(buf, 4)?;
    if magic != VAB_MAGIC {
        return None;
    }

    // (3) VAB version sanity-check.
    let version = legaia_bytes::u32_le(buf, 8)?;
    if version == 0 || version > MAX_VAB_VERSION {
        return None;
    }

    // (4) Programs / tones counts at +0x12 / +0x14 (post-fsize at +0x0C, ps
    //     at +0x12). PS <= 128, TS <= 128. (PsyQ docs cap both at 128.)
    let ps = legaia_bytes::u16_le(buf, 4 + 0x12)?;
    let ts = legaia_bytes::u16_le(buf, 4 + 0x14)?;
    if ps > 128 || ts > 128 {
        return None;
    }

    // (5) Optionally walk the streaming tail. We only require it to be
    //     well-formed *or* cleanly terminated by EOF (the common case is
    //     "file ends exactly at chunk0 size").
    let (tail_chunks, tail_terminated) = walk_tail(buf, 4 + size);

    Some(SceneVabStream {
        chunk0_size: size,
        vab_version: version,
        vab_ps: ps,
        vab_ts: ts,
        tail_chunks,
        tail_terminated,
    })
}

/// Walk standard streaming chunks until terminator, EOF, or corruption.
/// Returns `(chunk_count, terminated_cleanly)`. EOF without a terminator
/// counts as terminated (most on-disc VAB streams have no trailing chunks).
fn walk_tail(buf: &[u8], mut offset: usize) -> (usize, bool) {
    let mut chunks = 0usize;
    let walk_limit = offset.saturating_add(MAX_TAIL_WALK).min(buf.len());

    while offset + 4 <= walk_limit {
        let Some(header) = legaia_bytes::u32_le(buf, offset) else {
            return (chunks, false);
        };
        let chunk_size = (header & 0x00FF_FFFF) as usize;
        if chunk_size == 0 {
            // Clean terminator (size zero).
            return (chunks, true);
        }
        chunks += 1;
        if chunks > MAX_TAIL_CHUNKS {
            return (chunks, false);
        }
        let next = offset + 4 + chunk_size;
        if next > walk_limit {
            return (chunks, false);
        }
        offset = next;
    }

    // Reached EOF or walk limit. Treat EOF-aligned as terminated.
    (chunks, offset == buf.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid scene_vab_stream buffer.
    fn synth(vab_size: usize, version: u32, ps: u16, ts: u16) -> Vec<u8> {
        let mut buf = Vec::with_capacity(4 + vab_size);
        // chunk0 header: type=0, size=vab_size.
        buf.extend_from_slice(&(vab_size as u32).to_le_bytes());
        // VAB magic + version.
        buf.extend_from_slice(&VAB_MAGIC.to_le_bytes());
        buf.extend_from_slice(&version.to_le_bytes());
        // Pad header to +0x12.
        while buf.len() < 4 + 0x12 {
            buf.push(0);
        }
        // ps + ts.
        buf.extend_from_slice(&ps.to_le_bytes());
        buf.extend_from_slice(&ts.to_le_bytes());
        // Pad to declared vab_size (so the body fits within the chunk).
        while buf.len() < 4 + vab_size {
            buf.push(0);
        }
        buf
    }

    #[test]
    fn detects_minimal_scene_vab_stream() {
        let buf = synth(0x40, 7, 4, 16);
        let s = detect(&buf).expect("should detect");
        assert_eq!(s.chunk0_size, 0x40);
        assert_eq!(s.vab_version, 7);
        assert_eq!(s.vab_ps, 4);
        assert_eq!(s.vab_ts, 16);
        assert_eq!(s.tail_chunks, 0);
        assert!(s.tail_terminated);
    }

    #[test]
    fn rejects_buffer_with_wrong_magic() {
        let mut buf = synth(0x40, 7, 4, 16);
        // Corrupt the VAB magic.
        buf[4] = 0;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_buffer_with_nonzero_type_byte() {
        let mut buf = synth(0x40, 7, 4, 16);
        // Set high byte of chunk0 header - type byte = 0x14 (TIM_LIST).
        buf[3] = 0x14;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_buffer_with_implausible_version() {
        let buf = synth(0x40, 99, 4, 16);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_buffer_with_oversized_program_count() {
        let buf = synth(0x40, 7, 200, 16);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_too_small_buffer() {
        // 4 + 0x20 = 36 bytes minimum.
        assert!(detect(&[0u8; 16]).is_none());
        assert!(detect(&[0u8; 35]).is_none());
    }

    #[test]
    fn rejects_chunk0_size_that_exceeds_buffer() {
        // chunk0 header says size 0x10000 but buffer is only 100 bytes.
        let mut buf = synth(0x40, 7, 4, 16);
        // Patch the chunk0_size to 0x10000.
        buf[0..4].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn accepts_real_world_head_pattern() {
        // 20 1A 00 00 70 42 41 56 07 00 00 00 ... = chunk0 size 0x1A20,
        // VAB magic, version 7. Used by 0267_teien.BIN and friends.
        let mut buf = vec![0x20, 0x1A, 0, 0, 0x70, 0x42, 0x41, 0x56, 7, 0, 0, 0];
        // Pad with zeros up to the declared chunk0 size + 4.
        buf.resize(4 + 0x1A20, 0);
        // Set sane ps / ts.
        buf[4 + 0x12] = 4;
        buf[4 + 0x13] = 0;
        buf[4 + 0x14] = 16;
        buf[4 + 0x15] = 0;
        let s = detect(&buf).expect("real-world pattern should detect");
        assert_eq!(s.chunk0_size, 0x1A20);
        assert_eq!(s.vab_version, 7);
    }

    #[test]
    fn walks_terminated_streaming_tail() {
        // Build: chunk0 (VAB), then a fake tail chunk of size 8, then a
        // zero-size terminator.
        let vab_size = 0x40;
        let mut buf = synth(vab_size, 7, 4, 16);
        // Tail chunk: type=0x14 (TIM_LIST) + size 8.
        let tail_header = (0x14u32 << 24) | 8;
        buf.extend_from_slice(&tail_header.to_le_bytes());
        buf.extend_from_slice(&[0u8; 8]);
        // Terminator.
        buf.extend_from_slice(&0u32.to_le_bytes());
        let s = detect(&buf).expect("should detect with tail");
        assert_eq!(s.chunk0_size, vab_size);
        assert_eq!(s.tail_chunks, 1);
        assert!(s.tail_terminated);
    }

    #[test]
    fn vab_range_is_offset_4_to_chunk0_end() {
        let buf = synth(0x40, 7, 4, 16);
        let s = detect(&buf).unwrap();
        assert_eq!(s.vab_range(), 4..4 + 0x40);
    }
}
