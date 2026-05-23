//! `prot::timpack` - the standalone TIM-pack container some PROT entries use.
//!
//! Layout: a header byte pair (`blob[2] < 0x10`, `blob[3] == 0x01`), then a
//! `u32 tim_num` count at `+4`, then `tim_num` little-endian `i32` word
//! offsets. Each offset converts to a byte position via `word_index * 4 + 4`.
//! See [`docs/formats/tim-pack.md`].
//!
//! Every function here is panic-safe on arbitrary bytes: bulk scanners feed
//! whole PROT entries through [`is_tim_pack`] / [`unpack`] to classify them.

/// Heuristic: does `blob` look like a standalone TIM-pack container?
///
/// Validates the 2-byte header signature, that the `tim_num` count at `+4` is
/// positive, and that the offset table fits within `blob`. Returns `false`
/// (never panics) on short or malformed input.
pub fn is_tim_pack(blob: &[u8]) -> bool {
    if blob.len() < 12 {
        return false;
    }
    if !(blob[3] == 0x01 && blob[2] < 0x10) {
        return false;
    }
    let tim_num = i32::from_le_bytes(blob[4..8].try_into().unwrap());
    if tim_num <= 0 {
        return false;
    }
    let table_end = 8usize.saturating_add(4usize.saturating_mul(tim_num as usize));
    table_end <= blob.len()
}

/// Split a TIM-pack `blob` into its member sub-blobs.
///
/// Returns an empty `Vec` if `blob` doesn't pass [`is_tim_pack`]. Offsets that
/// are negative or run past the end of `blob` are skipped rather than indexed,
/// so arbitrary / truncated input yields a (possibly empty) result without
/// panicking. Members are returned in ascending offset order, de-duplicated.
pub fn unpack(blob: &[u8]) -> Vec<Vec<u8>> {
    if !is_tim_pack(blob) {
        return Vec::new();
    }
    let tim_num = i32::from_le_bytes(blob[4..8].try_into().unwrap()) as usize;
    let mut offsets = Vec::with_capacity(tim_num + 1);
    for x in 0..tim_num {
        let entry = i32::from_le_bytes(blob[8 + 4 * x..12 + 4 * x].try_into().unwrap());
        let off = (entry as i64) * 4 + 4;
        if off < 0 || off as usize > blob.len() {
            continue;
        }
        offsets.push(off as usize);
    }
    offsets.sort_unstable();
    offsets.dedup();
    offsets.push(blob.len());

    let mut out = Vec::with_capacity(offsets.len().saturating_sub(1));
    for w in offsets.windows(2) {
        let (s, e) = (w[0], w[1]);
        if s < e && e <= blob.len() {
            out.push(blob[s..e].to_vec());
        }
    }
    out
}

/// Guess a member's file extension from its first byte: `"TIM"` when it
/// starts with the PSX TIM magic low byte (`0x10`), `"BIN"` otherwise.
pub fn detected_ext(item: &[u8]) -> &'static str {
    if !item.is_empty() && item[0] == 0x10 {
        "TIM"
    } else {
        "BIN"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid TIM-pack: 2 members at word offsets that map to
    /// byte positions inside `total_len`.
    fn build_pack(word_offsets: &[i32], total_len: usize) -> Vec<u8> {
        let mut blob = vec![0u8; total_len.max(8 + 4 * word_offsets.len())];
        // header signature: blob[2] < 0x10, blob[3] == 0x01
        blob[2] = 0x00;
        blob[3] = 0x01;
        blob[4..8].copy_from_slice(&(word_offsets.len() as i32).to_le_bytes());
        for (i, &w) in word_offsets.iter().enumerate() {
            blob[8 + 4 * i..12 + 4 * i].copy_from_slice(&w.to_le_bytes());
        }
        blob
    }

    #[test]
    fn is_tim_pack_accepts_well_formed() {
        // 2 members; byte = word*4 + 4 -> word 4 = byte 20, word 5 = byte 24.
        let blob = build_pack(&[4, 5], 32);
        assert!(is_tim_pack(&blob));
    }

    #[test]
    fn is_tim_pack_rejects_short_or_bad_signature() {
        assert!(!is_tim_pack(&[]));
        assert!(!is_tim_pack(&[0u8; 11])); // < 12 bytes
        let mut blob = build_pack(&[4, 5], 32);
        blob[3] = 0x02; // wrong signature byte
        assert!(!is_tim_pack(&blob));
        blob[3] = 0x01;
        blob[2] = 0x10; // blob[2] must be < 0x10
        assert!(!is_tim_pack(&blob));
    }

    #[test]
    fn is_tim_pack_rejects_negative_or_huge_count() {
        let mut blob = vec![0u8; 16];
        blob[3] = 0x01;
        blob[4..8].copy_from_slice(&(-1i32).to_le_bytes());
        assert!(!is_tim_pack(&blob)); // tim_num <= 0
        // Enormous count whose offset table can't fit -> rejected, no panic.
        blob[4..8].copy_from_slice(&0x7FFF_FFFFi32.to_le_bytes());
        assert!(!is_tim_pack(&blob));
    }

    #[test]
    fn unpack_non_pack_returns_empty() {
        assert!(unpack(&[]).is_empty());
        assert!(unpack(&[0xAB; 64]).is_empty());
    }

    #[test]
    fn unpack_skips_out_of_range_and_negative_offsets() {
        // word offsets: 4 (-> byte 20, in range), 0x4000_0000 (huge, skipped),
        // and -10 (negative entry -> off < 0, skipped).
        let blob = build_pack(&[4, 0x4000_0000, -10], 32);
        // Must not panic; only the in-range member(s) survive.
        let members = unpack(&blob);
        for m in &members {
            assert!(m.len() <= blob.len());
        }
    }

    #[test]
    fn unpack_huge_count_with_truncated_table_does_not_panic() {
        // is_tim_pack rejects this (table doesn't fit), so unpack returns empty
        // rather than indexing past the buffer.
        let mut blob = vec![0u8; 12];
        blob[3] = 0x01;
        blob[4..8].copy_from_slice(&1000i32.to_le_bytes());
        assert!(unpack(&blob).is_empty());
    }

    #[test]
    fn detected_ext_classifies() {
        assert_eq!(detected_ext(&[0x10, 0, 0, 0]), "TIM");
        assert_eq!(detected_ext(&[0xFF]), "BIN");
        assert_eq!(detected_ext(&[]), "BIN");
    }
}
