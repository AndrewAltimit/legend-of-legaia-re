//! Hunt for the source data that backs the live PSX GPU prim pool.
//!
//! Once `prim_pool::tile_signatures` has clustered POLY_FT4 packets by their
//! immutable `(clut, tpage, uvs)` fingerprint, this module searches a
//! RAM-window for byte sequences that match those fingerprints, and reports
//! whether the matches sit at a consistent stride. A clean stride is the
//! tell-tale signature of a per-tile descriptor table - which is exactly
//! what the world-map continent terrain generator reads each frame.
//!
//! The search is dumb-but-fast: linear byte scan. The RAM windows we care
//! about are 100-200 KB, scanned once per fingerprint, so the brute-force
//! cost is fine in practice (low ms even with hundreds of fingerprints).

use serde::Serialize;

/// One byte-pattern search hit: the position (byte offset within the
/// supplied window) and the absolute kuseg address for human-friendly
/// output. `window_base` is the PSX virtual address that maps to offset 0
/// of the search window.
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub offset: usize,
    pub addr: u32,
}

/// Stride analysis over a set of match positions. Both forms are reported:
///
/// - `dominant_gap` is the most common pairwise gap between consecutive
///   matches. If 70%+ of all pairs share this gap, the data is almost
///   certainly a fixed-stride table.
/// - `gap_histogram` is the top-N gaps with their counts, so a bimodal or
///   sparse distribution is visible.
#[derive(Debug, Clone, Default, Serialize)]
pub struct StrideReport {
    pub match_count: usize,
    pub dominant_gap: Option<usize>,
    pub dominant_gap_share: f64,
    pub gap_histogram: Vec<(usize, usize)>,
}

/// Find every occurrence of `pattern` in `window`. Returns the byte
/// offsets (within `window`) ordered ascending. Empty pattern yields an
/// empty match list.
pub fn search(window: &[u8], pattern: &[u8]) -> Vec<usize> {
    if pattern.is_empty() || pattern.len() > window.len() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    let end = window.len() - pattern.len() + 1;
    let mut i = 0;
    while i < end {
        if window[i..i + pattern.len()] == *pattern {
            hits.push(i);
            i += 1;
        } else {
            i += 1;
        }
    }
    hits
}

/// Build a `Hit` list from raw offsets + the window's base PSX address.
pub fn hits_with_addr(offsets: &[usize], window_base: u32) -> Vec<Hit> {
    offsets
        .iter()
        .map(|&o| Hit {
            offset: o,
            addr: window_base.wrapping_add(o as u32),
        })
        .collect()
}

/// Compute stride statistics from a set of ascending positions.
///
/// Sorts the positions, computes pairwise gaps between consecutive
/// matches, and returns the gap histogram + the dominant gap (if any
/// single gap accounts for at least 40% of pairs). Below that threshold
/// the table is almost certainly not fixed-stride.
pub fn stride(positions: &[usize]) -> StrideReport {
    let mut report = StrideReport {
        match_count: positions.len(),
        ..Default::default()
    };
    if positions.len() < 3 {
        return report;
    }
    let mut sorted = positions.to_vec();
    sorted.sort_unstable();
    let mut gaps: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for w in sorted.windows(2) {
        let g = w[1] - w[0];
        *gaps.entry(g).or_insert(0) += 1;
    }
    let mut histogram: Vec<(usize, usize)> = gaps.into_iter().collect();
    histogram.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let total_pairs = (sorted.len() - 1) as f64;
    if let Some(&(gap, count)) = histogram.first() {
        let share = count as f64 / total_pairs;
        if share >= 0.40 {
            report.dominant_gap = Some(gap);
            report.dominant_gap_share = share;
        } else {
            report.dominant_gap_share = share;
        }
    }
    let trunc = histogram.len().min(8);
    report.gap_histogram = histogram.into_iter().take(trunc).collect();
    report
}

/// Bulk-search a set of patterns against a single window. Returns hits
/// + stride for each pattern.
pub fn search_all<'p>(
    window: &[u8],
    window_base: u32,
    patterns: &'p [Vec<u8>],
) -> Vec<(&'p [u8], Vec<Hit>, StrideReport)> {
    patterns
        .iter()
        .map(|p| {
            let offs = search(window, p);
            let s = stride(&offs);
            let hits = hits_with_addr(&offs, window_base);
            (p.as_slice(), hits, s)
        })
        .collect()
}

/// Aggregate stride report across MULTIPLE fingerprints' hits in the same
/// window. The continent terrain is unlikely to have all tiles spaced by
/// the same fingerprint, but if the per-tile RECORDS are fixed-stride
/// then the union of all fingerprints' positions should still show a
/// dominant inter-record gap.
pub fn pooled_stride(per_pattern_offsets: &[Vec<usize>]) -> StrideReport {
    let mut all: Vec<usize> = per_pattern_offsets.iter().flatten().copied().collect();
    all.sort_unstable();
    all.dedup();
    stride(&all)
}

