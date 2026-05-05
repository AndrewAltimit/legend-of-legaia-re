//! "Truncated DATA_FIELD streaming" detector — sister of
//! [`crate::parse_streaming`] for the case where the leading chunks decode
//! cleanly but the **last** chunk's declared size walks past EOF (no
//! terminator chunk on disc).
//!
//! ### Layout
//!
//! ```text
//! +0                 [u32 header_0 = (type<<24) | size_0]   ; type known,
//!                                                            ; magic OK
//! +4                 size_0 bytes of chunk 0 payload
//! +4 + size_0        [u32 header_1]
//! +...               ...
//! +X                 [u32 header_N = (type<<24) | size_N]   ; declared size
//!                                                            ; would extend
//!                                                            ; past EOF
//! ```
//!
//! ### Why "truncated"
//!
//! The leading chunks parse exactly like a [`Class::DataFieldStreaming`]
//! container (full-fat [`StreamReport`] with `all_known_types = true`,
//! `all_magic_ok = true`), but instead of reaching a `size == 0` terminator
//! the parser walks off the buffer end on the final chunk's declared size.
//! Real PROT entries matching this layout (`0157_rikuroa`, `0228_station`,
//! `0373_taiku`) carry a per-scene secondary table in the trailing chunk
//! whose declared `size` exceeds the remaining file by 4–66 KB — the
//! runtime extends the chunk via streaming DMA continuation rather than
//! consuming a literal terminator on disc.
//!
//! Distinct from [`Class::DataFieldStreaming`], which requires a clean
//! terminator within the buffer.
//!
//! ### Validation criteria
//!
//! 1. At least 2 leading chunks (rejects single-chunk false positives).
//! 2. Every leading chunk's type byte is a known [`crate::AssetType`].
//! 3. Every leading chunk's magic matches the registered magic for its
//!    type.
//! 4. The final chunk's declared size walks past EOF by at least 1 byte
//!    (otherwise the streaming detector would have terminated this
//!    cleanly).
//! 5. The final chunk's type byte must also be known (rejects "stream
//!    decoded fine until type=0xFF garbage chunk in the middle").

use serde::Serialize;

use crate::{AssetType, parse_streaming};

/// Minimum number of valid leading chunks required.
const MIN_LEADING_CHUNKS: usize = 2;

/// Detection result.
#[derive(Debug, Clone, Serialize)]
pub struct DataFieldTruncated {
    /// Number of leading chunks that decoded cleanly (size + type + magic).
    pub leading_chunks: usize,
    /// Type byte of the final, over-large chunk.
    pub final_type_byte: u8,
    /// Declared size of the final chunk's body.
    pub final_declared_size: u32,
    /// How many bytes the final chunk's declared size would have walked
    /// past EOF.
    pub overrun_bytes: u32,
}

