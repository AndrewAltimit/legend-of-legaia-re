//! "v12-cluster" detector — a strict structural detector for the largest
//! remaining sub-cluster of the `unknown_high_entropy` bucket.
//!
//! ### Provenance
//!
//! Round-18 cluster characterisation (2026-05-04) found that **97 PROT entries
//! share a strict 6-byte signature** at bytes 2..8: `12 00 00 00 14 00`. All
//! 97 are scene-named (one entry per CDNAME label, 97 unique scenes), 30 KB to
//! 387 KB in size, and dense (>96 % nonzero past offset 0x2000).
//!
//! ### Layout
//!
//! ```text
//! +0x00   u16  N + 4          ; first offset table base = `N + 4`
//! +0x02   u16  0x0012         ; constant
//! +0x04   u16  0x0000         ; constant
//! +0x06   u16  0x0014         ; constant — second-table base offset *header*
//! +0x08   u16  ?              ; per-scene parameter (varies)
//! +0x0A   u16  N              ; record count for the first table
//! +0x0C   u16  0x0000         ; constant
//! +0x0E   u16  N + 2          ; second offset table base = `N + 2`
//! ...                          ; trailing dense data
//! ```
//!
//! `u16[0]` and `u16[7]` are tied to `u16[5]` by the algebraic identities
//! `u16[0] == N + 4` and `u16[7] == N + 2`. These constraints, paired with the
//! three constant words at `u16[1] / u16[2] / u16[3]` and `u16[6]`, are
//! specific enough that a full corpus scan finds **97 / 1234** entries — zero
//! false positives — every match is a current `unknown_high_entropy`,
//! `unknown_other`, or misclassed `field_pack` entry.
//!
//! ### Format meaning — open
//!
//! The runtime consumer hasn't been located. Likely candidates: per-scene
//! navmesh / collision data, scene event-trigger tables, or a scene-local SEQ
//! sequencer block. Until the consumer is reversed we deliberately keep the
//! detector / class name **format-agnostic** — `Class::SceneV12Table` reflects
//! the structural signature, not a guessed semantic.
//!
//! ### Coverage impact
//!
//! Promotes 97 entries out of `unknown_high_entropy` (95) / `unknown_other`
//! (1) / `field_pack` (1 misclass — `0002_gameover_data.BIN`). Coverage moves
//! from 532 / 1232 (43.2 %) to 629 / 1232 (51.1 %).
//!
//! See `docs/formats/scene-bundles.md` for the byte-level spec.

use serde::Serialize;

/// First constant word. The header is `[N+4, 0x12, 0, 0x14, ?, N, 0, N+2]`.
const W1_MAGIC: u16 = 0x0012;
const W3_MAGIC: u16 = 0x0014;

/// Minimum sane record count. The smallest observed live entry has `N == 0x18`
/// (24 records). Defensive bound — anything below 8 is almost certainly a
/// false match on stray bytes that happen to satisfy the constants.
const MIN_N: u16 = 8;

/// Maximum sane record count. The largest observed live entry has
/// `N == 0x16E` (366 records). Cap at 4096 to leave headroom for unseen
/// variants while still rejecting random buffers whose `u16[5]` falls in the
/// 0x0000..=0xFFFF range.
const MAX_N: u16 = 4096;

/// Detection result.
#[derive(Debug, Clone, Serialize)]
pub struct SceneV12Table {
    /// Record count from `u16[5]`. The two offset tables base at `N + 4` and
    /// `N + 2` (see module docs).
    pub n: u16,
    /// Per-scene parameter from `u16[4]`. Varies across the corpus; semantics
    /// not yet understood. Surfaced for downstream tooling.
    pub param: u16,
    /// First offset-table base — algebraically `n + 4`. Stored for caller
    /// convenience; equals `self.n + 4`.
    pub table_a_base: u16,
    /// Second offset-table base — algebraically `n + 2`. Equals `self.n + 2`.
    pub table_b_base: u16,
}

/// Try to detect a v12-cluster table. Returns `None` when the buffer doesn't
/// match the strict 8-word header.
pub fn detect(buf: &[u8]) -> Option<SceneV12Table> {
    if buf.len() < 16 {
        return None;
    }
    let n_plus_4 = read_u16_le(buf, 0)?;
    let w1 = read_u16_le(buf, 2)?;
    let w2 = read_u16_le(buf, 4)?;
    let w3 = read_u16_le(buf, 6)?;
    let param = read_u16_le(buf, 8)?;
    let n = read_u16_le(buf, 10)?;
    let w6 = read_u16_le(buf, 12)?;
    let n_plus_2 = read_u16_le(buf, 14)?;

    // Three constant words.
    if w1 != W1_MAGIC || w2 != 0 || w3 != W3_MAGIC || w6 != 0 {
        return None;
    }
    // Algebraic ties. Use saturating math so `n` near `u16::MAX` doesn't
    // accidentally validate via wrap.
    if !(MIN_N..=MAX_N).contains(&n) {
        return None;
    }
    if n_plus_4 != n.checked_add(4)? || n_plus_2 != n.checked_add(2)? {
        return None;
    }

    Some(SceneV12Table {
        n,
        param,
        table_a_base: n_plus_4,
        table_b_base: n_plus_2,
    })
}