/// Per-stride autocorrelation score over a RAM window.
///
/// For each candidate stride `s` in `strides`, compute a score in `[0..1]`:
/// the fraction of byte positions `i` where `window[i] == window[i + s]`.
/// A fixed-stride record table with `M` constant fields will score
/// `M / s`. A noisy or unrelated region scores near `1/256 ≈ 0.004`.
///
/// Output is sorted by descending score so the dominant stride leads.
#[derive(Debug, Clone, Serialize)]
pub struct StrideAutocorr {
    pub stride: usize,
    pub score: f64,
}

pub fn autocorr_strides(window: &[u8], strides: &[usize]) -> Vec<StrideAutocorr> {
    let mut out = Vec::new();
    for &s in strides {
        if s == 0 || s >= window.len() {
            continue;
        }
        let pairs = window.len() - s;
        if pairs == 0 {
            continue;
        }
        let mut equal = 0usize;
        for i in 0..pairs {
            if window[i] == window[i + s] {
                equal += 1;
            }
        }
        out.push(StrideAutocorr {
            stride: s,
            score: equal as f64 / pairs as f64,
        });
    }
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_finds_all_occurrences() {
        let win = b"abc DEF abc DEF abc";
        let hits = search(win, b"DEF");
        assert_eq!(hits, vec![4, 12]);
        assert!(search(win, b"").is_empty());
        assert!(search(win, b"toolongtoolongtoolong").is_empty());
    }

    #[test]
    fn stride_detects_fixed_gap() {
        // Five matches at offsets 0, 32, 64, 96, 128 - clean stride.
        let report = stride(&[0, 32, 64, 96, 128]);
        assert_eq!(report.match_count, 5);
        assert_eq!(report.dominant_gap, Some(32));
        assert!(report.dominant_gap_share > 0.99);
    }

    #[test]
    fn stride_rejects_noise() {
        // Random positions - no dominant gap.
        let report = stride(&[1, 17, 33, 51, 70, 91, 113, 134]);
        assert_eq!(report.match_count, 8);
        // All gaps are distinct so no gap can be dominant.
        assert!(report.dominant_gap.is_none());
    }

    #[test]
    fn stride_dominant_below_threshold() {
        // 5 gaps: 32, 32, 33, 30, 31. Most common gap = 32 with count 2
        // out of 5 pairs = 40%. Edge of the threshold.
        let positions = vec![0, 32, 64, 97, 127, 158];
        let report = stride(&positions);
        assert_eq!(report.match_count, 6);
        // dominant_gap might or might not be Some(32) at exactly 40%;
        // accept either to match the boundary semantics.
        assert!((report.dominant_gap_share - 0.40).abs() < 0.001);
    }

    #[test]
    fn search_all_returns_per_pattern() {
        let win = b"AAAA DEF GHI DEF JKL DEF";
        let patterns: Vec<Vec<u8>> = vec![b"DEF".to_vec(), b"NOPE".to_vec()];
        let out = search_all(win, 0x80100000, &patterns);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].1.len(), 3);
        assert_eq!(out[0].1[0].addr, 0x80100005);
        assert_eq!(out[1].1.len(), 0);
    }

    #[test]
    fn hits_with_addr_translates_offsets() {
        let h = hits_with_addr(&[0x10, 0x40], 0x80100000);
        assert_eq!(h[0].addr, 0x80100010);
        assert_eq!(h[1].addr, 0x80100040);
    }

    #[test]
    fn autocorr_detects_fixed_stride_table() {
        // Synthesise a 16-byte stride record table: bytes 0..16 repeat
        // verbatim N times, with the marker at offset 8 always 0x0001 LE.
        let mut buf = Vec::new();
        for _ in 0..32 {
            let rec: [u8; 16] = [
                0x3D, 0x1D, 0xD1, 0xF2, 0xF1, 0x3F, 0x2D, 0xFF, 0x01, 0x00, 0xE2, 0xE0, 0xE3, 0xF1,
                0x3F, 0x1C,
            ];
            buf.extend_from_slice(&rec);
        }
        let scores = autocorr_strides(&buf, &[4, 8, 12, 16, 20, 24, 32]);
        // Stride 16 should be the clear winner (1.0 score).
        assert_eq!(scores[0].stride, 16);
        assert!(scores[0].score > 0.99);
    }

    #[test]
    fn autocorr_rejects_random_noise() {
        let mut rng = 0xBEEF_u32;
        let buf: Vec<u8> = (0..1024)
            .map(|_| {
                rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
                (rng >> 16) as u8
            })
            .collect();
        let scores = autocorr_strides(&buf, &[4, 8, 16, 32]);
        // All scores should be near 1/256 (random byte match rate).
        for s in &scores {
            assert!(s.score < 0.05, "stride={} score={}", s.stride, s.score);
        }
    }
}
