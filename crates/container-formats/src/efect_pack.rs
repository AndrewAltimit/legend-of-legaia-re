//! Structural detector for the runtime `efect.dat` 2-pack (PROT entry 0873).
//!
//! `data\battle\efect.dat` is **not** the magic-prefixed
//! [`effect_bundle`](crate::effect_bundle) container - it is a headerless
//! 2-pack whose first two words are the file offsets of its two packs, fixed
//! up to absolute pointers on first init. The byte-level record layouts (sprite
//! atlas entries, pack0 frame batches, pack1 spawn scripts) and their
//! provenance are on
//! [`docs/formats/effect.md`](../../../../docs/formats/effect.md); the runtime
//! parser is `legaia_engine_vm::effect_vm::EffectCatalog::from_efect_dat_bytes`.
//!
//! ```text
//! +0x00   u32  pack0_offset
//! +0x04   u32  pack1_offset
//! +0x08        sprite-atlas entries, 8 bytes each, up to pack0_offset
//! +pack0  u32  count, u32 entry_offsets[count]   (absolute file offsets)
//! +pack1  u32  count, u32 entry_offsets[count]   (absolute file offsets)
//! ```
//!
//! This module exists so `categorize` can put a name on the entry from bytes
//! alone. The detector is the pack tables' own self-consistency - each table's
//! first entry offset must land exactly on the end of its offset array, the
//! offsets must ascend, and every one must stay inside its pack's extent. That
//! is a strong enough shape that it fires on one PROT entry and nothing else.

/// Bytes per inline sprite-atlas entry (`u`, `v`, `w`, `h`, + 4 tail bytes).
pub const ATLAS_ENTRY_BYTES: usize = 8;

/// Recognised `efect.dat` 2-pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EfectPack {
    /// File offset of pack 0 (frame-batched sprite animations).
    pub pack0_offset: usize,
    /// File offset of pack 1 (effect-id spawn scripts).
    pub pack1_offset: usize,
    /// Inline sprite-atlas entry count (`(pack0_offset - 8) / 8`).
    pub atlas_entries: usize,
    /// Entry count of pack 0.
    pub pack0_count: usize,
    /// Entry count of pack 1.
    pub pack1_count: usize,
}

fn u32_at(buf: &[u8], off: usize) -> Option<usize> {
    buf.get(off..off + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
}

/// Read one pack's `[u32 count][u32 offsets[count]]` table and check that the
/// offsets are ascending, land past the table, and stay below `limit`.
fn pack_count(buf: &[u8], at: usize, limit: usize) -> Option<usize> {
    let count = u32_at(buf, at)?;
    if count == 0 || count > 1024 {
        return None;
    }
    let table_end = at.checked_add(4 + 4 * count)?;
    if table_end > limit {
        return None;
    }
    let mut prev = 0usize;
    for i in 0..count {
        let off = u32_at(buf, at + 4 + 4 * i)?;
        if off < table_end || off >= limit || (i > 0 && off <= prev) {
            return None;
        }
        prev = off;
    }
    // The first entry starts immediately after the offset array - no slack.
    if u32_at(buf, at + 4)? != table_end {
        return None;
    }
    Some(count)
}

/// Recognise the runtime `efect.dat` 2-pack.
pub fn detect(buf: &[u8]) -> Option<EfectPack> {
    let pack0_offset = u32_at(buf, 0)?;
    let pack1_offset = u32_at(buf, 4)?;
    if pack0_offset <= 8 || pack1_offset <= pack0_offset || pack1_offset >= buf.len() {
        return None;
    }
    if !(pack0_offset - 8).is_multiple_of(ATLAS_ENTRY_BYTES) {
        return None;
    }
    let pack0_count = pack_count(buf, pack0_offset, pack1_offset)?;
    let pack1_count = pack_count(buf, pack1_offset, buf.len())?;
    Some(EfectPack {
        pack0_offset,
        pack1_offset,
        atlas_entries: (pack0_offset - 8) / ATLAS_ENTRY_BYTES,
        pack0_count,
        pack1_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal but structurally valid 2-pack.
    fn synth() -> Vec<u8> {
        let atlas = 16usize;
        let p0 = 8 + atlas * ATLAS_ENTRY_BYTES;
        let n0 = 3usize;
        let p0_end = p0 + 4 + 4 * n0;
        let p1 = p0_end + 3 * 16;
        let n1 = 2usize;
        let p1_end = p1 + 4 + 4 * n1;
        let total = p1_end + 2 * 32;
        let mut buf = vec![0u8; total];
        let put = |buf: &mut Vec<u8>, at: usize, v: usize| {
            buf[at..at + 4].copy_from_slice(&(v as u32).to_le_bytes());
        };
        put(&mut buf, 0, p0);
        put(&mut buf, 4, p1);
        put(&mut buf, p0, n0);
        for i in 0..n0 {
            put(&mut buf, p0 + 4 + 4 * i, p0_end + i * 16);
        }
        put(&mut buf, p1, n1);
        for i in 0..n1 {
            put(&mut buf, p1 + 4 + 4 * i, p1_end + i * 32);
        }
        buf
    }

    #[test]
    fn detects_the_synthetic_two_pack() {
        let buf = synth();
        let p = detect(&buf).expect("2-pack");
        assert_eq!(p.atlas_entries, 16);
        assert_eq!(p.pack0_count, 3);
        assert_eq!(p.pack1_count, 2);
    }

    #[test]
    fn rejects_a_misaligned_atlas() {
        let mut buf = synth();
        // pack0_offset no longer 8 + k*8.
        buf[0] = buf[0].wrapping_add(4);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_slack_before_the_first_entry() {
        let mut buf = synth();
        let p0 = u32_at(&buf, 0).unwrap();
        let first = u32_at(&buf, p0 + 4).unwrap();
        buf[p0 + 4..p0 + 8].copy_from_slice(&((first + 4) as u32).to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_zeros_and_short_buffers() {
        assert!(detect(&[]).is_none());
        assert!(detect(&vec![0u8; 4096]).is_none());
    }
}