fn read_u16_le(buf: &[u8], at: usize) -> Option<u16> {
    let bytes = buf.get(at..at + 2)?;
    Some(u16::from_le_bytes(bytes.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid v12 header with caller-chosen `n` and `param`.
    fn synth(n: u16, param: u16, total_size: usize) -> Vec<u8> {
        let mut buf = Vec::with_capacity(total_size);
        buf.extend_from_slice(&(n + 4).to_le_bytes());
        buf.extend_from_slice(&W1_MAGIC.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&W3_MAGIC.to_le_bytes());
        buf.extend_from_slice(&param.to_le_bytes());
        buf.extend_from_slice(&n.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&(n + 2).to_le_bytes());
        buf.resize(total_size.max(buf.len()), 0);
        buf
    }

    #[test]
    fn detects_minimal_header() {
        let buf = synth(0xE2, 0x33, 0x100);
        let s = detect(&buf).expect("should detect");
        assert_eq!(s.n, 0xE2);
        assert_eq!(s.param, 0x33);
        assert_eq!(s.table_a_base, 0xE6);
        assert_eq!(s.table_b_base, 0xE4);
    }

    #[test]
    fn rejects_buffer_smaller_than_header() {
        assert!(detect(&[0u8; 8]).is_none());
        assert!(detect(&[0u8; 15]).is_none());
    }

    #[test]
    fn rejects_wrong_constant_at_w1() {
        let mut buf = synth(0x100, 0x10, 0x100);
        // Corrupt u16[1] from 0x0012 to 0x0013.
        buf[2] = 0x13;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_wrong_constant_at_w3() {
        let mut buf = synth(0x100, 0x10, 0x100);
        // Corrupt u16[3] from 0x0014 to 0x0015.
        buf[6] = 0x15;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_nonzero_w2() {
        let mut buf = synth(0x100, 0x10, 0x100);
        buf[4] = 0xFF;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_nonzero_w6() {
        let mut buf = synth(0x100, 0x10, 0x100);
        buf[12] = 0xFF;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_n_below_min() {
        let buf = synth(7, 0x10, 0x100);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_n_above_max() {
        let buf = synth(5000, 0x10, 0x10000);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_n_plus_4_mismatch() {
        let mut buf = synth(0x100, 0x10, 0x200);
        // Corrupt u16[0] so it no longer equals n + 4.
        buf[0] = 0x05;
        buf[1] = 0x00;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_n_plus_2_mismatch() {
        let mut buf = synth(0x100, 0x10, 0x200);
        // Corrupt u16[7] so it no longer equals n + 2.
        buf[14] = 0xFF;
        buf[15] = 0xFF;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn accepts_real_world_head_pattern_gameover_data() {
        // 0002_gameover_data.BIN head:
        // E6 00 12 00 00 00 14 00 33 00 E2 00 00 00 E4 00
        let mut buf = vec![
            0xE6, 0x00, 0x12, 0x00, 0x00, 0x00, 0x14, 0x00, 0x33, 0x00, 0xE2, 0x00, 0x00, 0x00,
            0xE4, 0x00,
        ];
        buf.resize(0x100, 0);
        let s = detect(&buf).expect("real-world pattern should detect");
        assert_eq!(s.n, 0xE2);
        assert_eq!(s.param, 0x33);
    }

    #[test]
    fn accepts_real_world_head_pattern_town01() {
        // 0011_town01.BIN head:
        // 16 01 12 00 00 00 14 00 3F 00 12 01 00 00 14 01
        let mut buf = vec![
            0x16, 0x01, 0x12, 0x00, 0x00, 0x00, 0x14, 0x00, 0x3F, 0x00, 0x12, 0x01, 0x00, 0x00,
            0x14, 0x01,
        ];
        buf.resize(0x100, 0);
        let s = detect(&buf).expect("real-world pattern should detect");
        assert_eq!(s.n, 0x0112);
        assert_eq!(s.param, 0x003F);
    }

    #[test]
    fn rejects_random_buffer() {
        // Cycle 0..255 — no chance of matching the strict header.
        let buf: Vec<u8> = (0..=255u8).cycle().take(0x100).collect();
        assert!(detect(&buf).is_none());
    }
}
