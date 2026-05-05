//! Streaming-chunk pack format.
//!
//! Used inside the data payload of `AssetType::TimList` (type 1) and
//! `AssetType::Tmd` (type 2) chunks in DATA_FIELD streaming files. Layout:
//!
//! ```text
//! u32 count
//! u32 offset[count]    // word indices from the start of THIS data
//! ...                  // sub-asset bytes, packed back-to-back
//! ```
//!
//! Each `offset[i]` is in 4-byte words from the pack-data start. So the
//! byte position of sub-asset `i` is `offset[i] * 4`. The end of sub-asset
//! `i` is either `offset[i+1] * 4` or, for the last one, the end of the
//! pack-data buffer.
//!
//! This is **not** the standalone TIM-pack format (`byte[3]==0x01` marker,
//! count in `byte[2]`; see `crates/prot/src/timpack.rs`). The streaming-chunk
//! pack stores count as a full u32 and lacks the 4-byte marker prefix.

use anyhow::{Result, bail};
use serde::Serialize;

/// A single entry in a pack: byte range within the pack data.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PackEntry {
    pub index: usize,
    pub byte_offset: usize,
    pub size: usize,
}

/// Sanity bound on the count u32. Real packs have small counts (TIM_LIST
/// in retock had 2; TMD had 29). 64K is a paranoid upper limit.
const MAX_REASONABLE_COUNT: u32 = 65_536;

/// Parse the pack header and return entry ranges. The input is the chunk's
/// data payload (NOT including the 4-byte chunk header).
pub fn parse_pack(data: &[u8]) -> Result<Vec<PackEntry>> {
    if data.len() < 4 {
        bail!("pack data too small ({} bytes)", data.len());
    }
    let count = u32::from_le_bytes(data[0..4].try_into().unwrap());
    if count == 0 {
        return Ok(Vec::new());
    }
    if count > MAX_REASONABLE_COUNT {
        bail!("implausible pack count {}", count);
    }
    let count = count as usize;
    let table_end = 4 + count * 4;
    if table_end > data.len() {
        bail!(
            "pack offset table ({} entries, ends at byte {}) overruns data ({} bytes)",
            count,
            table_end,
            data.len()
        );
    }

    let mut byte_offsets: Vec<usize> = Vec::with_capacity(count + 1);
    for i in 0..count {
        let off_word = u32::from_le_bytes(data[4 + i * 4..8 + i * 4].try_into().unwrap()) as usize;
        let off_byte = off_word.checked_mul(4).ok_or_else(|| {
            anyhow::anyhow!(
                "offset[{}] = {} overflows when multiplied by 4",
                i,
                off_word
            )
        })?;
        if off_byte > data.len() {
            bail!(
                "offset[{}] = byte {} past data end ({} bytes)",
                i,
                off_byte,
                data.len()
            );
        }
        byte_offsets.push(off_byte);
    }
    // Offsets must be monotonically non-decreasing for slice ranges to work.
    // Real packs have strictly increasing offsets; some entries can have
    // zero size if duplicated. We enforce non-decreasing.
    for w in byte_offsets.windows(2) {
        if w[0] > w[1] {
            bail!("offsets not monotonic: {} -> {}", w[0], w[1]);
        }
    }
    // Append data length as a sentinel for the last entry's end.
    byte_offsets.push(data.len());

    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let s = byte_offsets[i];
        let e = byte_offsets[i + 1];
        out.push(PackEntry {
            index: i,
            byte_offset: s,
            size: e - s,
        });
    }
    Ok(out)
}

/// Return slices for each entry in the pack.
pub fn extract_pack(data: &[u8]) -> Result<Vec<&[u8]>> {
    let entries = parse_pack(data)?;
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        out.push(&data[e.byte_offset..e.byte_offset + e.size]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_two_tims() {
        // count=2, offset[0]=3 (=12 bytes), offset[1]=5 (=20 bytes)
        // → entry 0: bytes 12..20 (size 8); entry 1: bytes 20..end
        let mut data = Vec::new();
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(&5u32.to_le_bytes());
        // 12 bytes of header, then 8 bytes for entry[0], then 4 bytes for entry[1]
        data.extend_from_slice(&[0xAA; 8]);
        data.extend_from_slice(&[0xBB; 4]);

        let entries = parse_pack(&data).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].byte_offset, 12);
        assert_eq!(entries[0].size, 8);
        assert_eq!(entries[1].byte_offset, 20);
        assert_eq!(entries[1].size, 4);

        let slices = extract_pack(&data).unwrap();
        assert_eq!(slices[0], &[0xAA; 8][..]);
        assert_eq!(slices[1], &[0xBB; 4][..]);
    }

    #[test]
    fn pack_zero_count_is_empty() {
        let data = 0u32.to_le_bytes();
        let entries = parse_pack(&data).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn pack_rejects_implausible_count() {
        let data = u32::MAX.to_le_bytes();
        assert!(parse_pack(&data).is_err());
    }

    #[test]
    fn pack_rejects_overrun_table() {
        // claims count=10 but only 4 bytes of header
        let data = 10u32.to_le_bytes();
        assert!(parse_pack(&data).is_err());
    }

    #[test]
    fn pack_rejects_offset_past_end() {
        // count=1, offset[0] = 1000 words = 4000 bytes, but only 8 bytes total
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&1000u32.to_le_bytes());
        assert!(parse_pack(&data).is_err());
    }
}
