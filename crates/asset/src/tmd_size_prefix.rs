//! TMD-with-size-prefix detector.
//!
//! Sister to [`crate::scene_tmd_stream`] - same outer shape (`[u32 prefix][TMD
//! magic 0x80000002 at +4]`), but the on-disc payload is **shorter than the
//! prefix claims**. The runtime is presumably either (a) streaming the
//! remainder from another PROT entry, or (b) using the prefix as an
//! "in-RAM allocation size" hint and zero-filling the tail.
//!
//! ### Layout (empirically verified)
//!
//! ```text
//! +0x00   u32  total_size       ; claimed total in-memory size, > on-disc len
//! +0x04   u32  0x80000002       ; Legaia TMD magic
//! +0x08   u32  0x00000000       ; TMD flags (on-disc)
//! +0x0C   u32  nobj             ; small (typically 2 or 4)
//! +0x10   object_table[nobj]    ; 28 bytes per object (PsyQ TMD layout)
//!                                ; (vert_top, n_vert, norm_top, n_norm,
//!                                ;  prim_top, n_prim, scale)
//! +0x10 + nobj*0x1C             ; primitive data (truncated)
//! ```
//!
//! All object pointers (`vert_top`, `norm_top`, `prim_top`) point **within
//! the prefix-claimed total size** - so the on-disc file is genuinely a
//! prefix of a larger logical resource, not a malformed header.
//!
//! ### Provenance
//!
//! ~30 PROT entries match this shape. They cluster as 12 KB files (6 sectors)
//! across scene-named PROT entries - `izumi`, `cave01`, `dolk2`, `suimon`,
//! `vozz`, `keikoku`, `dream`, `tunnel*`, `kor*`, `koin*`, `bubu*`, `map0*`,
//! and similar. The shared shape suggests a per-scene "primary mesh" slot
//! that's always the same on-disc size but expands to a variable-size mesh
//! at load time.
//!
//! Distinct from [`Class::SceneTmdStream`](crate::categorize::Class::SceneTmdStream)
//! by the strict invariant `prefix_size > on-disc len`. That subset of
//! `scene_tmd_stream`-shaped files is the exact set this detector catches -
//! `scene_tmd_stream`'s `tmd_end > buf.len()` rejection branch.
//!
//! ### Format meaning - open
//!
//! The runtime consumer hasn't been located. The on-disc file is a *prefix*
//! of a logical TMD-with-streaming-tail; what supplies the missing tail
//! (a sibling PROT entry? A descriptor table elsewhere? Zero-fill?) is
//! still TBD.

use serde::Serialize;

/// Maximum sane object count. Real hits sit at 2 or 4; 8 is generous headroom.
const MAX_TMD_OBJECTS: u32 = 8;

/// Maximum sane claimed total size. The largest plausible TMD resource is
/// well under 1 MB; anything past that is almost certainly a false match.
const MAX_TOTAL_SIZE: u32 = 1024 * 1024;

/// Detection result.
#[derive(Debug, Clone, Serialize)]
pub struct TmdSizePrefix {
    /// Prefix u32 - claimed total size of the in-RAM resource. Always
    /// strictly greater than the on-disc buffer length.
    pub claimed_total: u32,
    /// Object count from `u32@12`.
    pub nobj: u32,
}

