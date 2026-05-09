//! Multi-bank VAB archive detector.
//!
//! Layout: `[u32 reserved=0][u32 count=N][u32 sector_nums[N]]`
//! where at each `sector_nums[i] * 0x800 + 4` the VABp magic appears.
//!
//! Covers the level_up cluster's multi-bank sound archive (206 VABp entries).

const VAB_MAGIC: &[u8; 4] = &[0x70, 0x42, 0x41, 0x56]; // 'pBAV' LE = VABp

#[derive(Debug, Clone)]
pub struct VabMultiBank {
    pub count: usize,
}

pub fn detect(buf: &[u8]) -> Option<VabMultiBank> {
    if buf.len() < 12 {
        return None;
    }
    let reserved = u32::from_le_bytes(buf[0..4].try_into().ok()?);
    if reserved != 0 {
        return None;
    }
    let count = u32::from_le_bytes(buf[4..8].try_into().ok()?) as usize;
    if !(4..=1024).contains(&count) {
        return None;
    }
    let header_end = 8usize.checked_add(count.checked_mul(4)?)?;
    if header_end > buf.len() {
        return None;
    }
    // Check that the first sector contains VABp magic at sector*0x800+4
    let first_sector = u32::from_le_bytes(buf[8..12].try_into().ok()?) as usize;
    if first_sector == 0 {
        return None;
    }
    let vab_pos = first_sector.checked_mul(0x800)?.checked_add(4)?;
    if vab_pos + 4 > buf.len() {
        return None;
    }
    if &buf[vab_pos..vab_pos + 4] != VAB_MAGIC {
        return None;
    }
    Some(VabMultiBank { count })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_buf(count: usize, first_sector: u32) -> Vec<u8> {
        // Minimum size: header (8 + count*4) + sector data up to vab_pos+4
        let header_size = 8 + count * 4;
        let vab_pos = (first_sector as usize) * 0x800 + 4;
        let total_size = vab_pos + 4;
        let mut buf = vec![0u8; total_size.max(header_size)];
        // reserved = 0 (already zero)
        buf[4..8].copy_from_slice(&(count as u32).to_le_bytes());
        // sector_nums[0] = first_sector
        buf[8..12].copy_from_slice(&first_sector.to_le_bytes());
        // fill remaining sector entries with 0
        // write VABp magic at sector*0x800+4
        buf[vab_pos..vab_pos + 4].copy_from_slice(VAB_MAGIC);
        buf
    }

    #[test]
    fn detects_minimal_valid_archive() {
        let buf = make_buf(4, 1);
        let result = detect(&buf);
        assert!(result.is_some(), "should detect minimal valid archive");
        assert_eq!(result.unwrap().count, 4);
    }

    #[test]
    fn rejects_nonzero_reserved() {
        let mut buf = make_buf(4, 1);
        buf[0] = 1; // break reserved=0
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_zero_first_sector() {
        let mut buf = make_buf(4, 1);
        // set first_sector to 0
        buf[8..12].copy_from_slice(&0u32.to_le_bytes());
        // remove the VABp magic at position 4 (where sector=0 would put it)
        // already missing — just test
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_count_too_small() {
        // count=2 is below minimum 4
        let buf = make_buf(2, 1);
        // manually set count to 2
        let mut buf2 = buf.clone();
        buf2[4..8].copy_from_slice(&2u32.to_le_bytes());
        assert!(detect(&buf2).is_none());
    }

    #[test]
    fn rejects_wrong_magic() {
        let mut buf = make_buf(4, 1);
        let vab_pos = 0x800 + 4;
        buf[vab_pos] = 0xFF; // corrupt the magic
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn detects_with_206_count() {
        let buf = make_buf(206, 1);
        let result = detect(&buf);
        assert!(result.is_some());
        assert_eq!(result.unwrap().count, 206);
    }
}