/// Try to detect a truncated streaming container.
pub fn detect(buf: &[u8]) -> Option<DataFieldTruncated> {
    let report = parse_streaming(buf, 4096).ok()?;
    if report.terminated {
        return None;
    }
    if !report.all_known_types || !report.all_magic_ok {
        return None;
    }

    // Leading chunks all parsed cleanly. We need at least N of them and
    // a partial trailing chunk that walks past EOF.
    if report.chunks.len() < MIN_LEADING_CHUNKS {
        return None;
    }

    // The streaming parser stops *before* consuming the partial chunk —
    // `bytes_consumed` lands at the start of the final header. Read it
    // manually and verify it walks past EOF.
    let pos = report.bytes_consumed;
    if pos + 4 > buf.len() {
        // No room for a final header at all. Not the layout we expect.
        return None;
    }
    let header = u32::from_le_bytes(buf[pos..pos + 4].try_into().ok()?);
    let type_byte = ((header >> 24) & 0xFF) as u8;
    let size = header & 0x00FF_FFFF;

    // The final chunk's type must also be known — otherwise this is just
    // a busted streaming buffer with garbage in the middle.
    if matches!(AssetType::from_byte(type_byte), AssetType::Unknown(_)) {
        return None;
    }
    if size == 0 {
        // Terminator — streaming would have completed cleanly. Shouldn't
        // happen because `report.terminated == false`, but guard anyway.
        return None;
    }

    let body_end = pos + 4 + size as usize;
    if body_end <= buf.len() {
        // Fits — wouldn't be the truncated case.
        return None;
    }

    Some(DataFieldTruncated {
        leading_chunks: report.chunks.len(),
        final_type_byte: type_byte,
        final_declared_size: size,
        overrun_bytes: (body_end - buf.len()) as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a streaming buffer with `n` clean chunks followed by a final
    /// chunk whose declared size exceeds the buffer.
    fn synth_truncated(n_clean: usize, final_overrun: usize) -> Vec<u8> {
        let mut buf = Vec::new();
        // n_clean × small type-0 (TIM) chunks with `0x00000010` magic.
        for _ in 0..n_clean {
            // header: type=0x00, size=0x10 (magic + 12 zeros)
            buf.extend_from_slice(&0x0000_0010u32.to_le_bytes());
            buf.extend_from_slice(&0x0000_0010u32.to_le_bytes()); // TIM magic
            buf.extend_from_slice(&[0u8; 12]); // body filler
        }
        // Final chunk header: type=0x02 (TMD), declared size = remaining + overrun.
        // The chunk header itself sits in `buf`; declare a body that runs
        // past the end by `final_overrun` bytes.
        let body_size_we_will_write = 16usize;
        let declared = (body_size_we_will_write + final_overrun) as u32;
        let header = (0x02u32 << 24) | (declared & 0x00FF_FFFF);
        buf.extend_from_slice(&header.to_le_bytes());
        // Magic: TMD2 magic 0x80000002. body_size_we_will_write bytes total
        // including magic. (parse_streaming reads magic from chunk[0..4].)
        buf.extend_from_slice(&0x8000_0002u32.to_le_bytes());
        buf.extend_from_slice(&[0xAAu8; 12]);
        buf
    }

    #[test]
    fn detects_truncated_after_three_chunks() {
        let buf = synth_truncated(3, 100);
        let r = detect(&buf).expect("should detect");
        assert_eq!(r.leading_chunks, 3);
        assert_eq!(r.final_type_byte, 0x02);
        assert_eq!(r.overrun_bytes, 100);
    }

    #[test]
    fn detects_truncated_after_two_chunks() {
        // The minimum-leading-chunks threshold is 2; this matches the
        // 0228_station / 0373_taiku layout (2 small chunks then a final
        // chunk whose declared MOVE-table size walks past EOF).
        let buf = synth_truncated(2, 100);
        let r = detect(&buf).expect("should detect at the threshold");
        assert_eq!(r.leading_chunks, 2);
    }

    #[test]
    fn rejects_terminated_stream() {
        // Three clean chunks + terminator = data_field_streaming territory.
        let mut buf = Vec::new();
        for _ in 0..3 {
            buf.extend_from_slice(&0x0000_0010u32.to_le_bytes());
            buf.extend_from_slice(&0x0000_0010u32.to_le_bytes());
            buf.extend_from_slice(&[0u8; 12]);
        }
        // Terminator chunk (size = 0).
        buf.extend_from_slice(&0u32.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_single_leading_chunk() {
        let buf = synth_truncated(1, 100);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_unknown_final_type() {
        // Two clean chunks + final chunk with unknown type byte.
        let mut buf = Vec::new();
        for _ in 0..3 {
            buf.extend_from_slice(&0x0000_0010u32.to_le_bytes());
            buf.extend_from_slice(&0x0000_0010u32.to_le_bytes());
            buf.extend_from_slice(&[0u8; 12]);
        }
        // Final header: type=0xFE (unknown), declared size walks past EOF.
        let header = (0xFEu32 << 24) | 100u32;
        buf.extend_from_slice(&header.to_le_bytes());
        // Less body than declared.
        buf.extend_from_slice(&[0u8; 16]);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_random_bytes() {
        let buf: Vec<u8> = (0..0x2000u32).map(|i| (i & 0xFF) as u8).collect();
        assert!(detect(&buf).is_none());
    }
}