/// Try to detect a TMD-size-prefix file. Returns `None` if the buffer doesn't
/// match the strict shape.
pub fn detect(buf: &[u8]) -> Option<TmdSizePrefix> {
    if buf.len() < 16 {
        return None;
    }
    let claimed_total = legaia_bytes::u32_le(buf, 0)?;
    let magic = legaia_bytes::u32_le(buf, 4)?;
    let flags = legaia_bytes::u32_le(buf, 8)?;
    let nobj = legaia_bytes::u32_le(buf, 12)?;

    if magic != 0x80000002 {
        return None;
    }
    if flags != 0 {
        return None;
    }
    if !(1..=MAX_TMD_OBJECTS).contains(&nobj) {
        return None;
    }
    if claimed_total == 0 || claimed_total > MAX_TOTAL_SIZE {
        return None;
    }
    // Distinguish from `scene_tmd_stream`: this detector is for files whose
    // claimed body extends past the on-disc bytes. The complete (non-truncated)
    // case is already covered by `scene_tmd_stream`.
    if (claimed_total as usize) <= buf.len() {
        return None;
    }
    // Object table must fit on disc (we need to verify per-object pointers).
    let obj_table_end = 16usize.checked_add((nobj as usize) * 28)?;
    if obj_table_end > buf.len() {
        return None;
    }

    // Each object's (vert_top, norm_top, prim_top) must point inside the
    // claimed total size, AND past the object table.
    let claimed = claimed_total as usize;
    for i in 0..(nobj as usize) {
        let off = 16 + i * 28;
        let vert_top = legaia_bytes::u32_le(buf, off)? as usize;
        let n_vert = legaia_bytes::u32_le(buf, off + 4)? as usize;
        let norm_top = legaia_bytes::u32_le(buf, off + 8)? as usize;
        let n_norm = legaia_bytes::u32_le(buf, off + 12)? as usize;
        let prim_top = legaia_bytes::u32_le(buf, off + 16)? as usize;
        let n_prim = legaia_bytes::u32_le(buf, off + 20)? as usize;

        // Pointers must reside inside the claimed payload, after the obj table.
        // The vertex / normal / primitive sections themselves must also fit.
        // PSX vertex stride is 8 (3xs16 + pad); normal stride is 8; primitives
        // are variable but each is at least 8 bytes (group header).
        if vert_top < obj_table_end - 4 || vert_top > claimed {
            return None;
        }
        if norm_top > claimed {
            return None;
        }
        if prim_top > claimed {
            return None;
        }
        // Vertex / normal upper bounds.
        let vert_end = vert_top.checked_add(n_vert.checked_mul(8)?)?;
        let norm_end = norm_top.checked_add(n_norm.checked_mul(8)?)?;
        if vert_end > claimed || norm_end > claimed {
            return None;
        }
        // Sanity bound on counts.
        if n_vert > 65536 || n_norm > 65536 || n_prim > 65536 {
            return None;
        }
    }

    Some(TmdSizePrefix {
        claimed_total,
        nobj,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid header: claimed_total bytes of "data", on-disc
    /// truncated to `disc_len` bytes (must be < claimed_total). Object 0 is
    /// configured with vert / normal / prim pointers within `claimed_total`.
    fn synth(claimed_total: u32, nobj: u32, disc_len: usize) -> Vec<u8> {
        let mut buf = Vec::with_capacity(disc_len);
        buf.extend_from_slice(&claimed_total.to_le_bytes());
        buf.extend_from_slice(&0x80000002u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&nobj.to_le_bytes());
        // Object headers - 28 bytes each. Set vert/normal/prim pointers
        // safely within the claimed range, with small counts that fit.
        let obj_table_end = 16 + (nobj as usize) * 28;
        for i in 0..nobj {
            let base = (obj_table_end as u32) + i * 0x40;
            // vert_top, n_vert, norm_top, n_norm, prim_top, n_prim, scale
            buf.extend_from_slice(&base.to_le_bytes()); // vert_top
            buf.extend_from_slice(&8u32.to_le_bytes()); // n_vert (8 verts = 64 bytes)
            buf.extend_from_slice(&(base + 0x40 / 2).to_le_bytes()); // norm_top
            buf.extend_from_slice(&0u32.to_le_bytes()); // n_norm
            buf.extend_from_slice(&(base + 0x10).to_le_bytes()); // prim_top
            buf.extend_from_slice(&4u32.to_le_bytes()); // n_prim
            buf.extend_from_slice(&0x00808080u32.to_le_bytes()); // scale
        }
        // Pad on-disc data with zeros up to `disc_len`.
        buf.resize(disc_len, 0);
        buf
    }

    #[test]
    fn detects_truncated_header() {
        // claimed = 17696, on-disc = 12288 (typical real-world layout).
        let buf = synth(17696, 2, 12288);
        let r = detect(&buf).expect("should detect");
        assert_eq!(r.claimed_total, 17696);
        assert_eq!(r.nobj, 2);
    }

    #[test]
    fn rejects_when_complete_on_disc() {
        // If claimed_total <= on-disc, this is scene_tmd_stream's territory.
        let buf = synth(2048, 2, 4096);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_when_magic_mismatched() {
        let mut buf = synth(17696, 2, 12288);
        buf[4] = 0x42;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_when_flags_nonzero() {
        let mut buf = synth(17696, 2, 12288);
        buf[8] = 1;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_when_nobj_zero() {
        let mut buf = synth(17696, 2, 12288);
        buf[12] = 0;
        buf[13] = 0;
        buf[14] = 0;
        buf[15] = 0;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_when_nobj_implausibly_large() {
        let mut buf = synth(17696, 2, 12288);
        buf[12] = 0xFF;
        buf[13] = 0xFF;
        buf[14] = 0;
        buf[15] = 0;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_when_vert_top_outside_claimed_range() {
        let mut buf = synth(17696, 2, 12288);
        // Stomp object 0 vert_top to a value > claimed.
        let off = 16; // vert_top is the first u32 of object 0.
        buf[off..off + 4].copy_from_slice(&0x10000000u32.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_random_bytes() {
        let buf: Vec<u8> = (0..512u32)
            .map(|i| (i.wrapping_mul(11) & 0xFF) as u8)
            .collect();
        assert!(detect(&buf).is_none());
    }
}
